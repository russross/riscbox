//! VM-owned sector storage shared by `VirtIO` and halted host operations.

use crate::config::{Value, parse_value};
use crate::virtio_devices::{BlockBackend, BlockRequestStatus, DeviceError};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
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

const SECTOR_SIZE: usize = 512;
const CLUSTER_SIZE: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    Get,
    Post(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpRequest {
    pub id: u32,
    pub url: String,
    pub method: HttpMethod,
    pub authorization: Option<(String, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageError {
    InvalidManifest(&'static str),
    InvalidResponse,
    UnknownRequest,
    MissingBlock(u32),
    OutOfRange,
    BackingFailed,
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "browser storage error: {self:?}")
    }
}

impl std::error::Error for StorageError {}

#[derive(Default)]
pub struct HttpQueue {
    next_id: u32,
    pending: BTreeMap<u32, String>,
    outgoing: VecDeque<HttpRequest>,
}

impl HttpQueue {
    /// Queues a GET and returns its stable request identifier.
    pub fn get(&mut self, url: String) -> u32 {
        self.request(url, HttpMethod::Get, None)
    }

    /// Queues an HTTP request and returns its stable identifier.
    pub fn request(
        &mut self,
        url: String,
        method: HttpMethod,
        authorization: Option<(String, String)>,
    ) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = self.next_id;
        self.pending.insert(id, url.clone());
        self.outgoing.push_back(HttpRequest {
            id,
            url,
            method,
            authorization,
        });
        id
    }

    /// Removes the next request for delivery to the browser host.
    pub fn next_request(&mut self) -> Option<HttpRequest> {
        self.outgoing.pop_front()
    }

    #[must_use]
    pub fn has_outgoing(&self) -> bool {
        !self.outgoing.is_empty()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.outgoing.clear();
    }

    /// Completes a pending request and returns its URL.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` is not pending.
    pub fn complete(&mut self, id: u32) -> Result<String, StorageError> {
        self.pending.remove(&id).ok_or(StorageError::UnknownRequest)
    }
}

// A dirty bit identifies each whole sector whose bytes override the base.
// Unwritten sectors never need to be fetched merely to allocate an overlay.
struct Overlay {
    bytes: Box<[u8; CLUSTER_SIZE]>,
    dirty: u8,
}

// Prefetch is speculative until an actual reader joins it. Only demand failures
// become I/O errors; a failed unused prefetch can be fetched again on demand.
struct PendingChunk {
    block: u32,
    demanded: bool,
}

pub struct HttpBlockStore {
    base_url: String,
    block_size: usize,
    block_count: u32,
    cache_limit: usize,
    clock: u64,
    cache: BTreeMap<u32, (u64, Vec<u8>)>,
    overlays: BTreeMap<u64, Overlay>,
    failed: BTreeSet<u32>,
    pending: BTreeMap<u32, PendingChunk>,
    queue: HttpQueue,
}

impl HttpBlockStore {
    /// Parses the split-image descriptor used by `splitimg`.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed descriptors or unsafe dimensions.
    pub fn from_manifest(
        manifest_url: &str,
        source: &str,
        cache_limit: usize,
    ) -> Result<Self, StorageError> {
        let value = parse_value(source).map_err(|_| StorageError::InvalidManifest("syntax"))?;
        let object = value
            .as_object()
            .ok_or(StorageError::InvalidManifest("object expected"))?;
        let block_kib = positive_integer(object.get("block_size"), "block_size")?;
        let block_size = usize::try_from(block_kib)
            .ok()
            .and_then(|value| value.checked_mul(1024))
            .filter(|value| value.is_power_of_two() && value.is_multiple_of(SECTOR_SIZE))
            .ok_or(StorageError::InvalidManifest("invalid block_size"))?;
        let block_count = u32::try_from(positive_integer(object.get("n_block"), "n_block")?)
            .map_err(|_| StorageError::InvalidManifest("invalid n_block"))?;
        let base_url = manifest_url
            .rfind('/')
            .map_or_else(String::new, |index| manifest_url[..=index].to_owned());
        let mut store = Self {
            base_url,
            block_size,
            block_count,
            cache_limit: cache_limit.max(block_size),
            clock: 0,
            cache: BTreeMap::new(),
            overlays: BTreeMap::new(),
            failed: BTreeSet::new(),
            pending: BTreeMap::new(),
            queue: HttpQueue::default(),
        };
        if let Some(prefetch) = object.get("prefetch") {
            for block in prefetch
                .as_array()
                .ok_or(StorageError::InvalidManifest("prefetch array expected"))?
            {
                let index = u32::try_from(
                    block
                        .as_integer()
                        .ok_or(StorageError::InvalidManifest("prefetch integer expected"))?,
                )
                .map_err(|_| StorageError::InvalidManifest("invalid prefetch block"))?;
                store.request_block(index, false)?;
            }
        }
        Ok(store)
    }

    #[must_use]
    pub fn capacity_sectors(&self) -> u64 {
        u64::from(self.block_count) * u64::try_from(self.block_size / SECTOR_SIZE).unwrap_or(0)
    }

    pub fn next_request(&mut self) -> Option<HttpRequest> {
        self.queue.next_request()
    }

    #[must_use]
    pub fn has_outgoing(&self) -> bool {
        self.queue.has_outgoing()
    }

    pub fn reset_requests(&mut self) {
        self.pending.clear();
        self.queue.clear();
        self.failed.clear();
    }

    pub fn discard_changes(&mut self) {
        self.reset_requests();
        self.overlays.clear();
    }

    pub fn clear_errors(&mut self) {
        self.failed.clear();
    }

    /// Records a failed fetch so its waiting readers receive an I/O error.
    /// # Errors
    /// Rejects an unknown or retired request.
    pub fn fail(&mut self, id: u32) -> Result<(), StorageError> {
        self.queue.complete(id)?;
        let pending = self
            .pending
            .remove(&id)
            .ok_or(StorageError::UnknownRequest)?;
        if pending.demanded {
            self.failed.insert(pending.block);
        }
        Ok(())
    }

    /// Accepts one block response and makes it available to subsequent reads.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown request or a response of the wrong size.
    pub fn complete(&mut self, id: u32, data: Vec<u8>) -> Result<(), StorageError> {
        self.queue.complete(id)?;
        let pending = self
            .pending
            .remove(&id)
            .ok_or(StorageError::UnknownRequest)?;
        let block = pending.block;
        if data.len() != self.block_size {
            if pending.demanded {
                self.failed.insert(block);
            }
            return Err(StorageError::InvalidResponse);
        }
        self.clock = self.clock.wrapping_add(1);
        self.cache.insert(block, (self.clock, data));
        self.trim_cache();
        Ok(())
    }

    /// Reads sectors, queuing the first missing HTTP block when necessary.
    ///
    /// # Errors
    ///
    /// Returns `MissingBlock` after queuing a fetch, or an error for an invalid range.
    pub fn read_sectors(&mut self, sector: u64, data: &mut [u8]) -> Result<(), StorageError> {
        self.validate_range(sector, data.len())?;
        let start = sector
            .checked_mul(SECTOR_SIZE as u64)
            .ok_or(StorageError::OutOfRange)?;
        self.retain_working_set(start, data.len())?;
        for (index, chunk) in data.chunks_mut(SECTOR_SIZE).enumerate() {
            let byte = start
                .checked_add(
                    u64::try_from(index * SECTOR_SIZE).map_err(|_| StorageError::OutOfRange)?,
                )
                .ok_or(StorageError::OutOfRange)?;
            let cluster = byte / CLUSTER_SIZE as u64;
            let offset = usize::try_from(byte % CLUSTER_SIZE as u64)
                .map_err(|_| StorageError::OutOfRange)?;
            if let Some(overlay) = self.overlays.get(&cluster)
                && overlay.dirty & (1 << (offset / SECTOR_SIZE)) != 0
            {
                chunk.copy_from_slice(&overlay.bytes[offset..offset + SECTOR_SIZE]);
                continue;
            }
            let block = u32::try_from(byte / self.block_size as u64)
                .map_err(|_| StorageError::OutOfRange)?;
            if self.failed.contains(&block) {
                return Err(StorageError::BackingFailed);
            }
            if !self.cache.contains_key(&block) {
                self.request_block(block, true)?;
                return Err(StorageError::MissingBlock(block));
            }
            self.clock = self.clock.wrapping_add(1);
            let Some(entry) = self.cache.get_mut(&block) else {
                return Err(StorageError::MissingBlock(block));
            };
            entry.0 = self.clock;
            let offset = usize::try_from(byte % self.block_size as u64)
                .map_err(|_| StorageError::OutOfRange)?;
            chunk.copy_from_slice(&entry.1[offset..offset + SECTOR_SIZE]);
        }
        Ok(())
    }

    /// Writes sectors synchronously into sparse copy-on-write clusters.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid range.
    pub fn write_sectors(&mut self, sector: u64, data: &[u8]) -> Result<(), StorageError> {
        self.validate_range(sector, data.len())?;
        let start = sector
            .checked_mul(SECTOR_SIZE as u64)
            .ok_or(StorageError::OutOfRange)?;
        for (index, chunk) in data.chunks(SECTOR_SIZE).enumerate() {
            let byte = start
                .checked_add(
                    u64::try_from(index * SECTOR_SIZE).map_err(|_| StorageError::OutOfRange)?,
                )
                .ok_or(StorageError::OutOfRange)?;
            let cluster_index = byte / CLUSTER_SIZE as u64;
            let overlay = self
                .overlays
                .entry(cluster_index)
                .or_insert_with(|| Overlay {
                    bytes: Box::new([0; CLUSTER_SIZE]),
                    dirty: 0,
                });
            let offset = usize::try_from(byte % CLUSTER_SIZE as u64)
                .map_err(|_| StorageError::OutOfRange)?;
            overlay.bytes[offset..offset + SECTOR_SIZE].copy_from_slice(chunk);
            overlay.dirty |= 1 << (offset / SECTOR_SIZE);
        }
        Ok(())
    }

    fn validate_range(&self, sector: u64, len: usize) -> Result<(), StorageError> {
        if len == 0 || !len.is_multiple_of(SECTOR_SIZE) {
            return Err(StorageError::OutOfRange);
        }
        let count = u64::try_from(len / SECTOR_SIZE).map_err(|_| StorageError::OutOfRange)?;
        if sector
            .checked_add(count)
            .is_none_or(|end| end > self.capacity_sectors())
        {
            Err(StorageError::OutOfRange)
        } else {
            Ok(())
        }
    }

    fn retain_working_set(&mut self, start: u64, len: usize) -> Result<(), StorageError> {
        let last = start
            .checked_add(u64::try_from(len).map_err(|_| StorageError::OutOfRange)? - 1)
            .ok_or(StorageError::OutOfRange)?;
        let first_block = start / self.block_size as u64;
        let last_block = last / self.block_size as u64;
        let block_count = last_block - first_block + 1;
        let bytes = usize::try_from(block_count)
            .ok()
            .and_then(|count| count.checked_mul(self.block_size))
            .ok_or(StorageError::OutOfRange)?;
        self.cache_limit = self.cache_limit.max(bytes);
        Ok(())
    }

    fn request_block(&mut self, block: u32, demanded: bool) -> Result<(), StorageError> {
        if block >= self.block_count {
            return Err(StorageError::OutOfRange);
        }
        if self.cache.contains_key(&block) {
            return Ok(());
        }
        if let Some(pending) = self
            .pending
            .values_mut()
            .find(|pending| pending.block == block)
        {
            pending.demanded |= demanded;
            return Ok(());
        }
        let id = self
            .queue
            .get(format!("{}blk{block:09}.bin", self.base_url));
        self.pending.insert(id, PendingChunk { block, demanded });
        Ok(())
    }

    fn trim_cache(&mut self) {
        while self.cache.len().saturating_mul(self.block_size) > self.cache_limit {
            let Some((&oldest, _)) = self.cache.iter().min_by_key(|(_, (age, _))| age) else {
                break;
            };
            self.cache.remove(&oldest);
        }
    }
}

impl BlockBackend for HttpBlockStore {
    fn capacity_sectors(&self) -> u64 {
        self.capacity_sectors()
    }

    fn reset(&mut self) {
        self.reset_requests();
    }

    fn read(&mut self, sector: u64, data: &mut [u8]) -> Result<(), DeviceError> {
        self.read_sectors(sector, data)
            .map_err(|_| DeviceError::Backend)
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), DeviceError> {
        self.write_sectors(sector, data)
            .map_err(|_| DeviceError::Backend)
    }

    fn read_request(
        &mut self,
        sector: u64,
        data: &mut [u8],
    ) -> Result<BlockRequestStatus, DeviceError> {
        match self.read_sectors(sector, data) {
            Ok(()) => Ok(BlockRequestStatus::Complete),
            Err(StorageError::MissingBlock(_)) => Ok(BlockRequestStatus::Pending),
            Err(_) => {
                self.clear_errors();
                Err(DeviceError::Backend)
            }
        }
    }

    fn write_request(
        &mut self,
        sector: u64,
        data: &[u8],
    ) -> Result<BlockRequestStatus, DeviceError> {
        match self.write_sectors(sector, data) {
            Ok(()) => Ok(BlockRequestStatus::Complete),
            Err(StorageError::MissingBlock(_)) => Ok(BlockRequestStatus::Pending),
            Err(_) => Err(DeviceError::Backend),
        }
    }
}

fn positive_integer(value: Option<&Value>, name: &'static str) -> Result<i32, StorageError> {
    value
        .and_then(Value::as_integer)
        .filter(|value| *value > 0)
        .ok_or(StorageError::InvalidManifest(name))
}
