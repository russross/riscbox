use riscbox::fdt::{FdtConfig, FramebufferDescription, build};

fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

#[test]
fn platform_tree_has_standard_layout_and_required_nodes() {
    let tree = build(FdtConfig {
        ram_size: 256 << 20,
        command_line: "console=ttyS0",
        initrd: Some((0x8800_0000, 0x20_0000)),
        virtio_count: 2,
        framebuffer: Some(FramebufferDescription {
            size: 0x20_0000,
            width: 800,
            height: 600,
            stride: 3200,
        }),
    });
    assert_eq!(be32(&tree, 0), 0xd00d_feed);
    assert_eq!(be32(&tree, 4) as usize, tree.len());
    assert_eq!(be32(&tree, 20), 17);
    assert_eq!(be32(&tree, 24), 16);
    assert_eq!(&tree[40..56], &[0; 16]);
    for text in [
        "cpu@0",
        "memory@80000000",
        "clint@2000000",
        "interrupt-controller@c000000",
        "rtc@101000",
        "serial@10000000",
        "virtio@10001000",
        "virtio@10002000",
        "framebuffer@4100000",
        "console=ttyS0",
    ] {
        assert!(
            tree.windows(text.len())
                .any(|window| window == text.as_bytes()),
            "missing {text}"
        );
    }
}
