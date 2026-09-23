use crate::guest_memory::AccessWidth;
use core::ptr;

#[cfg_attr(target_arch = "wasm32", inline(always))]
pub(crate) fn read_u16(arena: &[u8], offset: usize) -> u16 {
    // SAFETY: The execute-TLB fill proved the complete page containing offset
    // is inside the fixed arena, and this two-byte access cannot cross it. The
    // arena allocation is stable for Cpu::run; read_unaligned requires no host
    // alignment and compiles to one scalar WebAssembly load.
    unsafe { ptr::read_unaligned(arena.as_ptr().add(offset).cast::<u16>()).to_le() }
}

#[cfg_attr(target_arch = "wasm32", inline(always))]
pub(crate) fn read_u32(arena: &[u8], offset: usize) -> u32 {
    // SAFETY: The execute-TLB fill proved the complete page containing offset
    // is inside the fixed arena, and this four-byte access cannot cross it. The
    // arena allocation is stable for Cpu::run; read_unaligned requires no host
    // alignment and compiles to one scalar WebAssembly load.
    unsafe { ptr::read_unaligned(arena.as_ptr().add(offset).cast::<u32>()).to_le() }
}

#[cfg_attr(target_arch = "wasm32", inline(always))]
pub(crate) fn read_width(arena: &[u8], offset: usize, width: AccessWidth) -> u64 {
    // SAFETY: A matching TLB entry proves the complete page containing offset
    // is inside the fixed arena. These naturally aligned fixed-width accesses
    // cannot cross that page, the arena allocation is stable for Cpu::run, and
    // read_unaligned imposes no additional host alignment requirement. Each
    // arm compiles to one scalar WebAssembly load.
    unsafe {
        let pointer = arena.as_ptr().add(offset);
        match width {
            AccessWidth::Byte => u64::from(ptr::read(pointer)),
            AccessWidth::HalfWord => u64::from(ptr::read_unaligned(pointer.cast::<u16>()).to_le()),
            AccessWidth::Word => u64::from(ptr::read_unaligned(pointer.cast::<u32>()).to_le()),
            AccessWidth::DoubleWord => ptr::read_unaligned(pointer.cast::<u64>()).to_le(),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", inline(always))]
pub(crate) fn write_width(arena: &mut [u8], offset: usize, width: AccessWidth, value: u64) {
    // SAFETY: A matching writable TLB entry proves the complete page containing
    // offset is inside the fixed arena and writable. These naturally aligned
    // fixed-width accesses cannot cross it, the mutable borrow excludes aliases,
    // and write_unaligned imposes no host alignment requirement. Each arm
    // compiles to one scalar WebAssembly store.
    unsafe {
        let pointer = arena.as_mut_ptr().add(offset);
        match width {
            AccessWidth::Byte => ptr::write(pointer, value.to_le_bytes()[0]),
            AccessWidth::HalfWord => {
                let bytes = value.to_le_bytes();
                ptr::write_unaligned(
                    pointer.cast::<u16>(),
                    u16::from_le_bytes([bytes[0], bytes[1]]).to_le(),
                );
            }
            AccessWidth::Word => {
                let bytes = value.to_le_bytes();
                ptr::write_unaligned(
                    pointer.cast::<u32>(),
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]).to_le(),
                );
            }
            AccessWidth::DoubleWord => {
                ptr::write_unaligned(pointer.cast::<u64>(), value.to_le());
            }
        }
    }
}
