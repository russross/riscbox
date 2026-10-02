//! VM-owned sector storage shared by `VirtIO` and powered-off host operations.

use crate::browser_storage::{HttpBlockStore, HttpRequest, StorageError};
use crate::virtio_devices::{BlockBackend, BlockRequestStatus, DeviceError};
use std::ops::Range;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BlockDiskId(pub u32);

enum BlockBase {
    Array(Vec<u8>),
    Http(HttpBlockStore),
}

pub struct BlockStore {
    base: BlockBase,
}

impl BlockStore {
    #[must_use]
    pub const fn http(store: HttpBlockStore) -> Self {
        Self {
            base: BlockBase::Http(store),
        }
    }

    /// Allocates a zero-filled write-through disk with whole-sector capacity.
    /// # Errors
    /// Rejects dimensions that cannot be represented or allocated in memory.
    pub fn array(capacity_sectors: u64) -> Result<Self, StorageError> {
        let length = capacity_sectors
            .checked_mul(512)
            .and_then(|bytes| usize::try_from(bytes).ok())
            .filter(|length| *length > 0)
            .ok_or(StorageError::OutOfRange)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| StorageError::OutOfRange)?;
        bytes.resize(length, 0);
        Ok(Self {
            base: BlockBase::Array(bytes),
        })
    }

    // Both bases use identical range validation and copy semantics. HTTP alone
    // may queue immutable chunks; dirty writes never cross the host boundary.
    /// Reads resident sectors or queues immutable backing chunks.
    /// # Errors
    /// Reports invalid ranges, missing chunks, or failed backing reads.
    pub fn read_sectors(&mut self, sector: u64, output: &mut [u8]) -> Result<(), StorageError> {
        match &mut self.base {
            BlockBase::Array(bytes) => {
                let range = Self::range(sector, output.len(), bytes.len())?;
                output.copy_from_slice(&bytes[range]);
                Ok(())
            }
            BlockBase::Http(store) => store.read_sectors(sector, output),
        }
    }

    /// Writes whole sectors without requesting backing data.
    /// # Errors
    /// Reports invalid ranges or non-sector-sized input.
    pub fn write_sectors(&mut self, sector: u64, input: &[u8]) -> Result<(), StorageError> {
        match &mut self.base {
            BlockBase::Array(bytes) => {
                let range = Self::range(sector, input.len(), bytes.len())?;
                bytes[range].copy_from_slice(input);
                Ok(())
            }
            BlockBase::Http(store) => store.write_sectors(sector, input),
        }
    }

    fn range(sector: u64, length: usize, capacity: usize) -> Result<Range<usize>, StorageError> {
        let start = sector
            .checked_mul(512)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(StorageError::OutOfRange)?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= capacity)
            .ok_or(StorageError::OutOfRange)?;
        if length == 0 || !length.is_multiple_of(512) {
            return Err(StorageError::OutOfRange);
        }
        Ok(start..end)
    }

    pub fn next_request(&mut self) -> Option<HttpRequest> {
        match &mut self.base {
            BlockBase::Array(_) => None,
            BlockBase::Http(store) => store.next_request(),
        }
    }

    #[must_use]
    pub fn has_outgoing(&self) -> bool {
        matches!(&self.base, BlockBase::Http(store) if store.has_outgoing())
    }

    // Completions belong to the base, never to a guest descriptor. Generation
    // retirement therefore also protects host reads and clean-state resets.
    /// Publishes immutable backing bytes or marks the waiting reads as failed.
    /// # Errors
    /// Rejects an unknown or retired request.
    pub fn complete(&mut self, id: u32, result: Result<Vec<u8>, ()>) -> Result<(), StorageError> {
        let BlockBase::Http(store) = &mut self.base else {
            return Err(StorageError::UnknownRequest);
        };
        match result {
            Ok(bytes) => match store.complete(id, bytes) {
                Err(StorageError::InvalidResponse) => Ok(()),
                result => result,
            },
            Err(()) => store.fail(id),
        }
    }

    pub fn clear_errors(&mut self) {
        if let BlockBase::Http(store) = &mut self.base {
            store.clear_errors();
        }
    }

    /// Restores an HTTP disk's immutable base while retaining clean cache.
    /// # Errors
    /// Rejects write-through array disks.
    pub fn discard_changes(&mut self) -> Result<(), StorageError> {
        let BlockBase::Http(store) = &mut self.base else {
            return Err(StorageError::OutOfRange);
        };
        store.discard_changes();
        Ok(())
    }
}

impl BlockBackend for BlockStore {
    fn capacity_sectors(&self) -> u64 {
        match &self.base {
            BlockBase::Array(bytes) => bytes.len() as u64 / 512,
            BlockBase::Http(store) => store.capacity_sectors(),
        }
    }

    fn reset(&mut self) {
        if let BlockBase::Http(store) = &mut self.base {
            store.reset_requests();
        }
    }

    fn read(&mut self, sector: u64, bytes: &mut [u8]) -> Result<(), DeviceError> {
        self.read_sectors(sector, bytes)
            .map_err(|_| DeviceError::Backend)
    }

    fn write(&mut self, sector: u64, bytes: &[u8]) -> Result<(), DeviceError> {
        self.write_sectors(sector, bytes)
            .map_err(|_| DeviceError::Backend)
    }

    fn read_request(
        &mut self,
        sector: u64,
        bytes: &mut [u8],
    ) -> Result<BlockRequestStatus, DeviceError> {
        match self.read_sectors(sector, bytes) {
            Ok(()) => Ok(BlockRequestStatus::Complete),
            Err(StorageError::MissingBlock(_)) => Ok(BlockRequestStatus::Pending),
            Err(_) => {
                self.clear_errors();
                Err(DeviceError::Backend)
            }
        }
    }
}
