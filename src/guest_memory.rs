//! Shared guest memory types and access to TinyEMU-owned RAM.

use core::fmt;

use crate::tinyemu_core::{Core, CoreHandle};

pub const PAGE_SIZE: u64 = 4096;
pub const MAX_REGIONS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuestAddress(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArenaOffset(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessWidth {
    Byte,
    HalfWord,
    Word,
    DoubleWord,
}

impl AccessWidth {
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            Self::Byte => 1,
            Self::HalfWord => 2,
            Self::Word => 4,
            Self::DoubleWord => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionId(pub(crate) usize);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RamFlags(u8);

impl RamFlags {
    pub(crate) const ROM_BIT: u8 = 1 << 0;
    pub(crate) const DIRTY_TRACKING_BIT: u8 = 1 << 1;
    pub(crate) const DISABLED_BIT: u8 = 1 << 2;

    pub const ROM: Self = Self(Self::ROM_BIT);
    pub const DIRTY_TRACKING: Self = Self(Self::DIRTY_TRACKING_BIT);
    pub const DISABLED: Self = Self(Self::DISABLED_BIT);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn contains(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceWidths(u8);

impl DeviceWidths {
    pub const U8: Self = Self(1 << 0);
    pub const U16: Self = Self(1 << 1);
    pub const U32: Self = Self(1 << 2);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }
}

pub trait MemoryAccess {
    /// Reads a guest value.
    ///
    /// # Errors
    /// Returns an error for an inaccessible range.
    fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, MemoryError>;
    /// Writes a guest value.
    ///
    /// # Errors
    /// Returns an error for an inaccessible or read-only range.
    fn write(
        &mut self,
        address: GuestAddress,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), MemoryError>;
    /// Checks a complete guest RAM range.
    ///
    /// # Errors
    /// Returns an error when the range is not suitable RAM.
    fn validate_ram(
        &mut self,
        address: GuestAddress,
        len: usize,
        write: bool,
    ) -> Result<(), MemoryError>;
    /// Copies bytes out of guest RAM.
    ///
    /// # Errors
    /// Returns an error for an inaccessible range.
    fn read_bytes(&mut self, address: GuestAddress, bytes: &mut [u8]) -> Result<(), MemoryError>;
    /// Copies bytes into guest RAM.
    ///
    /// # Errors
    /// Returns an error for an inaccessible or read-only range.
    fn write_bytes(&mut self, address: GuestAddress, bytes: &[u8]) -> Result<(), MemoryError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Invalidation {
    pub arena_offset: ArenaOffset,
    pub len: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirtySnapshot {
    pub words: Vec<u32>,
    pub invalidation: Option<Invalidation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryError {
    ArenaTooLarge,
    InvalidRamSize(u64),
    InvalidRegion(usize),
    RegionLimit,
    RegionTooLarge(u64),
    ReadOnly,
    Unmapped(GuestAddress),
    WrongRegionKind,
}

impl fmt::Display for MemoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArenaTooLarge => formatter.write_str("guest-memory arena exceeds 4 GiB"),
            Self::InvalidRamSize(size) => {
                write!(
                    formatter,
                    "RAM size {size:#x} is zero or is not page aligned"
                )
            }
            Self::InvalidRegion(index) => write!(formatter, "invalid memory region {index}"),
            Self::RegionLimit => write!(
                formatter,
                "physical memory map exceeds {MAX_REGIONS} regions"
            ),
            Self::RegionTooLarge(size) => {
                write!(formatter, "device region size {size:#x} exceeds 32 bits")
            }
            Self::ReadOnly => formatter.write_str("write to read-only memory"),
            Self::Unmapped(address) => {
                write!(formatter, "unmapped physical address {:#x}", address.0)
            }
            Self::WrongRegionKind => formatter.write_str("operation does not apply to this region"),
        }
    }
}

impl std::error::Error for MemoryError {}

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

    fn read_bytes(&mut self, address: GuestAddress, bytes: &mut [u8]) -> Result<(), MemoryError> {
        bytes.copy_from_slice(self.ram_range(address, bytes.len(), false)?);
        Ok(())
    }

    fn write_bytes(&mut self, address: GuestAddress, bytes: &[u8]) -> Result<(), MemoryError> {
        self.ram_range(address, bytes.len(), true)?
            .copy_from_slice(bytes);
        Ok(())
    }
}
