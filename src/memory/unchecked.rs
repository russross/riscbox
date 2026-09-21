use super::AccessWidth;

pub(crate) fn read_u16(arena: &[u8], offset: usize) -> u16 {
    // SAFETY: Callers provide an offset in a page validated as wholly inside
    // the fixed arena. A two-byte fetch never crosses that page.
    unsafe {
        u16::from_le_bytes([
            *arena.get_unchecked(offset),
            *arena.get_unchecked(offset + 1),
        ])
    }
}

pub(crate) fn read_u32(arena: &[u8], offset: usize) -> u32 {
    // SAFETY: Callers provide an offset in a page validated as wholly inside
    // the fixed arena. A four-byte fetch never crosses that page.
    unsafe {
        u32::from_le_bytes([
            *arena.get_unchecked(offset),
            *arena.get_unchecked(offset + 1),
            *arena.get_unchecked(offset + 2),
            *arena.get_unchecked(offset + 3),
        ])
    }
}

pub(crate) fn read_width(arena: &[u8], offset: usize, width: AccessWidth) -> u64 {
    let len = width.bytes();
    let mut bytes = [0_u8; 8];
    // SAFETY: A TLB hit proves the complete page is in the arena, and aligned
    // fixed-width accesses cannot cross the page. The destination has eight
    // bytes and every supported width is at most eight bytes.
    unsafe {
        for index in 0..len {
            *bytes.get_unchecked_mut(index) = *arena.get_unchecked(offset + index);
        }
    }
    u64::from_le_bytes(bytes)
}

pub(crate) fn write_width(arena: &mut [u8], offset: usize, width: AccessWidth, value: u64) {
    let len = width.bytes();
    let bytes = value.to_le_bytes();
    // SAFETY: A writable TLB hit proves the complete page is exclusively
    // borrowed in the arena, and aligned fixed-width accesses cannot cross it.
    unsafe {
        for index in 0..len {
            *arena.get_unchecked_mut(offset + index) = *bytes.get_unchecked(index);
        }
    }
}
