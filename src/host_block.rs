//! Asynchronous sector requests for a host-owned block provider.

use crate::virtio_devices::{BlockBackend, BlockRequestStatus, DeviceError};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct HostBlockProviderId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostBlockGeneration(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostBlockRequestId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostBlockOutcome {
    Success(Vec<u8>),
    IoError,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostBlockKind {
    Read,
    Write,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostBlockRequest {
    pub provider: HostBlockProviderId,
    pub generation: HostBlockGeneration,
    pub id: HostBlockRequestId,
    pub kind: HostBlockKind,
    pub sector: u64,
    pub length: u32,
    pub data: Vec<u8>,
}

struct Pending {
    request: HostBlockRequest,
    result: Option<Result<Vec<u8>, ()>>,
    issued: bool,
}

// One guest request is outstanding per VirtIO block queue. The request and
// reply remain in Rust until the host result can complete the descriptor.
pub struct HostBlockStore {
    provider: HostBlockProviderId,
    capacity_sectors: u64,
    generation: HostBlockGeneration,
    next_id: HostBlockRequestId,
    pending: Option<Pending>,
}

impl HostBlockStore {
    #[must_use]
    pub const fn new(provider: HostBlockProviderId, capacity_sectors: u64) -> Self {
        Self {
            provider,
            capacity_sectors,
            generation: HostBlockGeneration(1),
            next_id: HostBlockRequestId(1),
            pending: None,
        }
    }

    #[must_use]
    pub const fn provider(&self) -> HostBlockProviderId {
        self.provider
    }

    #[must_use]
    pub const fn generation(&self) -> HostBlockGeneration {
        self.generation
    }

    pub fn next_request(&mut self) -> Option<HostBlockRequest> {
        // Mark the request before exposing it so a second host-service pass
        // cannot dispatch the same provider operation twice.
        let pending = self.pending.as_mut()?;
        if pending.issued {
            return None;
        }
        pending.issued = true;
        Some(pending.request.clone())
    }

    #[must_use]
    pub fn has_outgoing(&self) -> bool {
        self.pending.as_ref().is_some_and(|pending| !pending.issued)
    }

    /// Records a host result; malformed reads become guest I/O errors.
    ///
    /// # Errors
    /// Returns an error for an unknown current request.
    pub fn complete(
        &mut self,
        generation: HostBlockGeneration,
        id: HostBlockRequestId,
        result: HostBlockOutcome,
    ) -> Result<bool, DeviceError> {
        // A reset changes the generation before old promises can settle.
        if generation != self.generation {
            return Ok(false);
        }
        let pending = self.pending.as_mut().ok_or(DeviceError::InvalidRequest)?;
        if pending.request.id != id || pending.result.is_some() {
            return Err(DeviceError::InvalidRequest);
        }
        pending.result = Some(match result {
            HostBlockOutcome::Success(data)
                if pending.request.kind == HostBlockKind::Read
                    && data.len() == pending.request.length as usize =>
            {
                Ok(data)
            }
            HostBlockOutcome::Success(data)
                if pending.request.kind == HostBlockKind::Write && data.is_empty() =>
            {
                Ok(data)
            }
            _ => Err(()),
        });
        Ok(true)
    }

    fn request(
        &mut self,
        kind: HostBlockKind,
        sector: u64,
        data: &[u8],
    ) -> Result<Option<Vec<u8>>, DeviceError> {
        // The VirtIO device calls this again after completion. Consume the
        // saved result rather than issuing a second host request.
        if let Some(pending) = self.pending.as_mut() {
            if pending.request.kind != kind
                || pending.request.sector != sector
                || pending.request.length as usize != data.len()
            {
                return Err(DeviceError::InvalidRequest);
            }
            if let Some(result) = pending.result.take() {
                self.pending = None;
                return result.map(Some).map_err(|()| DeviceError::Backend);
            }
            return Ok(None);
        }
        // The request owns write bytes because guest memory may change after
        // the CPU run releases its exclusive memory borrow.
        let id = self.next_id;
        self.next_id = HostBlockRequestId(self.next_id.0.wrapping_add(1).max(1));
        self.pending = Some(Pending {
            request: HostBlockRequest {
                provider: self.provider,
                generation: self.generation,
                id,
                kind,
                sector,
                length: u32::try_from(data.len()).map_err(|_| DeviceError::InvalidRequest)?,
                data: if kind == HostBlockKind::Write {
                    data.to_vec()
                } else {
                    Vec::new()
                },
            },
            result: None,
            issued: false,
        });
        Ok(None)
    }
}

impl BlockBackend for HostBlockStore {
    fn capacity_sectors(&self) -> u64 {
        self.capacity_sectors
    }

    fn reset(&mut self) {
        // Stored bytes belong to the host provider and survive this reset.
        self.generation = HostBlockGeneration(self.generation.0.wrapping_add(1).max(1));
        self.pending = None;
    }

    fn read(&mut self, _sector: u64, _data: &mut [u8]) -> Result<(), DeviceError> {
        Err(DeviceError::Backend)
    }
    fn write(&mut self, _sector: u64, _data: &[u8]) -> Result<(), DeviceError> {
        Err(DeviceError::Backend)
    }

    fn read_request(
        &mut self,
        sector: u64,
        data: &mut [u8],
    ) -> Result<BlockRequestStatus, DeviceError> {
        match self.request(HostBlockKind::Read, sector, data)? {
            Some(bytes) => {
                data.copy_from_slice(&bytes);
                Ok(BlockRequestStatus::Complete)
            }
            None => Ok(BlockRequestStatus::Pending),
        }
    }

    fn write_request(
        &mut self,
        sector: u64,
        data: &[u8],
    ) -> Result<BlockRequestStatus, DeviceError> {
        match self.request(HostBlockKind::Write, sector, data)? {
            Some(_) => Ok(BlockRequestStatus::Complete),
            None => Ok(BlockRequestStatus::Pending),
        }
    }
}
