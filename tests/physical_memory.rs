use riscbox::guest_memory::{
    ArenaOffset, DeviceWidths, DirtySnapshot, GuestAddress, Invalidation, MemoryError, RamFlags,
};
use riscbox::memory::PhysicalMemory;

#[test]
fn maps_ram_and_devices_in_registration_order() {
    let mut memory = PhysicalMemory::new();
    let ram = memory
        .register_ram(GuestAddress(0x8000_0000), 0x2000, RamFlags::default())
        .unwrap();
    let device = memory
        .register_device(
            GuestAddress(0x1000_0000),
            0x100,
            DeviceWidths::U8.union(DeviceWidths::U32),
            true,
        )
        .unwrap();

    assert_eq!(
        memory.ram_offset(GuestAddress(0x8000_0000), false),
        Some(ArenaOffset(0))
    );
    assert_eq!(
        memory.ram_offset(GuestAddress(0x8000_1fff), false),
        Some(ArenaOffset(0x1fff))
    );
    assert_eq!(memory.region_at(GuestAddress(0x8000_2000)), None);
    assert_eq!(memory.ram_offset(GuestAddress(0x1000_0000), false), None);
    assert_eq!(memory.ram_flags(ram), Ok(RamFlags::default()));
    assert_eq!(
        memory.device_widths(device),
        Ok(DeviceWidths::U8.union(DeviceWidths::U32))
    );
    assert_eq!(memory.arena().len(), 0x2000);
    assert!(memory.arena().iter().all(|byte| *byte == 0));
}

#[test]
fn disabled_and_moved_mappings_produce_ram_invalidations() {
    let mut memory = PhysicalMemory::new();
    let ram = memory
        .register_ram(GuestAddress(0x8000_0000), 0x1000, RamFlags::DISABLED)
        .unwrap();

    assert_eq!(memory.region_at(GuestAddress(0x8000_0000)), None);
    assert_eq!(
        memory.set_mapping(ram, GuestAddress(0x8100_0000), true),
        Ok(Some(Invalidation {
            arena_offset: ArenaOffset(0),
            len: 0x1000,
        }))
    );
    assert_eq!(
        memory.ram_offset(GuestAddress(0x8100_0000), false),
        Some(ArenaOffset(0))
    );
    assert_eq!(
        memory.set_mapping(ram, GuestAddress(0x8100_0000), true),
        Ok(None)
    );
    assert_eq!(
        memory.set_mapping(ram, GuestAddress(0), false),
        Ok(Some(Invalidation {
            arena_offset: ArenaOffset(0),
            len: 0x1000,
        }))
    );
    assert_eq!(memory.region_at(GuestAddress(0x8100_0000)), None);
}

#[test]
fn dirty_pages_snapshot_and_reset_match_the_c_double_buffer() {
    let mut memory = PhysicalMemory::new();
    let first = memory
        .register_ram(GuestAddress(0x8000_0000), 0x1000, RamFlags::default())
        .unwrap();
    let tracked = memory
        .register_ram(
            GuestAddress(0x9000_0000),
            33 * 0x1000,
            RamFlags::DIRTY_TRACKING,
        )
        .unwrap();

    assert_eq!(
        memory.ram_offset(GuestAddress(0x9000_0000), true),
        Some(ArenaOffset(0x1000))
    );
    assert_eq!(
        memory.ram_offset(GuestAddress(0x9002_0000), true),
        Some(ArenaOffset(0x2_1000))
    );
    assert_eq!(
        memory.take_dirty_pages(tracked),
        Ok(DirtySnapshot {
            words: vec![1, 1],
            invalidation: Some(Invalidation {
                arena_offset: ArenaOffset(0x1000),
                len: 33 * 0x1000,
            }),
        })
    );
    assert_eq!(memory.take_dirty_pages(tracked).unwrap().words, vec![0, 0]);
    assert_eq!(
        memory.take_dirty_pages(first).unwrap().words,
        Vec::<u32>::new()
    );

    memory.ram_offset(GuestAddress(0x9000_1000), true).unwrap();
    assert_eq!(
        memory.clear_dirty_page(tracked, 0x1001),
        Ok(Some(Invalidation {
            arena_offset: ArenaOffset(0x2000),
            len: 0x1000,
        }))
    );
    assert_eq!(memory.clear_dirty_page(tracked, 0x1001), Ok(None));
    assert_eq!(memory.take_dirty_pages(tracked).unwrap().words, vec![0, 0]);
}

#[test]
fn rejects_invalid_region_definitions() {
    let mut memory = PhysicalMemory::new();
    assert_eq!(
        memory.register_ram(GuestAddress(0), 0, RamFlags::default()),
        Err(MemoryError::InvalidRamSize(0))
    );
    assert_eq!(
        memory.register_ram(GuestAddress(0), 1, RamFlags::default()),
        Err(MemoryError::InvalidRamSize(1))
    );
    assert_eq!(
        memory.register_device(
            GuestAddress(0),
            u64::from(u32::MAX) + 1,
            DeviceWidths::U32,
            true
        ),
        Err(MemoryError::RegionTooLarge(u64::from(u32::MAX) + 1))
    );
}
