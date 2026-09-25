use riscbox::tinyemu_core::{BusError, Core, HostCallbacks};

#[derive(Default)]
struct Device {
    reads: Vec<(u64, u32)>,
    writes: Vec<(u64, u32, u32)>,
    attention: bool,
}

impl HostCallbacks for Device {
    fn read(&mut self, address: u64, width: u32) -> Result<u32, BusError> {
        self.reads.push((address, width));
        Ok(0x1234_5678)
    }

    fn write(&mut self, address: u64, width: u32, value: u32) -> Result<(), BusError> {
        self.writes.push((address, width, value));
        Ok(())
    }

    fn interrupts(&self) -> u32 {
        0
    }

    fn host_attention(&self) -> bool {
        self.attention && !self.writes.is_empty()
    }
}

#[test]
fn c_core_exits_after_the_mmio_store_retires_and_resumes_the_unused_budget() {
    let mut core = Core::new().expect("C core allocation");
    core.register_ram(0, 0x2000, 0).expect("instruction RAM");
    core.register_device(0x1000_0000, 0x1000, 4)
        .expect("device aperture");
    let firmware = core.ram_range(0x1000, 16, true).expect("firmware");
    for (bytes, instruction) in
        firmware
            .chunks_exact_mut(4)
            .zip([0x1000_0137_u32, 0x02a0_0093, 0x0011_2023, 0x1050_0073])
    {
        bytes.copy_from_slice(&instruction.to_le_bytes());
    }
    let mut device = Device {
        attention: true,
        ..Device::default()
    };
    let first = core.run_host(20, &mut device);
    assert_eq!((first.cycles, first.reason, core.pc()), (3, 2, 0x100c));
    assert_eq!(device.writes, [(0x1000_0000, 4, 42)]);
    let second = core.run_host(20 - first.cycles, &mut device);
    assert_eq!((second.cycles, second.reason), (1, 1));
}

#[test]
fn c_core_code_block_overrun_fits_the_browser_turn_guard() {
    let mut core = Core::new().expect("C core allocation");
    core.register_ram(0, 0x3000, 0).expect("instruction RAM");
    core.ram_range(0x1000, 0x1000, true)
        .expect("code page")
        .chunks_exact_mut(2)
        .for_each(|bytes| bytes.copy_from_slice(&0x0001_u16.to_le_bytes()));
    let result = core.run(1);
    assert!(result.cycles > 1);
    assert!(result.cycles <= 4_096);
}

#[test]
fn c_core_executes_from_owned_ram_and_waits() {
    let mut core = Core::new().expect("C core allocation");
    core.register_ram(0, 0x2000, 0).expect("reset RAM");
    let bytes = core.ram_range(0x1000, 8, true).expect("instruction RAM");
    bytes[..4].copy_from_slice(&0x02a0_0093_u32.to_le_bytes());
    bytes[4..].copy_from_slice(&0x1050_0073_u32.to_le_bytes());

    let result = core.run(10);
    assert_eq!(core.register(1), 42);
    assert_eq!(result.reason, 1);
    assert!(result.cycles >= 2);
}

#[test]
fn c_core_routes_device_accesses_without_leaving_the_slice() {
    let mut core = Core::new().expect("C core allocation");
    core.register_ram(0, 0x2000, 0).expect("reset RAM");
    core.register_device(0x1000_0000, 0x1000, 4)
        .expect("device aperture");
    let bytes = core.ram_range(0x1000, 16, true).expect("instruction RAM");
    for (chunk, instruction) in
        bytes
            .chunks_exact_mut(4)
            .zip([0x1000_0137_u32, 0x0001_2083, 0x0011_2223, 0x1050_0073])
    {
        chunk.copy_from_slice(&instruction.to_le_bytes());
    }
    let mut device = Device::default();

    let outcome = core.run_host(20, &mut device);

    assert_eq!(outcome.reason, 1);
    assert_eq!(core.register(1), 0x1234_5678);
    assert_eq!(device.reads, [(0x1000_0000, 4)]);
    assert_eq!(device.writes, [(0x1000_0004, 4, 0x1234_5678)]);
}

#[test]
fn c_core_ram_bounds_read_only_flags_and_region_validation() {
    let mut core = Core::new().expect("C core allocation");
    assert!(core.register_ram(0x8000_0000, 0, 0).is_none());
    assert!(core.register_ram(0x8000_0000, 0x1001, 0).is_none());
    assert!(core.register_ram(u64::MAX - 0xfff, 0x2000, 0).is_none());

    core.register_ram(0x8000_0000, 0x2000, 0)
        .expect("writable RAM");
    core.register_ram(0x9000_0000, 0x1000, 1)
        .expect("read-only RAM");
    assert!(core.ram_range(0x8000_0000, 0x2000, true).is_some());
    assert!(core.ram_range(0x8000_1fff, 2, false).is_none());
    assert!(core.ram_range(0x9000_0000, 1, true).is_none());
    assert_eq!(core.ram_range(0x9000_0000, 1, false).unwrap(), &[0]);
}

#[test]
fn c_core_dirty_pages_snapshot_and_clear_follow_guest_writes() {
    let mut core = Core::new().expect("C core allocation");
    core.register_ram(0x9000_0000, 33 * 0x1000, 2)
        .expect("dirty-tracked RAM");

    core.ram_range(0x9000_0000, 1, true).unwrap()[0] = 1;
    core.ram_range(0x9000_1000, 1, true).unwrap()[0] = 2;
    core.ram_range(0x9002_0000, 1, true).unwrap()[0] = 3;
    assert_eq!(core.take_dirty(0, 2), Some(vec![3, 1]));
    assert_eq!(core.take_dirty(0, 2), Some(vec![0, 0]));

    core.ram_range(0x9000_1000, 1, true).unwrap()[0] = 4;
    assert!(core.clear_dirty(0, 0x1001));
    assert_eq!(core.take_dirty(0, 2), Some(vec![0, 0]));
    assert!(!core.clear_dirty(0, 33 * 0x1000));
}
