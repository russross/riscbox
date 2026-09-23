//! Shared modern `VirtIO` MMIO transport and split-ring queue handling.

use core::fmt;

use crate::guest_memory::{AccessWidth, GuestAddress, MemoryAccess, MemoryError};

pub const MMIO_SIZE: u64 = 0x1000;
pub const MAX_QUEUES: usize = 8;
pub const VERSION_1: u64 = 1 << 32;

const MAGIC_VALUE: u32 = 0x7472_6976;
const VENDOR_ID: u32 = 0x554d_4551;
const DESC_F_NEXT: u16 = 1;
const DESC_F_WRITE: u16 = 2;
const DESC_F_INDIRECT: u16 = 4;
const STATUS_FEATURES_OK: u32 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueIndex(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorIndex(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueError {
    NotReady,
    InvalidQueue,
    InvalidSize,
    InvalidDescriptor,
    DescriptorLoop,
    UnsupportedIndirect,
    DirectionChange,
    TooShort,
    Overflow,
    Misaligned,
    Memory,
}

impl fmt::Display for QueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid VirtIO queue: {self:?}")
    }
}

impl std::error::Error for QueueError {}

impl From<MemoryError> for QueueError {
    fn from(_: MemoryError) -> Self {
        Self::Memory
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VirtQueue {
    pub max_size: u16,
    pub size: u16,
    pub ready: bool,
    pub last_avail: u16,
    pub desc: u64,
    pub avail: u64,
    pub used: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Segment {
    address: u64,
    len: u32,
    writable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorChain {
    pub head: DescriptorIndex,
    pub readable: u32,
    pub writable: u32,
    segments: Vec<Segment>,
}

#[derive(Clone, Debug)]
pub struct VirtioTransport {
    device_id: u32,
    device_features: u64,
    driver_features: u64,
    device_features_sel: u32,
    driver_features_sel: u32,
    queue_sel: u16,
    queues: [VirtQueue; MAX_QUEUES],
    status: u32,
    interrupt_status: u32,
    config_generation: u32,
}

impl VirtioTransport {
    #[must_use]
    pub fn new(device_id: u32, device_features: u64, queue_maxima: &[u16]) -> Self {
        let mut queues = [VirtQueue::default(); MAX_QUEUES];
        for (queue, maximum) in queues.iter_mut().zip(queue_maxima.iter().copied()) {
            queue.max_size = maximum;
            queue.size = maximum;
        }
        let mut transport = Self {
            device_id,
            device_features: device_features | VERSION_1,
            driver_features: 0,
            device_features_sel: 0,
            driver_features_sel: 0,
            queue_sel: 0,
            queues,
            status: 0,
            interrupt_status: 0,
            config_generation: 0,
        };
        transport.reset();
        transport
    }

    pub fn reset(&mut self) {
        self.driver_features = 0;
        self.device_features_sel = 0;
        self.driver_features_sel = 0;
        self.queue_sel = 0;
        self.status = 0;
        self.interrupt_status = 0;
        for queue in &mut self.queues {
            queue.size = queue.max_size;
            queue.ready = false;
            queue.last_avail = 0;
            queue.desc = 0;
            queue.avail = 0;
            queue.used = 0;
        }
    }

    #[must_use]
    pub fn irq(&self) -> bool {
        self.interrupt_status != 0
    }

    #[must_use]
    pub fn queue(&self, index: QueueIndex) -> Option<&VirtQueue> {
        self.queues.get(usize::from(index.0))
    }

    #[must_use]
    pub const fn driver_features(&self) -> u64 {
        self.driver_features
    }

    #[must_use]
    pub const fn status(&self) -> u32 {
        self.status
    }

    #[must_use]
    pub fn read_mmio(&self, offset: u32, width: AccessWidth) -> u32 {
        if width != AccessWidth::Word {
            return 0;
        }
        let queue = &self.queues[usize::from(self.queue_sel)];
        match offset {
            0x000 => MAGIC_VALUE,
            0x004 => 2,
            0x008 => self.device_id,
            0x00c => VENDOR_ID,
            0x010 => select_u64(self.device_features, self.device_features_sel),
            0x014 => self.device_features_sel,
            0x020 => select_u64(self.driver_features, self.driver_features_sel),
            0x024 => self.driver_features_sel,
            0x030 => u32::from(self.queue_sel),
            0x034 => u32::from(queue.max_size),
            0x038 => u32::from(queue.size),
            0x044 => u32::from(queue.ready),
            0x060 => self.interrupt_status,
            0x070 => self.status,
            0x080 => low_u32(queue.desc),
            0x084 => (queue.desc >> 32) as u32,
            0x090 => low_u32(queue.avail),
            0x094 => (queue.avail >> 32) as u32,
            0x0a0 => low_u32(queue.used),
            0x0a4 => (queue.used >> 32) as u32,
            0x0fc => self.config_generation,
            _ => 0,
        }
    }

    pub fn write_mmio(&mut self, offset: u32, value: u32, width: AccessWidth) {
        if width != AccessWidth::Word {
            return;
        }
        match offset {
            0x014 => self.device_features_sel = value,
            0x020 => set_selected_u64(&mut self.driver_features, self.driver_features_sel, value),
            0x024 => self.driver_features_sel = value,
            0x030 => {
                if let Ok(selector) = u16::try_from(value)
                    && usize::from(selector) < MAX_QUEUES
                {
                    self.queue_sel = selector;
                }
            }
            0x038 => {
                let queue = &mut self.queues[usize::from(self.queue_sel)];
                if value.is_power_of_two()
                    && value <= u32::from(queue.max_size)
                    && let Ok(size) = u16::try_from(value)
                {
                    queue.size = size;
                }
            }
            0x044 => self.queues[usize::from(self.queue_sel)].ready = value & 1 != 0,
            0x064 => self.interrupt_status &= !value,
            0x070 => {
                if value == 0 {
                    self.reset();
                } else {
                    self.status = value;
                    if value & STATUS_FEATURES_OK != 0
                        && self.driver_features & !self.device_features != 0
                    {
                        self.status &= !STATUS_FEATURES_OK;
                    }
                }
            }
            0x080 => set_low(&mut self.queues[usize::from(self.queue_sel)].desc, value),
            0x084 => set_high(&mut self.queues[usize::from(self.queue_sel)].desc, value),
            0x090 => set_low(&mut self.queues[usize::from(self.queue_sel)].avail, value),
            0x094 => set_high(&mut self.queues[usize::from(self.queue_sel)].avail, value),
            0x0a0 => set_low(&mut self.queues[usize::from(self.queue_sel)].used, value),
            0x0a4 => set_high(&mut self.queues[usize::from(self.queue_sel)].used, value),
            _ => {}
        }
    }

    /// Returns and advances past the next available descriptor chain.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid queue or malformed guest descriptor chain.
    pub fn next_chain(
        &mut self,
        memory: &mut dyn MemoryAccess,
        index: QueueIndex,
    ) -> Result<Option<DescriptorChain>, QueueError> {
        let queue = self.queue_checked(index)?;
        if !queue.ready {
            return Err(QueueError::NotReady);
        }
        validate_queue(queue)?;
        let avail_index = read_u16(memory, checked_add(queue.avail, 2)?)?;
        if queue.last_avail == avail_index {
            return Ok(None);
        }
        let slot = queue.last_avail & (queue.size - 1);
        let head_address = checked_add(queue.avail, 4 + u64::from(slot) * 2)?;
        let head = read_u16(memory, head_address)?;
        if head >= queue.size {
            return Err(QueueError::InvalidDescriptor);
        }
        let chain = read_chain(memory, queue, DescriptorIndex(head))?;
        self.queue_checked_mut(index)?.last_avail = queue.last_avail.wrapping_add(1);
        Ok(Some(chain))
    }

    /// Copies bytes from the readable portion of a descriptor chain.
    ///
    /// # Errors
    ///
    /// Returns an error when the range exceeds the chain or guest RAM is invalid.
    pub fn read_chain(
        &self,
        memory: &mut dyn MemoryAccess,
        chain: &DescriptorChain,
        offset: u32,
        destination: &mut [u8],
    ) -> Result<(), QueueError> {
        copy_chain(memory, chain, false, offset, destination)
    }

    /// Copies bytes into the writable portion of a descriptor chain.
    ///
    /// # Errors
    ///
    /// Returns an error when the range exceeds the chain or guest RAM is invalid.
    pub fn write_chain(
        &self,
        memory: &mut dyn MemoryAccess,
        chain: &DescriptorChain,
        offset: u32,
        source: &[u8],
    ) -> Result<(), QueueError> {
        write_chain_bytes(memory, chain, offset, source)
    }

    /// Publishes a used-ring element and raises the queue interrupt.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid queue, length, or used-ring address.
    pub fn complete_chain(
        &mut self,
        memory: &mut dyn MemoryAccess,
        index: QueueIndex,
        chain: &DescriptorChain,
        written: u32,
    ) -> Result<(), QueueError> {
        let queue = self.queue_checked(index)?;
        validate_queue(queue)?;
        if written > chain.writable {
            return Err(QueueError::TooShort);
        }
        let used_index = read_u16(memory, checked_add(queue.used, 2)?)?;
        let slot = used_index & (queue.size - 1);
        let element = checked_add(queue.used, 4 + u64::from(slot) * 8)?;
        write_u32(memory, element, u32::from(chain.head.0))?;
        write_u32(memory, checked_add(element, 4)?, written)?;
        write_u16(
            memory,
            checked_add(queue.used, 2)?,
            used_index.wrapping_add(1),
        )?;
        self.interrupt_status |= 1;
        Ok(())
    }

    pub fn raise_config_interrupt(&mut self) {
        self.config_generation = self.config_generation.wrapping_add(1);
        self.interrupt_status |= 2;
    }

    fn queue_checked(&self, index: QueueIndex) -> Result<&VirtQueue, QueueError> {
        self.queues
            .get(usize::from(index.0))
            .ok_or(QueueError::InvalidQueue)
    }

    fn queue_checked_mut(&mut self, index: QueueIndex) -> Result<&mut VirtQueue, QueueError> {
        self.queues
            .get_mut(usize::from(index.0))
            .ok_or(QueueError::InvalidQueue)
    }
}

fn read_chain(
    memory: &mut dyn MemoryAccess,
    queue: &VirtQueue,
    head: DescriptorIndex,
) -> Result<DescriptorChain, QueueError> {
    let mut segments = Vec::new();
    let mut readable = 0_u32;
    let mut writable = 0_u32;
    let mut writing = false;
    let mut index = head.0;
    for _ in 0..queue.size {
        if index >= queue.size {
            return Err(QueueError::InvalidDescriptor);
        }
        let address = checked_add(queue.desc, u64::from(index) * 16)?;
        let data_address = read_u64(memory, address)?;
        let len = read_u32(memory, checked_add(address, 8)?)?;
        let flags = read_u16(memory, checked_add(address, 12)?)?;
        let next = read_u16(memory, checked_add(address, 14)?)?;
        if flags & DESC_F_INDIRECT != 0 {
            return Err(QueueError::UnsupportedIndirect);
        }
        let is_writable = flags & DESC_F_WRITE != 0;
        if !is_writable && writing {
            return Err(QueueError::DirectionChange);
        }
        writing |= is_writable;
        let total = if is_writable {
            &mut writable
        } else {
            &mut readable
        };
        *total = total.checked_add(len).ok_or(QueueError::Overflow)?;
        if len != 0 {
            checked_add(data_address, u64::from(len))?;
            let length = usize::try_from(len).map_err(|_| QueueError::Overflow)?;
            memory.validate_ram(GuestAddress(data_address), length, is_writable)?;
        }
        segments.push(Segment {
            address: data_address,
            len,
            writable: is_writable,
        });
        if flags & DESC_F_NEXT == 0 {
            return Ok(DescriptorChain {
                head,
                readable,
                writable,
                segments,
            });
        }
        index = next;
    }
    Err(QueueError::DescriptorLoop)
}

fn copy_chain(
    memory: &mut dyn MemoryAccess,
    chain: &DescriptorChain,
    writable: bool,
    offset: u32,
    bytes: &mut [u8],
) -> Result<(), QueueError> {
    let capacity = if writable {
        chain.writable
    } else {
        chain.readable
    };
    let count = u32::try_from(bytes.len()).map_err(|_| QueueError::Overflow)?;
    if offset.checked_add(count).ok_or(QueueError::Overflow)? > capacity {
        return Err(QueueError::TooShort);
    }
    let mut skip = offset;
    let mut copied = 0_usize;
    for segment in chain
        .segments
        .iter()
        .filter(|segment| segment.writable == writable)
    {
        if skip >= segment.len {
            skip -= segment.len;
            continue;
        }
        let available = usize::try_from(segment.len - skip).map_err(|_| QueueError::Overflow)?;
        let length = available.min(bytes.len() - copied);
        let address = checked_add(segment.address, u64::from(skip))?;
        if writable {
            memory.write_bytes(GuestAddress(address), &bytes[copied..copied + length])?;
        } else {
            memory.read_bytes(GuestAddress(address), &mut bytes[copied..copied + length])?;
        }
        copied += length;
        skip = 0;
        if copied == bytes.len() {
            return Ok(());
        }
    }
    if copied == bytes.len() {
        Ok(())
    } else {
        Err(QueueError::TooShort)
    }
}

fn validate_queue(queue: &VirtQueue) -> Result<(), QueueError> {
    if queue.size == 0 || !queue.size.is_power_of_two() || queue.size > queue.max_size {
        Err(QueueError::InvalidSize)
    } else if !queue.desc.is_multiple_of(16)
        || !queue.avail.is_multiple_of(2)
        || !queue.used.is_multiple_of(4)
    {
        Err(QueueError::Misaligned)
    } else {
        Ok(())
    }
}

fn write_chain_bytes(
    memory: &mut dyn MemoryAccess,
    chain: &DescriptorChain,
    offset: u32,
    bytes: &[u8],
) -> Result<(), QueueError> {
    let count = u32::try_from(bytes.len()).map_err(|_| QueueError::Overflow)?;
    if offset.checked_add(count).ok_or(QueueError::Overflow)? > chain.writable {
        return Err(QueueError::TooShort);
    }
    let mut skip = offset;
    let mut copied = 0_usize;
    for segment in chain.segments.iter().filter(|segment| segment.writable) {
        if skip >= segment.len {
            skip -= segment.len;
            continue;
        }
        let available = usize::try_from(segment.len - skip).map_err(|_| QueueError::Overflow)?;
        let length = available.min(bytes.len() - copied);
        let address = checked_add(segment.address, u64::from(skip))?;
        memory.write_bytes(GuestAddress(address), &bytes[copied..copied + length])?;
        copied += length;
        skip = 0;
        if copied == bytes.len() {
            return Ok(());
        }
    }
    if copied == bytes.len() {
        Ok(())
    } else {
        Err(QueueError::TooShort)
    }
}

fn read_u16(memory: &mut dyn MemoryAccess, address: u64) -> Result<u16, QueueError> {
    Ok(
        u16::try_from(memory.read(GuestAddress(address), AccessWidth::HalfWord)?)
            .expect("a half-word memory read fits in u16"),
    )
}

fn read_u32(memory: &mut dyn MemoryAccess, address: u64) -> Result<u32, QueueError> {
    Ok(
        u32::try_from(memory.read(GuestAddress(address), AccessWidth::Word)?)
            .expect("a word memory read fits in u32"),
    )
}

fn read_u64(memory: &mut dyn MemoryAccess, address: u64) -> Result<u64, QueueError> {
    Ok(memory.read(GuestAddress(address), AccessWidth::DoubleWord)?)
}

fn write_u16(memory: &mut dyn MemoryAccess, address: u64, value: u16) -> Result<(), QueueError> {
    memory.write(
        GuestAddress(address),
        AccessWidth::HalfWord,
        u64::from(value),
    )?;
    Ok(())
}

fn write_u32(memory: &mut dyn MemoryAccess, address: u64, value: u32) -> Result<(), QueueError> {
    memory.write(GuestAddress(address), AccessWidth::Word, u64::from(value))?;
    Ok(())
}

fn checked_add(base: u64, offset: u64) -> Result<u64, QueueError> {
    base.checked_add(offset).ok_or(QueueError::Overflow)
}

const fn select_u64(value: u64, selector: u32) -> u32 {
    match selector {
        0 => low_u32(value),
        1 => (value >> 32) as u32,
        _ => 0,
    }
}

const fn low_u32(value: u64) -> u32 {
    let bytes = value.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn set_selected_u64(value: &mut u64, selector: u32, half: u32) {
    match selector {
        0 => set_low(value, half),
        1 => set_high(value, half),
        _ => {}
    }
}

fn set_low(value: &mut u64, low: u32) {
    *value = (*value & !u64::from(u32::MAX)) | u64::from(low);
}

fn set_high(value: &mut u64, high: u32) {
    *value = (*value & u64::from(u32::MAX)) | (u64::from(high) << 32);
}
