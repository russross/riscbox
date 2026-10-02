//! Copied host disk operations, restricted to the powered-off VM platform.

use std::collections::BTreeMap;

use super::{STATE, completion_bytes};
use crate::block_storage::{BlockDiskId, BlockStore};
use crate::browser_runtime::BrowserRuntime;
use crate::browser_storage::StorageError;
use crate::virtio_devices::BlockBackend;

struct Read {
    disk: BlockDiskId,
    sector: u64,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub(super) struct BlockAbi {
    next_read: u32,
    pending: BTreeMap<u32, Read>,
    output: Vec<u8>,
}

impl BlockAbi {
    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(super) fn retire(&mut self, disk: Option<BlockDiskId>) {
        self.pending
            .retain(|_, read| disk.is_some_and(|disk| disk != read.disk));
        self.output.clear();
    }

    // Every retry owns its destination. Neither guest memory nor a JavaScript
    // view is retained while the browser services immutable chunk requests.
    fn attempt(runtime: &mut BrowserRuntime, read: &mut Read) -> i32 {
        match runtime.with_disk(read.disk, |store| {
            store.read_sectors(read.sector, &mut read.bytes)
        }) {
            Ok(Ok(())) => 0,
            Ok(Err(StorageError::MissingBlock(_))) => 1,
            Ok(Err(StorageError::OutOfRange)) => -22,
            Ok(Err(_)) => -5,
            Err(_) => -16,
        }
    }

    fn read(
        &mut self,
        runtime: &mut BrowserRuntime,
        disk: BlockDiskId,
        sector: u64,
        length: u32,
    ) -> i32 {
        self.output.clear();
        let Ok(capacity) = runtime.with_disk(disk, |store| store.capacity_sectors()) else {
            return -16;
        };
        if length == 0
            || !length.is_multiple_of(512)
            || sector
                .checked_add(u64::from(length / 512))
                .is_none_or(|end| end > capacity)
        {
            return -22;
        }
        if !self.pending.values().any(|read| read.disk == disk)
            && runtime.with_disk(disk, BlockStore::clear_errors).is_err()
        {
            return -16;
        }
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(length as usize).is_err() {
            return -28;
        }
        bytes.resize(length as usize, 0);
        let mut read = Read {
            disk,
            sector,
            bytes,
        };
        let status = Self::attempt(runtime, &mut read);
        if status == 0 {
            self.output = read.bytes;
            return 0;
        }
        if status < 0 {
            return status;
        }
        let Some(id) = self
            .next_read
            .checked_add(1)
            .filter(|id| i32::try_from(*id).is_ok())
        else {
            return -28;
        };
        self.next_read = id;
        self.pending.insert(id, read);
        i32::try_from(id).expect("request IDs fit i32")
    }

    fn finish(&mut self, runtime: &mut BrowserRuntime, id: u32) -> i32 {
        self.output.clear();
        let Some(read) = self.pending.get_mut(&id) else {
            return -116;
        };
        let status = Self::attempt(runtime, read);
        if status != 1 {
            let read = self
                .pending
                .remove(&id)
                .expect("read exists until completion");
            if status == 0 {
                self.output = read.bytes;
            }
        }
        status
    }
}

// Scalar addresses retain all 64 sector bits. Inputs name adapter allocations
// and are copied before dispatch; output snapshots last until the next call.
#[must_use]
pub extern "C" fn riscbox_disk_read(disk: u32, low: u32, high: u32, length: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        state.blocks.read(
            &mut state.runtime,
            BlockDiskId(disk),
            u64::from(low) | (u64::from(high) << 32),
            length,
        )
    })
}

#[must_use]
pub extern "C" fn riscbox_disk_finish(id: u32) -> i32 {
    STATE.with_borrow_mut(|state| state.blocks.finish(&mut state.runtime, id))
}

#[must_use]
pub extern "C" fn riscbox_disk_write(
    disk: u32,
    low: u32,
    high: u32,
    address: u32,
    length: u32,
) -> i32 {
    STATE.with_borrow_mut(|state| {
        let Some(bytes) = completion_bytes(state, address, length) else {
            return -22;
        };
        match state.runtime.with_disk(BlockDiskId(disk), |store| {
            store.write_sectors(u64::from(low) | (u64::from(high) << 32), &bytes)
        }) {
            Ok(Ok(())) => 0,
            Ok(Err(_)) => -22,
            Err(_) => -16,
        }
    })
}

#[must_use]
pub extern "C" fn riscbox_disk_discard(disk: u32) -> i32 {
    STATE.with_borrow_mut(|state| {
        if state
            .runtime
            .discard_disk_changes(BlockDiskId(disk))
            .is_err()
        {
            return -22;
        }
        state.blocks.retire(Some(BlockDiskId(disk)));
        0
    })
}

#[must_use]
pub extern "C" fn riscbox_disk_capacity(disk: u32, high: u32) -> u32 {
    STATE.with_borrow_mut(|state| {
        let capacity = state
            .runtime
            .with_disk(BlockDiskId(disk), |store| store.capacity_sectors())
            .unwrap_or(0);
        let [low0, low1, low2, low3, high0, high1, high2, high3] = capacity.to_le_bytes();
        u32::from_le_bytes(if high == 0 {
            [low0, low1, low2, low3]
        } else {
            [high0, high1, high2, high3]
        })
    })
}

#[must_use]
pub extern "C" fn riscbox_disk_data_address() -> u32 {
    STATE.with_borrow(|state| u32::try_from(state.blocks.output.as_ptr() as usize).unwrap_or(0))
}

#[must_use]
pub extern "C" fn riscbox_disk_data_length() -> u32 {
    STATE.with_borrow(|state| u32::try_from(state.blocks.output.len()).unwrap_or(0))
}
