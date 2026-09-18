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
