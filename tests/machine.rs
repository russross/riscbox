use riscbox::browser_storage::HttpBlockStore;
use riscbox::entropy::{EntropyError, EntropySource};
use riscbox::guest_memory::{AccessWidth, GuestAddress};
use riscbox::machine::{
    BootImages, CLINT_BASE, FRAMEBUFFER_BASE, FramebufferConfig, Machine, MachineConfig, RAM_BASE,
    RTC_BASE, RedrawSpan, VIRTIO_BASE,
};
use riscbox::platform::FinishStatus;
use riscbox::tinyemu_core::CpuRunExitReason;
use riscbox::virtio_devices::{DeviceError, NetworkBackend, NetworkIngress};
use std::cell::RefCell;
use std::rc::Rc;

struct FixedEntropy(u8);

impl EntropySource for FixedEntropy {
    fn fill(&mut self, destination: &mut [u8]) -> Result<(), EntropyError> {
        destination.fill(self.0);
        self.0 = self.0.wrapping_add(1);
        Ok(())
    }
}

#[derive(Default)]
struct Net;

impl NetworkBackend for Net {
    fn transmit(&mut self, _: &[u8]) -> Result<(), DeviceError> {
        Ok(())
    }
}

fn machine(framebuffer: bool) -> Machine {
    Machine::new(MachineConfig {
        ram_size: 64 << 20,
        framebuffer: framebuffer.then_some(FramebufferConfig {
            width: 640,
            height: 480,
        }),
    })
    .expect("valid machine")
}

#[test]
fn virtio_slots_route_mmio_and_appear_in_the_device_tree() {
    let mut machine = machine(false);
    let slot = machine.add_console_device(80, 25).expect("console slot");
    assert_eq!(slot, 0);
    assert_eq!(
        machine
            .bus_mut()
            .read(GuestAddress(0x1000_1000), AccessWidth::Word)
            .expect("VirtIO magic"),
        0x7472_6976
    );
    let layout = machine
        .load_boot(BootImages {
            firmware: &[0; 64],
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("boot layout");
    let tree = machine
        .read_ram(layout.fdt_address, layout.fdt_size as usize)
        .expect("FDT RAM");
    let node = b"virtio@10001000";
    assert!(tree.windows(node.len()).any(|window| window == node));
}

#[test]
fn cpu_reads_virtio_config_bytes_through_the_c_device_aperture() {
    let mut machine = machine(false);
    machine.add_console_device(80, 25).expect("console slot");
    let mut firmware = Vec::new();
    for instruction in [0x1000_1137_u32, 0x1001_4283, 0x1050_0073] {
        firmware.extend_from_slice(&instruction.to_le_bytes());
    }
    machine
        .load_boot(BootImages {
            firmware: &firmware,
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("boot image");

    machine.run_cpu(30);

    assert_eq!(machine.cpu().register(5), 80);
}

#[test]
fn guest_clint_compare_write_exits_for_timer_replanning() {
    let mut machine = machine(false);
    let mut firmware = Vec::new();
    for instruction in [0x0200_40b7_u32, 0x0050_0113, 0x0020_a023, 0x1050_0073] {
        firmware.extend_from_slice(&instruction.to_le_bytes());
    }
    machine
        .load_boot(BootImages {
            firmware: &firmware,
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("boot image");
    let outcome = machine.run_cpu(30);
    assert_eq!(outcome.state, CpuRunExitReason::TimerReprogrammed);
    assert!(outcome.consumed_cycles > 0);
}

#[test]
fn guest_rtc_alarm_write_exits_for_timer_replanning() {
    let mut machine = machine(false);
    let mut firmware = Vec::new();
    for instruction in [0x0010_10b7_u32, 0x0050_0113, 0x0020_a423, 0x1050_0073] {
        firmware.extend_from_slice(&instruction.to_le_bytes());
    }
    machine
        .load_boot(BootImages {
            firmware: &firmware,
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("boot image");
    assert_eq!(
        machine.run_cpu(30).state,
        CpuRunExitReason::TimerReprogrammed
    );
}

#[test]
fn network_slot_has_standard_features_carrier_and_fdt_discovery() {
    let entropy = Rc::new(RefCell::new(FixedEntropy(0x40)));
    let mut machine = Machine::new_with_entropy(
        MachineConfig {
            ram_size: 64 << 20,
            framebuffer: None,
        },
        entropy,
    )
    .expect("valid machine");
    let first_mac = machine.generate_network_mac().expect("first MAC");
    let second_mac = machine.generate_network_mac().expect("second MAC");
    assert_ne!(first_mac, second_mac);
    assert_eq!(first_mac[0] & 3, 2);
    assert_eq!(second_mac[0] & 3, 2);

    let slot = machine
        .add_network_device(Box::new(Net), first_mac)
        .expect("network slot");
    assert_eq!(slot, 0);
    let mmio = VIRTIO_BASE;
    assert_eq!(
        machine
            .bus_mut()
            .read(GuestAddress(mmio + 0x10), AccessWidth::Word)
            .expect("network features"),
        (1 << 5) | (1 << 16)
    );
    machine
        .virtio_network_set_carrier(slot, true)
        .expect("carrier up");
    assert_eq!(
        machine
            .bus_mut()
            .read(GuestAddress(mmio + 0x106), AccessWidth::HalfWord)
            .expect("network status"),
        1
    );
    assert_eq!(
        machine
            .virtio_network_receive(slot, Vec::new())
            .expect("nonfatal invalid host frame"),
        NetworkIngress::Dropped
    );

    let layout = machine
        .load_boot(BootImages {
            firmware: &[0; 64],
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("boot layout");
    let tree = machine
        .read_ram(layout.fdt_address, layout.fdt_size as usize)
        .expect("FDT RAM");
    let node = b"virtio@10001000";
    assert!(tree.windows(node.len()).any(|window| window == node));
}

#[test]
fn boot_seed_and_virtio_rng_share_the_machine_entropy_source() {
    let entropy = Rc::new(RefCell::new(FixedEntropy(0xa5)));
    let mut machine = Machine::new_with_entropy(
        MachineConfig {
            ram_size: 64 << 20,
            framebuffer: None,
        },
        entropy,
    )
    .expect("valid machine");
    let slot = machine.add_entropy_device().expect("entropy slot");
    assert_eq!(slot, 0);
    assert_eq!(
        machine
            .bus_mut()
            .read(GuestAddress(VIRTIO_BASE + 8), AccessWidth::Word)
            .expect("VirtIO device ID"),
        4
    );
    let layout = machine
        .load_boot(BootImages {
            firmware: &[0; 64],
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("boot layout");
    let tree = machine
        .read_ram(layout.fdt_address, layout.fdt_size as usize)
        .expect("FDT RAM");
    assert!(tree.windows(32).any(|window| window == [0xa5; 32]));
}

#[test]
fn machine_routes_http_completions_back_to_a_pending_block_request() {
    const DESC: u64 = RAM_BASE + 0x1000;
    const AVAIL: u64 = RAM_BASE + 0x2000;
    const USED: u64 = RAM_BASE + 0x3000;
    const DATA: u64 = RAM_BASE + 0x4000;
    const MMIO: u64 = 0x1000_1000;

    let mut machine = machine(false);
    let store = HttpBlockStore::from_manifest("disk.json", "{block_size:1,n_block:1}", 1024)
        .expect("valid manifest");
    let slot = machine
        .add_http_block_device(store, *b"riscbox-http-0000000")
        .expect("HTTP block slot");
    for (register, address) in [(0x80, DESC), (0x90, AVAIL), (0xa0, USED)] {
        machine
            .bus_mut()
            .write(
                GuestAddress(MMIO + register),
                AccessWidth::Word,
                address & 0xffff_ffff,
            )
            .expect("low queue address");
        machine
            .bus_mut()
            .write(
                GuestAddress(MMIO + register + 4),
                AccessWidth::Word,
                address >> 32,
            )
            .expect("high queue address");
    }
    for (address, width, value) in [
        (MMIO + 0x44, AccessWidth::Word, 1),
        (MMIO + 0x70, AccessWidth::Word, 4),
        (DESC, AccessWidth::DoubleWord, DATA),
        (DESC + 8, AccessWidth::Word, 16),
        (DESC + 12, AccessWidth::HalfWord, 1),
        (DESC + 14, AccessWidth::HalfWord, 1),
        (DESC + 16, AccessWidth::DoubleWord, DATA + 0x100),
        (DESC + 24, AccessWidth::Word, 513),
        (DESC + 28, AccessWidth::HalfWord, 2),
        (AVAIL + 2, AccessWidth::HalfWord, 1),
    ] {
        machine
            .bus_mut()
            .write(GuestAddress(address), width, value)
            .expect("queue setup");
    }
    machine
        .bus_mut()
        .write(GuestAddress(MMIO + 0x50), AccessWidth::Word, 0)
        .expect("queue notify");
    assert_eq!(
        machine
            .bus_mut()
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .expect("used index"),
        0
    );
    let request = machine
        .next_http_block_request(slot)
        .expect("HTTP block slot")
        .expect("block request");
    machine
        .complete_http_block_request(slot, request.id, vec![0x37; 1024])
        .expect("HTTP completion");
    assert_eq!(
        machine
            .bus_mut()
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .expect("used index"),
        1
    );
    assert_eq!(
        machine.read_ram(DATA + 0x100, 513).expect("block data"),
        &[vec![0x37; 512], vec![0]].concat()
    );
}

#[test]
fn boot_layout_installs_images_tree_and_reset_trampoline() {
    let mut machine = machine(false);
    let firmware = [0xaa; 64];
    let kernel = [0xbb; 128];
    let initrd = [0xcc; 256];
    let layout = machine
        .load_boot(BootImages {
            firmware: &firmware,
            kernel: Some(&kernel),
            initrd: Some(&initrd),
            command_line: "console=ttyS0",
        })
        .expect("images fit");
    assert_eq!(
        machine.read_ram(RAM_BASE, 64).expect("firmware RAM"),
        &firmware
    );
    assert_eq!(
        machine
            .read_ram(RAM_BASE + 0x20_0000, 128)
            .expect("kernel RAM"),
        &kernel
    );
    assert_eq!(
        machine
            .read_ram(layout.initrd_address.expect("initrd"), 256)
            .expect("initrd RAM"),
        &initrd
    );
    assert_eq!(
        machine.read_ram(layout.fdt_address, 4).expect("FDT RAM"),
        &[0xd0, 0x0d, 0xfe, 0xed]
    );
    let reset = machine.read_ram(0x1000, 40).expect("reset RAM");
    assert_eq!(&reset[..4], &0x0000_0297_u32.to_le_bytes());
    assert_eq!(&reset[24..32], &RAM_BASE.to_le_bytes());
    assert_eq!(&reset[32..40], &layout.fdt_address.to_le_bytes());
    assert_eq!(machine.cpu().pc(), 0x1000);
}

#[test]
fn bus_dispatches_uart_and_finisher_mmio() {
    let mut machine = machine(false);
    machine
        .bus_mut()
        .write(
            GuestAddress(0x1000_0000),
            AccessWidth::Byte,
            u64::from(b'A'),
        )
        .expect("UART write");
    assert_eq!(machine.take_console_output(), b"A");
    machine
        .bus_mut()
        .write(GuestAddress(0x10_0000), AccessWidth::Word, 0x5555)
        .expect("finisher write");
    assert_eq!(machine.finish_status(), FinishStatus::Passed);
    machine
        .bus_mut()
        .write(
            GuestAddress(0x0200_4000),
            AccessWidth::DoubleWord,
            0x1234_5678_9abc_def0,
        )
        .expect("64-bit CLINT time comparison write");
    assert_eq!(
        machine
            .bus_mut()
            .read(GuestAddress(0x0200_4000), AccessWidth::DoubleWord)
            .expect("64-bit CLINT time comparison read"),
        0x1234_5678_9abc_def0
    );
}

#[test]
fn framebuffer_dirty_rows_are_merged_and_consumed() {
    let mut machine = machine(true);
    machine
        .bus_mut()
        .write(
            GuestAddress(FRAMEBUFFER_BASE + 10 * 4096),
            AccessWidth::Word,
            1,
        )
        .expect("framebuffer write");
    let spans = machine.take_redraw_spans().expect("dirty snapshot");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].y, 16);
    assert_eq!(spans[0].height, 2);
    assert!(
        machine
            .take_redraw_spans()
            .expect("consumed snapshot")
            .is_empty()
    );

    machine
        .bus_mut()
        .write(
            GuestAddress(FRAMEBUFFER_BASE + 10 * 4096),
            AccessWidth::Word,
            2,
        )
        .expect("second framebuffer write");
    let span = machine.take_redraw_spans().expect("second dirty snapshot")[0];
    let update = machine
        .framebuffer_update(span)
        .expect("framebuffer range")
        .expect("configured framebuffer");
    assert_eq!(
        (
            update.x,
            update.y,
            update.width,
            update.height,
            update.stride
        ),
        (0, 16, 640, 2, 2560)
    );
    assert_eq!(
        machine
            .framebuffer_bytes(update)
            .expect("framebuffer bytes")
            .len(),
        5120
    );
}

#[test]
fn framebuffer_snapshot_invalidates_cached_cpu_write_translation() {
    let mut machine = machine(true);
    let instructions = [
        0x0410_00b7_u32, // lui x1, 0x4100
        0x0010_0113,     // addi x2, x0, 1
        0x0020_a023,     // sw x2, 0(x1)
        0xfe00_0ee3,     // beq x0, x0, -4
    ];
    let firmware: Vec<u8> = instructions
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    machine
        .load_boot(BootImages {
            firmware: &firmware,
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("boot image");

    machine.run_cpu(1_000);
    assert_eq!(
        machine.take_redraw_spans().expect("first dirty snapshot"),
        [RedrawSpan { y: 0, height: 2 }]
    );
    assert!(
        machine
            .take_redraw_spans()
            .expect("consumed snapshot")
            .is_empty()
    );

    machine.run_cpu(1_000);
    assert_eq!(
        machine.take_redraw_spans().expect("second dirty snapshot"),
        [RedrawSpan { y: 0, height: 2 }]
    );
}

#[test]
fn interrupt_changes_from_guest_mmio_end_the_current_cpu_block() {
    let mut machine = machine(false);
    let firmware: Vec<u8> = [
        0x0200_00b7_u32, // lui x1,0x2000 (CLINT)
        0x0010_0113,     // addi x2,x0,1
        0x0020_a023,     // sw x2,0(x1)
        0x3440_22f3,     // csrr x5,mip
        0x0000_a023,     // sw x0,0(x1)
        0x3440_2373,     // csrr x6,mip
        0x0000_006f,     // j .
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    machine
        .load_boot(BootImages {
            firmware: &firmware,
            kernel: None,
            initrd: None,
            command_line: "",
        })
        .expect("boot image");

    for _ in 0..3 {
        machine.run_cpu(100);
    }
    assert_eq!(machine.cpu().register(5) & (1 << 3), 1 << 3);
    assert_eq!(machine.cpu().register(6) & (1 << 3), 0);
}

#[test]
fn machine_exposes_complete_host_time_through_the_rtc() {
    let mut machine = machine(false);
    let milliseconds = 1_730_000_000_123_u64;
    let nanoseconds = milliseconds * 1_000_000;
    machine.present_guest_clocks(milliseconds * 10_000, nanoseconds);

    let low = machine
        .bus_mut()
        .read(GuestAddress(RTC_BASE), AccessWidth::Word)
        .expect("RTC low word");
    let high = machine
        .bus_mut()
        .read(GuestAddress(RTC_BASE + 4), AccessWidth::Word)
        .expect("RTC high word");
    assert_eq!((high << 32) | low, nanoseconds);
}

#[test]
fn timer_deadlines_use_guest_ticks_and_drop_due_compares() {
    let mut machine = machine(false);
    machine.present_guest_clocks(1_000_000, 100_000_000);
    machine
        .bus_mut()
        .write(
            GuestAddress(CLINT_BASE + 0x4000),
            AccessWidth::Word,
            1_000_015,
        )
        .expect("CLINT compare low");
    machine
        .bus_mut()
        .write(GuestAddress(CLINT_BASE + 0x4004), AccessWidth::Word, 0)
        .expect("CLINT compare high");
    assert_eq!(machine.next_timer_remaining_guest_ticks(), Some(15));

    machine
        .bus_mut()
        .write(GuestAddress(RTC_BASE + 8), AccessWidth::Word, 100_000_101)
        .expect("RTC alarm low");
    assert_eq!(machine.next_timer_remaining_guest_ticks(), Some(2));
    machine.present_guest_clocks(1_000_002, 100_000_200);
    assert_eq!(machine.next_timer_remaining_guest_ticks(), Some(13));
    machine.present_guest_clocks(1_000_015, 100_001_500);
    assert!(
        machine
            .next_timer_remaining_guest_ticks()
            .expect("future RTC alarm")
            > u64::from(u32::MAX)
    );
}
