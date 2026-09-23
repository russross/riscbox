use riscbox::tinyemu_core::{BusError, Core, HostCallbacks};

#[derive(Default)]
struct Device {
    reads: Vec<(u64, u32)>,
    writes: Vec<(u64, u32, u32)>,
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
    assert_ne!(result.waiting, 0);
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

    assert_ne!(outcome.waiting, 0);
    assert_eq!(core.register(1), 0x1234_5678);
    assert_eq!(device.reads, [(0x1000_0000, 4)]);
    assert_eq!(device.writes, [(0x1000_0004, 4, 0x1234_5678)]);
}
