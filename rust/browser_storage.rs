//! Browser-owned HTTP requests and cacheable storage formats.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use crate::config::{Value, parse_value};
use crate::crypto::{CryptoError, decrypt_legacy_file};
use crate::virtio_devices::{BlockBackend, BlockRequestStatus, DeviceError};

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
    Crypto(CryptoError),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "browser storage error: {self:?}")
    }
}

impl std::error::Error for StorageError {}

impl From<CryptoError> for StorageError {
    fn from(error: CryptoError) -> Self {
        Self::Crypto(error)
    }
}

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

    /// Completes a pending request and returns its URL.
    ///
    /// # Errors
    ///
    /// Returns an error when `id` is not pending.
    pub fn complete(&mut self, id: u32) -> Result<String, StorageError> {
        self.pending.remove(&id).ok_or(StorageError::UnknownRequest)
    }
}

pub struct HttpBlockStore {
    base_url: String,
    block_size: usize,
    block_count: u32,
    cache_limit: usize,
    clock: u64,
    cache: BTreeMap<u32, (u64, Vec<u8>)>,
    overlays: BTreeMap<u64, Box<[u8; CLUSTER_SIZE]>>,
    pending: BTreeMap<u32, u32>,
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
                store.request_block(index)?;
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

    /// Accepts one block response and makes it available to subsequent reads.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown request or a response of the wrong size.
    pub fn complete(&mut self, id: u32, data: Vec<u8>) -> Result<(), StorageError> {
        self.queue.complete(id)?;
        let block = self
            .pending
            .remove(&id)
            .ok_or(StorageError::UnknownRequest)?;
        if data.len() != self.block_size {
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
        let start = usize::try_from(sector).map_err(|_| StorageError::OutOfRange)? * SECTOR_SIZE;
        for (index, chunk) in data.chunks_mut(SECTOR_SIZE).enumerate() {
            let byte = start + index * SECTOR_SIZE;
            let cluster =
                u64::try_from(byte / CLUSTER_SIZE).map_err(|_| StorageError::OutOfRange)?;
            if let Some(overlay) = self.overlays.get(&cluster) {
                let offset = byte % CLUSTER_SIZE;
                chunk.copy_from_slice(&overlay[offset..offset + SECTOR_SIZE]);
                continue;
            }
            let block =
                u32::try_from(byte / self.block_size).map_err(|_| StorageError::OutOfRange)?;
            if !self.cache.contains_key(&block) {
                self.request_block(block)?;
                return Err(StorageError::MissingBlock(block));
            }
            self.clock = self.clock.wrapping_add(1);
            let Some(entry) = self.cache.get_mut(&block) else {
                return Err(StorageError::MissingBlock(block));
            };
            entry.0 = self.clock;
            let offset = byte % self.block_size;
            chunk.copy_from_slice(&entry.1[offset..offset + SECTOR_SIZE]);
        }
        Ok(())
    }

    /// Writes sectors into 4 KiB copy-on-write clusters.
    ///
    /// # Errors
    ///
    /// Returns `MissingBlock` when the original cluster must first be fetched.
    pub fn write_sectors(&mut self, sector: u64, data: &[u8]) -> Result<(), StorageError> {
        self.validate_range(sector, data.len())?;
        let start = usize::try_from(sector).map_err(|_| StorageError::OutOfRange)? * SECTOR_SIZE;
        for (index, chunk) in data.chunks(SECTOR_SIZE).enumerate() {
            let byte = start + index * SECTOR_SIZE;
            let cluster_index =
                u64::try_from(byte / CLUSTER_SIZE).map_err(|_| StorageError::OutOfRange)?;
            if !self.overlays.contains_key(&cluster_index) {
                let cluster_start_sector = cluster_index * 8;
                let mut original = Box::new([0; CLUSTER_SIZE]);
                self.read_sectors(cluster_start_sector, original.as_mut_slice())?;
                self.overlays.insert(cluster_index, original);
            }
            let offset = byte % CLUSTER_SIZE;
            let Some(overlay) = self.overlays.get_mut(&cluster_index) else {
                return Err(StorageError::OutOfRange);
            };
            overlay[offset..offset + SECTOR_SIZE].copy_from_slice(chunk);
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

    fn request_block(&mut self, block: u32) -> Result<(), StorageError> {
        if block >= self.block_count {
            return Err(StorageError::OutOfRange);
        }
        if self.cache.contains_key(&block) || self.pending.values().any(|value| *value == block) {
            return Ok(());
        }
        let id = self
            .queue
            .get(format!("{}blk{block:09}.bin", self.base_url));
        self.pending.insert(id, block);
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
            Err(_) => Err(DeviceError::Backend),
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

pub struct HttpFile {
    pub url: String,
    key: Option<[u8; 16]>,
}

impl HttpFile {
    #[must_use]
    pub const fn new(url: String, key: Option<[u8; 16]>) -> Self {
        Self { url, key }
    }

    /// Validates and optionally decrypts a downloaded file.
    ///
    /// # Errors
    ///
    /// Returns an error when legacy decryption fails.
    pub fn decode<'a>(&self, data: &'a mut [u8]) -> Result<&'a [u8], StorageError> {
        match &self.key {
            Some(key) => Ok(decrypt_legacy_file(key, data)?),
            None => Ok(data),
        }
    }
}

fn positive_integer(value: Option<&Value>, name: &'static str) -> Result<i32, StorageError> {
    value
        .and_then(Value::as_integer)
        .filter(|value| *value > 0)
        .ok_or(StorageError::InvalidManifest(name))
}
