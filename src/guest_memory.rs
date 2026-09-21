//! Device access to the `TinyEMU` physical map and its RAM.

use crate::memory::{
    AccessWidth, DeviceWidths, DirtySnapshot, GuestAddress, MemoryAccess, MemoryError, RamFlags,
    RegionId, PAGE_SIZE,
};
use crate::tinyemu_core::{Core, CoreHandle};

#[derive(Clone, Copy)]
struct RamRegion {
    id: RegionId,
    len: u64,
    flags: RamFlags,
}

pub struct GuestMemory {
    core: CoreHandle,
    ram: Vec<RamRegion>,
}

impl GuestMemory {
    pub(crate) fn new(core: &Core) -> Self {
        Self {
            core: core.handle(),
            ram: Vec::new(),
        }
    }

    /// Registers an allocated `RAM` region.
    ///
    /// # Errors
    /// Returns an error for an invalid size or failed core allocation.
    pub fn register_ram(
        &mut self,
        base: GuestAddress,
        len: u64,
        flags: RamFlags,
    ) -> Result<RegionId, MemoryError> {
        if len == 0 || !len.is_multiple_of(PAGE_SIZE) {
            return Err(MemoryError::InvalidRamSize(len));
        }
        let id = self
            .core
            .register_ram(base.0, len, i32::from(flags.bits()))
            .ok_or(MemoryError::ArenaTooLarge)?;
        let id = RegionId(id);
        self.ram.push(RamRegion { id, len, flags });
        Ok(id)
    }

    /// Registers a Rust device aperture in the C physical map.
    ///
    /// # Errors
    /// Returns an error when the physical map is full.
    pub fn register_device(
        &mut self,
        base: GuestAddress,
        len: u64,
        widths: DeviceWidths,
    ) -> Result<RegionId, MemoryError> {
        let id = self
            .core
            .register_device(base.0, len, i32::from(widths.bits()))
            .ok_or(MemoryError::RegionLimit)?;
        Ok(RegionId(id))
    }

    /// Borrows a complete guest RAM range.
    ///
    /// # Errors
    /// Returns an error when the range is unmapped or read-only for a write.
    pub fn ram_range(
        &mut self,
        address: GuestAddress,
        len: usize,
        write: bool,
    ) -> Result<&mut [u8], MemoryError> {
        self.core
            .ram_range(address.0, len, write)
            .ok_or(MemoryError::Unmapped(address))
    }

    /// Borrows a complete guest RAM range for reading.
    ///
    /// # Errors
    /// Returns an error when the range is unmapped.
    pub fn ram_view(&self, address: GuestAddress, len: usize) -> Result<&[u8], MemoryError> {
        self.core
            .ram_view(address.0, len)
            .ok_or(MemoryError::Unmapped(address))
    }

    /// Returns and clears the active dirty-page bitmap.
    ///
    /// # Errors
    /// Returns an error for an invalid or non-RAM region.
    pub fn take_dirty_pages(&mut self, id: RegionId) -> Result<DirtySnapshot, MemoryError> {
        let region = self
            .ram
            .iter()
            .find(|region| region.id == id)
            .ok_or(MemoryError::InvalidRegion(id.0))?;
        if !region.flags.contains(RamFlags::DIRTY_TRACKING.bits()) {
            return Ok(DirtySnapshot {
                words: Vec::new(),
                invalidation: None,
            });
        }
        let count = usize::try_from(region.len.div_ceil(PAGE_SIZE).div_ceil(32))
            .map_err(|_| MemoryError::ArenaTooLarge)?;
        let words = self
            .core
            .take_dirty(id.0, count)
            .ok_or(MemoryError::InvalidRegion(id.0))?;
        Ok(DirtySnapshot {
            words,
            invalidation: None,
        })
    }

    /// Clears the dirty bit for one guest page.
    ///
    /// # Errors
    /// Returns an error for an invalid region or offset.
    pub fn clear_dirty_page(&mut self, id: RegionId, offset: u64) -> Result<(), MemoryError> {
        if self.core.clear_dirty(id.0, offset) {
            Ok(())
        } else {
            Err(MemoryError::InvalidRegion(id.0))
        }
    }
}

impl MemoryAccess for GuestMemory {
    fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, MemoryError> {
        let bytes = self.ram_range(address, width.bytes(), false)?;
        let mut value = [0; 8];
        value[..bytes.len()].copy_from_slice(bytes);
        Ok(u64::from_le_bytes(value))
    }

    fn write(
        &mut self,
        address: GuestAddress,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), MemoryError> {
        self.ram_range(address, width.bytes(), true)?
            .copy_from_slice(&value.to_le_bytes()[..width.bytes()]);
        Ok(())
    }

    fn validate_ram(
        &mut self,
        address: GuestAddress,
        len: usize,
        write: bool,
    ) -> Result<(), MemoryError> {
        self.ram_range(address, len, write).map(|_| ())
    }

    fn read_bytes(
        &mut self,
        address: GuestAddress,
        bytes: &mut [u8],
    ) -> Result<(), MemoryError> {
        bytes.copy_from_slice(self.ram_range(address, bytes.len(), false)?);
        Ok(())
    }

    fn write_bytes(&mut self, address: GuestAddress, bytes: &[u8]) -> Result<(), MemoryError> {
        self.ram_range(address, bytes.len(), true)?
            .copy_from_slice(bytes);
        Ok(())
    }
}
