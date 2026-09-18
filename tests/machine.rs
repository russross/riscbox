use riscbox::browser_storage::HttpBlockStore;
use riscbox::cpu::CpuBus;
use riscbox::machine::{
    BootImages, FRAMEBUFFER_BASE, FramebufferConfig, Machine, MachineConfig, RAM_BASE,
};
use riscbox::memory::{AccessWidth, GuestAddress};
use riscbox::platform::FinishStatus;

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
}
