//! Physical address map and fixed guest-memory arena.

use core::fmt;

pub const PAGE_SIZE: u64 = 4096;
const PAGE_SIZE_U32: u32 = 4096;
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
pub struct RegionId(usize);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RamFlags(u8);

impl RamFlags {
    const ROM_BIT: u8 = 1 << 0;
    const DIRTY_TRACKING_BIT: u8 = 1 << 1;
    const DISABLED_BIT: u8 = 1 << 2;

    pub const ROM: Self = Self(Self::ROM_BIT);
    pub const DIRTY_TRACKING: Self = Self(Self::DIRTY_TRACKING_BIT);
    pub const DISABLED: Self = Self(Self::DISABLED_BIT);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn contains(self, bit: u8) -> bool {
        self.0 & bit != 0
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

#[derive(Clone, Debug)]
enum RegionKind {
    Ram {
        arena_offset: ArenaOffset,
        flags: RamFlags,
        dirty: Option<DirtyPages>,
    },
    Device {
        widths: DeviceWidths,
    },
}

#[derive(Clone, Debug)]
struct Region {
    base: GuestAddress,
    original_len: u64,
    mapped_len: u64,
    kind: RegionKind,
}

#[derive(Clone, Debug)]
struct DirtyPages {
    active: usize,
    buffers: [Vec<u32>; 2],
}

impl DirtyPages {
    fn new(byte_len: u64) -> Self {
        let page_count = byte_len / PAGE_SIZE;
        let word_count = usize::try_from(page_count.div_ceil(32))
            .expect("a RAM region that fits the arena has a representable dirty bitmap");
        Self {
            active: 0,
            buffers: [vec![0; word_count], vec![0; word_count]],
        }
    }

    fn mark(&mut self, byte_offset: u64) {
        let page = usize::try_from(byte_offset / PAGE_SIZE)
            .expect("a RAM offset that fits the arena has a representable page index");
        self.buffers[self.active][page / 32] |= 1_u32 << (page % 32);
    }

    fn clear(&mut self, byte_offset: u64) -> bool {
        let page = usize::try_from(byte_offset / PAGE_SIZE)
            .expect("a RAM offset that fits the arena has a representable page index");
        let word = &mut self.buffers[self.active][page / 32];
        let mask = 1_u32 << (page % 32);
        let was_dirty = *word & mask != 0;
        *word &= !mask;
        was_dirty
    }

    fn snapshot(&mut self) -> (Vec<u32>, bool) {
        let previous = self.active;
        self.active ^= 1;
        self.buffers[self.active].fill(0);
        let words = self.buffers[previous].clone();
        let has_dirty_pages = words.iter().any(|word| *word != 0);
        (words, has_dirty_pages)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PhysicalMemory {
    arena: Vec<u8>,
    regions: Vec<Region>,
}

impl PhysicalMemory {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an allocated RAM region to the physical address map.
    ///
    /// # Errors
    ///
    /// Returns an error when the region limit is reached, the size is zero or
    /// not page aligned, or the fixed arena would exceed the 32-bit offset
    /// space.
    pub fn register_ram(
        &mut self,
        base: GuestAddress,
        len: u64,
        flags: RamFlags,
    ) -> Result<RegionId, MemoryError> {
        self.check_region_capacity()?;
        if len == 0 || !len.is_multiple_of(PAGE_SIZE) {
            return Err(MemoryError::InvalidRamSize(len));
        }
        let arena_offset =
            u32::try_from(self.arena.len()).map_err(|_| MemoryError::ArenaTooLarge)?;
        let new_len = self
            .arena
            .len()
            .checked_add(usize::try_from(len).map_err(|_| MemoryError::ArenaTooLarge)?)
            .ok_or(MemoryError::ArenaTooLarge)?;
        if new_len > u32::MAX as usize {
            return Err(MemoryError::ArenaTooLarge);
        }
        self.arena.resize(new_len, 0);

        let mapped_len = if flags.contains(RamFlags::DISABLED_BIT) {
            0
        } else {
            len
        };
        let dirty = flags
            .contains(RamFlags::DIRTY_TRACKING_BIT)
            .then(|| DirtyPages::new(len));
        let stored_flags = RamFlags(flags.0 & !RamFlags::DISABLED_BIT);
        let id = RegionId(self.regions.len());
        self.regions.push(Region {
            base,
            original_len: len,
            mapped_len,
            kind: RegionKind::Ram {
                arena_offset: ArenaOffset(arena_offset),
                flags: stored_flags,
                dirty,
            },
        });
        Ok(id)
    }

    /// Adds a device region to the physical address map.
    ///
    /// # Errors
    ///
    /// Returns an error when the region limit is reached or the region is
    /// larger than the C device interface can represent.
    pub fn register_device(
        &mut self,
        base: GuestAddress,
        len: u64,
        widths: DeviceWidths,
        enabled: bool,
    ) -> Result<RegionId, MemoryError> {
        self.check_region_capacity()?;
        if len > u64::from(u32::MAX) {
            return Err(MemoryError::RegionTooLarge(len));
        }
        let id = RegionId(self.regions.len());
        self.regions.push(Region {
            base,
            original_len: len,
            mapped_len: if enabled { len } else { 0 },
            kind: RegionKind::Device { widths },
        });
        Ok(id)
    }

    #[must_use]
    pub fn region_at(&self, address: GuestAddress) -> Option<RegionId> {
        self.regions
            .iter()
            .position(|region| {
                address.0 >= region.base.0
                    && address.0.wrapping_sub(region.base.0) < region.mapped_len
            })
            .map(RegionId)
    }

    pub fn ram_offset(&mut self, address: GuestAddress, write: bool) -> Option<ArenaOffset> {
        self.ram_range(address, 1, write).ok()
    }

    /// Resolves a complete physical RAM range to its arena offset.
    ///
    /// A successful write lookup marks every touched page dirty.
    ///
    /// # Errors
    ///
    /// Returns an error when the range is unmapped, crosses a region boundary,
    /// resolves to a device, is read-only, or cannot be represented by an arena
    /// offset.
    pub fn ram_range(
        &mut self,
        address: GuestAddress,
        len: usize,
        write: bool,
    ) -> Result<ArenaOffset, MemoryError> {
        let id = self
            .region_at(address)
            .ok_or(MemoryError::Unmapped(address))?;
        let region = &mut self.regions[id.0];
        let byte_offset = address.0 - region.base.0;
        let RegionKind::Ram {
            arena_offset,
            flags,
            dirty,
        } = &mut region.kind
        else {
            return Err(MemoryError::WrongRegionKind);
        };
        let len = u64::try_from(len).map_err(|_| MemoryError::ArenaTooLarge)?;
        let end = byte_offset
            .checked_add(len)
            .ok_or(MemoryError::Unmapped(address))?;
        if end > region.mapped_len {
            return Err(MemoryError::Unmapped(address));
        }
        if write && flags.contains(RamFlags::ROM_BIT) {
            return Err(MemoryError::ReadOnly);
        }
        if write
            && len != 0
            && let Some(dirty) = dirty
        {
            let last = end - 1;
            let mut page_offset = byte_offset & !(PAGE_SIZE - 1);
            loop {
                dirty.mark(page_offset);
                if page_offset / PAGE_SIZE == last / PAGE_SIZE {
                    break;
                }
                page_offset += PAGE_SIZE;
            }
        }
        let offset = arena_offset
            .0
            .checked_add(u32::try_from(byte_offset).map_err(|_| MemoryError::ArenaTooLarge)?)
            .ok_or(MemoryError::ArenaTooLarge)?;
        Ok(ArenaOffset(offset))
    }

    /// Reads a little-endian value from physical RAM.
    ///
    /// # Errors
    ///
    /// Returns an error when the complete value does not resolve to RAM.
    pub fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, MemoryError> {
        let len = width.bytes();
        let offset = self.ram_range(address, len, false)?.0 as usize;
        let bytes = &self.arena[offset..offset + len];
        let mut value = [0_u8; 8];
        value[..len].copy_from_slice(bytes);
        Ok(u64::from_le_bytes(value))
    }

    /// Writes a little-endian value to physical RAM.
    ///
    /// # Errors
    ///
    /// Returns an error when the complete value does not resolve to writable
    /// RAM.
    pub fn write(
        &mut self,
        address: GuestAddress,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), MemoryError> {
        let len = width.bytes();
        let offset = self.ram_range(address, len, true)?.0 as usize;
        self.arena[offset..offset + len].copy_from_slice(&value.to_le_bytes()[..len]);
        Ok(())
    }

    /// Enables, disables, or moves an existing mapping.
    ///
    /// Returns the RAM range whose cached write translations must be
    /// invalidated. Device mapping changes do not require an invalidation.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` does not identify an existing region or a
    /// RAM region length cannot be represented by the arena.
    pub fn set_mapping(
        &mut self,
        id: RegionId,
        base: GuestAddress,
        enabled: bool,
    ) -> Result<Option<Invalidation>, MemoryError> {
        let region = self.region_mut(id)?;
        let changed = if enabled {
            region.mapped_len == 0 || region.base != base
        } else {
            region.mapped_len != 0
        };
        if !changed {
            return Ok(None);
        }

        let invalidation = ram_invalidation(region)?;
        if enabled {
            region.base = base;
            region.mapped_len = region.original_len;
        } else {
            region.base = GuestAddress(0);
            region.mapped_len = 0;
        }
        Ok(invalidation)
    }

    /// Swaps the active dirty bitmap and returns the completed bitmap.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` is invalid, identifies a device, or the RAM
    /// region length cannot be represented by the arena.
    pub fn take_dirty_pages(&mut self, id: RegionId) -> Result<DirtySnapshot, MemoryError> {
        let region = self.region_mut(id)?;
        let RegionKind::Ram {
            arena_offset,
            dirty,
            ..
        } = &mut region.kind
        else {
            return Err(MemoryError::WrongRegionKind);
        };
        let Some(dirty) = dirty else {
            return Ok(DirtySnapshot {
                words: Vec::new(),
                invalidation: None,
            });
        };
        let (words, has_dirty_pages) = dirty.snapshot();
        let invalidation = if has_dirty_pages && region.mapped_len != 0 {
            Some(Invalidation {
                arena_offset: *arena_offset,
                len: u32::try_from(region.original_len).map_err(|_| MemoryError::ArenaTooLarge)?,
            })
        } else {
            None
        };
        Ok(DirtySnapshot {
            words,
            invalidation,
        })
    }

    /// Clears the dirty bit for the page containing `byte_offset`.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` is invalid, identifies a device, or the page
    /// offset cannot be represented by the arena.
    pub fn clear_dirty_page(
        &mut self,
        id: RegionId,
        byte_offset: u64,
    ) -> Result<Option<Invalidation>, MemoryError> {
        let region = self.region_mut(id)?;
        if byte_offset >= region.original_len {
            return Ok(None);
        }
        let RegionKind::Ram {
            arena_offset,
            dirty,
            ..
        } = &mut region.kind
        else {
            return Err(MemoryError::WrongRegionKind);
        };
        let was_dirty = dirty.as_mut().is_some_and(|dirty| dirty.clear(byte_offset));
        if !was_dirty {
            return Ok(None);
        }
        let page_offset = byte_offset & !(PAGE_SIZE - 1);
        let page_offset = u32::try_from(page_offset).map_err(|_| MemoryError::ArenaTooLarge)?;
        let invalidation_offset = arena_offset
            .0
            .checked_add(page_offset)
            .ok_or(MemoryError::ArenaTooLarge)?;
        Ok(Some(Invalidation {
            arena_offset: ArenaOffset(invalidation_offset),
            len: PAGE_SIZE_U32,
        }))
    }

    #[must_use]
    pub fn arena(&self) -> &[u8] {
        &self.arena
    }

    pub fn arena_mut(&mut self) -> &mut [u8] {
        &mut self.arena
    }

    /// Returns the flags stored for a RAM region.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` is invalid or identifies a device.
    pub fn ram_flags(&self, id: RegionId) -> Result<RamFlags, MemoryError> {
        let region = self.region(id)?;
        let RegionKind::Ram { flags, .. } = region.kind else {
            return Err(MemoryError::WrongRegionKind);
        };
        Ok(flags)
    }

    /// Returns the permitted access widths for a device region.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` is invalid or identifies RAM.
    pub fn device_widths(&self, id: RegionId) -> Result<DeviceWidths, MemoryError> {
        let region = self.region(id)?;
        let RegionKind::Device { widths } = region.kind else {
            return Err(MemoryError::WrongRegionKind);
        };
        Ok(widths)
    }

    fn check_region_capacity(&self) -> Result<(), MemoryError> {
        if self.regions.len() == MAX_REGIONS {
            Err(MemoryError::RegionLimit)
        } else {
            Ok(())
        }
    }

    fn region(&self, id: RegionId) -> Result<&Region, MemoryError> {
        self.regions
            .get(id.0)
            .ok_or(MemoryError::InvalidRegion(id.0))
    }

    fn region_mut(&mut self, id: RegionId) -> Result<&mut Region, MemoryError> {
        self.regions
            .get_mut(id.0)
            .ok_or(MemoryError::InvalidRegion(id.0))
    }
}

fn ram_invalidation(region: &Region) -> Result<Option<Invalidation>, MemoryError> {
    let RegionKind::Ram { arena_offset, .. } = region.kind else {
        return Ok(None);
    };
    Ok(Some(Invalidation {
        arena_offset,
        len: u32::try_from(region.original_len).map_err(|_| MemoryError::ArenaTooLarge)?,
    }))
}
