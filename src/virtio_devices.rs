//! Synchronous `VirtIO` device protocols used by the browser platform.

use core::fmt;

use crate::entropy::SharedEntropy;
use crate::memory::{AccessWidth, PhysicalMemory};
use crate::virtio::{DescriptorChain, QueueError, QueueIndex, VirtioTransport};

const CONFIG_BASE: u32 = 0x100;
const NET_HEADER_LEN: usize = 10;
const ENTROPY_CHUNK_SIZE: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceError {
    Queue(QueueError),
    InvalidRequest,
    Backend,
}

impl fmt::Display for DeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "VirtIO device error: {self:?}")
    }
}

impl std::error::Error for DeviceError {}

impl From<QueueError> for DeviceError {
    fn from(error: QueueError) -> Self {
        Self::Queue(error)
    }
}

pub trait VirtioDevice {
    fn read_config(&self, offset: u32, width: AccessWidth) -> u32;
    fn write_config(&mut self, offset: u32, value: u32, width: AccessWidth);
    fn reset(&mut self) {}
    /// Services all available chains in the notified queue.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed queues, requests, or host failures.
    fn notify(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        queue: QueueIndex,
    ) -> Result<(), DeviceError>;
}

pub struct VirtioMmioDevice<D> {
    pub transport: VirtioTransport,
    pub device: D,
}

impl<D: VirtioDevice> VirtioMmioDevice<D> {
    pub const fn new(transport: VirtioTransport, device: D) -> Self {
        Self { transport, device }
    }

    #[must_use]
    pub fn read(&self, offset: u32, width: AccessWidth) -> u32 {
        if offset >= CONFIG_BASE {
            self.device.read_config(offset - CONFIG_BASE, width)
        } else {
            self.transport.read_mmio(offset, width)
        }
    }

    /// Writes a transport or device register and services queue notifications.
    ///
    /// # Errors
    ///
    /// Returns an error when a queue notification cannot be serviced.
    pub fn write(
        &mut self,
        memory: &mut PhysicalMemory,
        offset: u32,
        value: u32,
        width: AccessWidth,
    ) -> Result<(), DeviceError> {
        if offset >= CONFIG_BASE {
            self.device.write_config(offset - CONFIG_BASE, value, width);
        } else if offset == 0x050 && width == AccessWidth::Word {
            let queue = u16::try_from(value).map_err(|_| DeviceError::InvalidRequest)?;
            if self.transport.status() & 4 == 0 {
                return Err(DeviceError::InvalidRequest);
            }
            if !self
                .transport
                .queue(QueueIndex(queue))
                .is_some_and(|state| state.ready)
            {
                return Err(DeviceError::Queue(QueueError::NotReady));
            }
            self.device
                .notify(&mut self.transport, memory, QueueIndex(queue))?;
        } else {
            if offset == 0x070 && width == AccessWidth::Word && value == 0 {
                self.device.reset();
            }
            self.transport.write_mmio(offset, value, width);
        }
        Ok(())
    }
}

pub trait BlockBackend {
    fn capacity_sectors(&self) -> u64;
    /// Reads whole sectors into `data`.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::Backend` when host storage cannot be read.
    fn read(&mut self, sector: u64, data: &mut [u8]) -> Result<(), DeviceError>;
    /// Writes whole sectors from `data`.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::Backend` when host storage cannot be written.
    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), DeviceError>;

    /// Starts a read which may remain pending on a host service.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::Backend` when host storage cannot be read.
    fn read_request(
        &mut self,
        sector: u64,
        data: &mut [u8],
    ) -> Result<BlockRequestStatus, DeviceError> {
        self.read(sector, data)?;
        Ok(BlockRequestStatus::Complete)
    }

    /// Starts a write which may remain pending on a host service.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::Backend` when host storage cannot be written.
    fn write_request(
        &mut self,
        sector: u64,
        data: &[u8],
    ) -> Result<BlockRequestStatus, DeviceError> {
        self.write(sector, data)?;
        Ok(BlockRequestStatus::Complete)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockRequestStatus {
    Complete,
    Pending,
}

impl<T: BlockBackend + ?Sized> BlockBackend for Box<T> {
    fn capacity_sectors(&self) -> u64 {
        (**self).capacity_sectors()
    }
    fn read(&mut self, sector: u64, data: &mut [u8]) -> Result<(), DeviceError> {
        (**self).read(sector, data)
    }
    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), DeviceError> {
        (**self).write(sector, data)
    }

    fn read_request(
        &mut self,
        sector: u64,
        data: &mut [u8],
    ) -> Result<BlockRequestStatus, DeviceError> {
        (**self).read_request(sector, data)
    }
    fn write_request(
        &mut self,
        sector: u64,
        data: &[u8],
    ) -> Result<BlockRequestStatus, DeviceError> {
        (**self).write_request(sector, data)
    }
}

pub struct EntropyDevice {
    source: SharedEntropy,
}

impl EntropyDevice {
    #[must_use]
    pub const fn new(source: SharedEntropy) -> Self {
        Self { source }
    }
}

impl VirtioDevice for EntropyDevice {
    fn read_config(&self, _: u32, _: AccessWidth) -> u32 {
        0
    }

    fn write_config(&mut self, _: u32, _: u32, _: AccessWidth) {}

    fn notify(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        queue: QueueIndex,
    ) -> Result<(), DeviceError> {
        if queue != QueueIndex(0) {
            return Err(DeviceError::InvalidRequest);
        }
        while let Some(chain) = transport.next_chain(memory, queue)? {
            if chain.readable != 0 {
                return Err(DeviceError::InvalidRequest);
            }
            let mut offset = 0;
            let mut bytes = vec![0; ENTROPY_CHUNK_SIZE.min(chain.writable as usize)];
            while offset < chain.writable {
                let remaining = usize::try_from(chain.writable - offset)
                    .map_err(|_| DeviceError::InvalidRequest)?;
                let length = remaining.min(ENTROPY_CHUNK_SIZE);
                self.source
                    .borrow_mut()
                    .fill(&mut bytes[..length])
                    .map_err(|_| DeviceError::Backend)?;
                transport.write_chain(memory, &chain, offset, &bytes[..length])?;
                offset += u32::try_from(length).expect("entropy chunk length fits in u32");
            }
            transport.complete_chain(memory, queue, &chain, chain.writable)?;
        }
        Ok(())
    }
}

pub struct BlockDevice<B> {
    backend: B,
    id: [u8; 20],
    pending: Option<(QueueIndex, DescriptorChain)>,
}

impl<B> BlockDevice<B> {
    pub const fn new(backend: B, id: [u8; 20]) -> Self {
        Self {
            backend,
            id,
            pending: None,
        }
    }
    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
}

impl<B: BlockBackend> VirtioDevice for BlockDevice<B> {
    fn read_config(&self, offset: u32, width: AccessWidth) -> u32 {
        config_u64(self.backend.capacity_sectors(), offset, width)
    }
    fn write_config(&mut self, _: u32, _: u32, _: AccessWidth) {}
    fn reset(&mut self) {
        self.pending = None;
    }
    fn notify(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        queue: QueueIndex,
    ) -> Result<(), DeviceError> {
        if self.pending.is_some() {
            return Ok(());
        }
        while let Some(chain) = transport.next_chain(memory, queue)? {
            if self.request(transport, memory, queue, &chain)? == BlockRequestStatus::Pending {
                self.pending = Some((queue, chain));
                break;
            }
        }
        Ok(())
    }
}

impl<B: BlockBackend> BlockDevice<B> {
    /// Retries an outstanding request after its host operation has completed.
    ///
    /// # Errors
    ///
    /// Returns an error for a malformed request or queue.
    pub fn resume(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
    ) -> Result<(), DeviceError> {
        let Some((queue, chain)) = self.pending.take() else {
            return Ok(());
        };
        if self.request(transport, memory, queue, &chain)? == BlockRequestStatus::Pending {
            self.pending = Some((queue, chain));
            return Ok(());
        }
        self.notify(transport, memory, queue)
    }

    fn request(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        queue: QueueIndex,
        chain: &DescriptorChain,
    ) -> Result<BlockRequestStatus, DeviceError> {
        if chain.readable < 16 || chain.writable < 1 {
            return Err(DeviceError::InvalidRequest);
        }
        let mut header = [0; 16];
        transport.read_chain(memory, chain, 0, &mut header)?;
        let kind = u32::from_le_bytes(header[..4].try_into().expect("four-byte slice"));
        let sector = u64::from_le_bytes(header[8..].try_into().expect("eight-byte slice"));
        let mut status = 0_u8;
        let written = match kind {
            0 => {
                let len =
                    usize::try_from(chain.writable - 1).map_err(|_| DeviceError::InvalidRequest)?;
                self.validate_range(sector, len)?;
                let mut data = vec![0; len];
                match self.backend.read_request(sector, &mut data) {
                    Ok(BlockRequestStatus::Pending) => return Ok(BlockRequestStatus::Pending),
                    Ok(BlockRequestStatus::Complete) => {}
                    Err(_) => status = 1,
                }
                transport.write_chain(memory, chain, 0, &data)?;
                u32::try_from(len).expect("descriptor length is u32") + 1
            }
            1 => {
                let len = usize::try_from(chain.readable - 16)
                    .map_err(|_| DeviceError::InvalidRequest)?;
                self.validate_range(sector, len)?;
                let mut data = vec![0; len];
                transport.read_chain(memory, chain, 16, &mut data)?;
                match self.backend.write_request(sector, &data) {
                    Ok(BlockRequestStatus::Pending) => return Ok(BlockRequestStatus::Pending),
                    Ok(BlockRequestStatus::Complete) => {}
                    Err(_) => status = 1,
                }
                1
            }
            8 if chain.writable == 21 => {
                transport.write_chain(memory, chain, 0, &self.id)?;
                21
            }
            _ => {
                status = 2;
                1
            }
        };
        transport.write_chain(memory, chain, chain.writable - 1, &[status])?;
        transport.complete_chain(memory, queue, chain, written)?;
        Ok(BlockRequestStatus::Complete)
    }

    fn validate_range(&self, sector: u64, len: usize) -> Result<(), DeviceError> {
        if len == 0 || !len.is_multiple_of(512) {
            return Err(DeviceError::InvalidRequest);
        }
        let sectors = u64::try_from(len / 512).map_err(|_| DeviceError::InvalidRequest)?;
        if sector
            .checked_add(sectors)
            .is_none_or(|end| end > self.backend.capacity_sectors())
        {
            Err(DeviceError::InvalidRequest)
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
pub struct ConsoleDevice {
    pub output: Vec<u8>,
    input: Vec<u8>,
    width: u16,
    height: u16,
}

impl ConsoleDevice {
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            ..Self::default()
        }
    }
    pub fn push_input(&mut self, bytes: &[u8]) {
        self.input.extend_from_slice(bytes);
    }
    /// Delivers bytes to an already posted receive chain.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable or malformed receive queue.
    pub fn receive(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        bytes: &[u8],
    ) -> Result<(), DeviceError> {
        if transport.status() & 4 == 0 {
            return Err(DeviceError::InvalidRequest);
        }
        self.push_input(bytes);
        self.drain_input(transport, memory)
    }
    pub fn resize(&mut self, transport: &mut VirtioTransport, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        transport.raise_config_interrupt();
    }

    fn drain_input(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
    ) -> Result<(), DeviceError> {
        while !self.input.is_empty() {
            let Some(chain) = transport.next_chain(memory, QueueIndex(0))? else {
                break;
            };
            let count = self.input.len().min(chain.writable as usize);
            transport.write_chain(memory, &chain, 0, &self.input[..count])?;
            self.input.drain(..count);
            transport.complete_chain(
                memory,
                QueueIndex(0),
                &chain,
                u32::try_from(count).expect("bounded by u32"),
            )?;
        }
        Ok(())
    }
}

impl VirtioDevice for ConsoleDevice {
    fn read_config(&self, offset: u32, width: AccessWidth) -> u32 {
        config_bytes(
            &[self.width.to_le_bytes(), self.height.to_le_bytes()].concat(),
            offset,
            width,
        )
    }
    fn write_config(&mut self, _: u32, _: u32, _: AccessWidth) {}
    fn notify(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        queue: QueueIndex,
    ) -> Result<(), DeviceError> {
        if queue.0 == 0 {
            return self.drain_input(transport, memory);
        }
        while let Some(chain) = transport.next_chain(memory, queue)? {
            let written = if queue.0 == 1 {
                let mut data = vec![0; chain.readable as usize];
                transport.read_chain(memory, &chain, 0, &mut data)?;
                self.output.extend(data);
                0
            } else {
                return Err(DeviceError::InvalidRequest);
            };
            transport.complete_chain(memory, queue, &chain, written)?;
        }
        Ok(())
    }
}

pub trait NetworkBackend {
    /// Sends one Ethernet frame.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::Backend` when the host rejects the frame.
    fn transmit(&mut self, packet: &[u8]) -> Result<(), DeviceError>;
}

impl<T: NetworkBackend + ?Sized> NetworkBackend for Box<T> {
    fn transmit(&mut self, packet: &[u8]) -> Result<(), DeviceError> {
        (**self).transmit(packet)
    }
}
pub struct NetworkDevice<B> {
    backend: B,
    mac: [u8; 6],
    receive: Vec<Vec<u8>>,
}
impl<B> NetworkDevice<B> {
    pub const fn new(backend: B, mac: [u8; 6]) -> Self {
        Self {
            backend,
            mac,
            receive: Vec::new(),
        }
    }
    pub fn push_packet(&mut self, packet: Vec<u8>) {
        self.receive.push(packet);
    }
    /// Delivers a frame to an already posted receive chain.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable or malformed receive queue.
    pub fn receive_packet(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        packet: Vec<u8>,
    ) -> Result<(), DeviceError>
    where
        B: NetworkBackend,
    {
        if transport.status() & 4 == 0 {
            return Err(DeviceError::InvalidRequest);
        }
        self.push_packet(packet);
        self.drain_receive(transport, memory)
    }
    pub fn backend(&self) -> &B {
        &self.backend
    }
}
impl<B: NetworkBackend> NetworkDevice<B> {
    fn drain_receive(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
    ) -> Result<(), DeviceError> {
        while !self.receive.is_empty() {
            let Some(chain) = transport.next_chain(memory, QueueIndex(0))? else {
                break;
            };
            let packet = self.receive.remove(0);
            let total = NET_HEADER_LEN + packet.len();
            if total > chain.writable as usize {
                return Err(DeviceError::InvalidRequest);
            }
            transport.write_chain(memory, &chain, 0, &[0; NET_HEADER_LEN])?;
            transport.write_chain(memory, &chain, 10, &packet)?;
            transport.complete_chain(
                memory,
                QueueIndex(0),
                &chain,
                u32::try_from(total).map_err(|_| DeviceError::InvalidRequest)?,
            )?;
        }
        Ok(())
    }
}
impl<B: NetworkBackend> VirtioDevice for NetworkDevice<B> {
    fn read_config(&self, offset: u32, width: AccessWidth) -> u32 {
        config_bytes(&self.mac, offset, width)
    }
    fn write_config(&mut self, _: u32, _: u32, _: AccessWidth) {}
    fn notify(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        queue: QueueIndex,
    ) -> Result<(), DeviceError> {
        if queue.0 == 0 {
            return self.drain_receive(transport, memory);
        }
        while let Some(chain) = transport.next_chain(memory, queue)? {
            let written = if queue.0 == 1 {
                if chain.readable < 10 {
                    return Err(DeviceError::InvalidRequest);
                }
                let mut data = vec![0; chain.readable as usize];
                transport.read_chain(memory, &chain, 0, &mut data)?;
                self.backend.transmit(&data[NET_HEADER_LEN..])?;
                0
            } else {
                return Err(DeviceError::InvalidRequest);
            };
            transport.complete_chain(memory, queue, &chain, written)?;
        }
        Ok(())
    }
}

pub trait NinePBackend {
    /// Processes one complete 9P message.
    ///
    /// # Errors
    ///
    /// Returns `DeviceError::Backend` when the host server fails.
    fn transact(&mut self, request: &[u8]) -> Result<Vec<u8>, DeviceError>;
}

impl<T: NinePBackend + ?Sized> NinePBackend for Box<T> {
    fn transact(&mut self, request: &[u8]) -> Result<Vec<u8>, DeviceError> {
        (**self).transact(request)
    }
}
pub struct NinePDevice<B> {
    backend: B,
    tag: Vec<u8>,
}
impl<B> NinePDevice<B> {
    pub fn new(backend: B, tag: &[u8]) -> Self {
        Self {
            backend,
            tag: tag.to_vec(),
        }
    }
}
impl<B: NinePBackend> VirtioDevice for NinePDevice<B> {
    fn read_config(&self, offset: u32, width: AccessWidth) -> u32 {
        let len = u16::try_from(self.tag.len()).unwrap_or(u16::MAX);
        let mut c = len.to_le_bytes().to_vec();
        c.extend(&self.tag);
        config_bytes(&c, offset, width)
    }
    fn write_config(&mut self, _: u32, _: u32, _: AccessWidth) {}
    fn notify(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        queue: QueueIndex,
    ) -> Result<(), DeviceError> {
        while let Some(chain) = transport.next_chain(memory, queue)? {
            let mut req = vec![0; chain.readable as usize];
            transport.read_chain(memory, &chain, 0, &mut req)?;
            validate_9p(&req)?;
            let reply = self.backend.transact(&req)?;
            validate_9p(&reply)?;
            if req[5..7] != reply[5..7] || reply.len() > chain.writable as usize {
                return Err(DeviceError::InvalidRequest);
            }
            transport.write_chain(memory, &chain, 0, &reply)?;
            let written = u32::try_from(reply.len()).map_err(|_| DeviceError::InvalidRequest)?;
            transport.complete_chain(memory, queue, &chain, written)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputEvent {
    pub kind: u16,
    pub code: u16,
    pub value: u32,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputKind {
    Keyboard,
    Mouse,
    Tablet,
}
pub struct InputDevice {
    kind: InputKind,
    select: u8,
    subsel: u8,
    events: Vec<InputEvent>,
    buttons: u32,
}
impl InputDevice {
    #[must_use]
    pub const fn new(kind: InputKind) -> Self {
        Self {
            kind,
            select: 0,
            subsel: 0,
            events: Vec::new(),
            buttons: 0,
        }
    }
    pub fn push_event(&mut self, event: InputEvent) {
        self.events.push(event);
    }

    /// Sends a key transition followed by a synchronization event.
    ///
    /// # Errors
    ///
    /// Returns an error for a non-keyboard device or unavailable event chains.
    pub fn send_key(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        code: u16,
        down: bool,
    ) -> Result<(), DeviceError> {
        if self.kind != InputKind::Keyboard || transport.status() & 4 == 0 {
            return Err(DeviceError::InvalidRequest);
        }
        self.events.extend([
            InputEvent {
                kind: 1,
                code,
                value: u32::from(down),
            },
            InputEvent {
                kind: 0,
                code: 0,
                value: 0,
            },
        ]);
        self.drain_events(transport, memory)
    }

    /// Sends relative mouse or absolute tablet movement, button changes, and synchronization.
    ///
    /// # Errors
    ///
    /// Returns an error for a keyboard device or unavailable event chains.
    pub fn send_pointer(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        position: (i32, i32),
        wheel: i32,
        buttons: u32,
    ) -> Result<(), DeviceError> {
        if transport.status() & 4 == 0 {
            return Err(DeviceError::InvalidRequest);
        }
        let event_kind = match self.kind {
            InputKind::Mouse => 2,
            InputKind::Tablet => 3,
            InputKind::Keyboard => return Err(DeviceError::InvalidRequest),
        };
        self.events.push(InputEvent {
            kind: event_kind,
            code: 0,
            value: u32::from_ne_bytes(position.0.to_ne_bytes()),
        });
        self.events.push(InputEvent {
            kind: event_kind,
            code: 1,
            value: u32::from_ne_bytes(position.1.to_ne_bytes()),
        });
        if wheel != 0 {
            self.events.push(InputEvent {
                kind: 2,
                code: 8,
                value: u32::from_ne_bytes(wheel.to_ne_bytes()),
            });
        }
        for (bit, code) in [0x110_u16, 0x111, 0x112].into_iter().enumerate() {
            let mask = 1_u32 << bit;
            if buttons & mask != self.buttons & mask {
                self.events.push(InputEvent {
                    kind: 1,
                    code,
                    value: u32::from(buttons & mask != 0),
                });
            }
        }
        self.buttons = buttons;
        self.events.push(InputEvent {
            kind: 0,
            code: 0,
            value: 0,
        });
        self.drain_events(transport, memory)
    }

    fn drain_events(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
    ) -> Result<(), DeviceError> {
        while !self.events.is_empty() {
            let Some(chain) = transport.next_chain(memory, QueueIndex(0))? else {
                break;
            };
            if chain.writable < 8 {
                return Err(DeviceError::InvalidRequest);
            }
            let event = self.events.remove(0);
            let mut bytes = [0; 8];
            bytes[..2].copy_from_slice(&event.kind.to_le_bytes());
            bytes[2..4].copy_from_slice(&event.code.to_le_bytes());
            bytes[4..].copy_from_slice(&event.value.to_le_bytes());
            transport.write_chain(memory, &chain, 0, &bytes)?;
            transport.complete_chain(memory, QueueIndex(0), &chain, 8)?;
        }
        Ok(())
    }
}
impl VirtioDevice for InputDevice {
    fn read_config(&self, offset: u32, width: AccessWidth) -> u32 {
        let mut c = vec![0; 256];
        c[0] = self.select;
        c[1] = self.subsel;
        match self.select {
            1 => {
                let name = match self.kind {
                    InputKind::Keyboard => b"virtio_keyboard".as_slice(),
                    InputKind::Mouse => b"virtio_mouse".as_slice(),
                    InputKind::Tablet => b"virtio_tablet".as_slice(),
                };
                c[2] = u8::try_from(name.len()).expect("input device names fit config size");
                c[8..8 + name.len()].copy_from_slice(name);
            }
            0x11 => input_event_bits(self.kind, self.subsel, &mut c),
            0x12 if self.kind == InputKind::Tablet && self.subsel <= 1 => {
                c[2] = 20;
                c[12..16].copy_from_slice(&32_767_u32.to_le_bytes());
            }
            _ => {}
        }
        config_bytes(&c, offset, width)
    }
    fn write_config(&mut self, offset: u32, value: u32, width: AccessWidth) {
        if width == AccessWidth::Byte {
            if offset == 0 {
                self.select = value.to_le_bytes()[0];
            } else if offset == 1 {
                self.subsel = value.to_le_bytes()[0];
            }
        }
    }
    fn notify(
        &mut self,
        transport: &mut VirtioTransport,
        memory: &mut PhysicalMemory,
        queue: QueueIndex,
    ) -> Result<(), DeviceError> {
        if queue.0 == 0 {
            return self.drain_events(transport, memory);
        }
        while let Some(chain) = transport.next_chain(memory, queue)? {
            if queue.0 == 1 {
                transport.complete_chain(memory, queue, &chain, 0)?;
                continue;
            }
            if queue.0 != 1 {
                return Err(DeviceError::InvalidRequest);
            }
        }
        Ok(())
    }
}

fn validate_9p(message: &[u8]) -> Result<(), DeviceError> {
    if message.len() < 7
        || u32::from_le_bytes(message[..4].try_into().expect("four-byte slice")) as usize
            != message.len()
    {
        Err(DeviceError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn input_event_bits(kind: InputKind, event: u8, config: &mut [u8]) {
    match (kind, event) {
        (InputKind::Keyboard, 1) => {
            config[2] = 16;
            config[8..24].fill(0xff);
        }
        (InputKind::Keyboard, 0x14) => config[2] = 1,
        (InputKind::Mouse | InputKind::Tablet, 1) => {
            config[2] = 64;
            for code in [0x110_usize, 0x111, 0x112] {
                config[8 + code / 8] |= 1 << (code % 8);
            }
        }
        (InputKind::Mouse, 2) => {
            config[2] = 2;
            config[8] = 0b11;
            config[9] = 1;
        }
        (InputKind::Tablet, 2) => {
            config[2] = 2;
            config[9] = 1;
        }
        (InputKind::Tablet, 3) => {
            config[2] = 1;
            config[8] = 0b11;
        }
        _ => {}
    }
}
fn config_u64(value: u64, offset: u32, width: AccessWidth) -> u32 {
    config_bytes(&value.to_le_bytes(), offset, width)
}
fn config_bytes(bytes: &[u8], offset: u32, width: AccessWidth) -> u32 {
    let start = offset as usize;
    let count = width.bytes();
    if count > 4 || start.checked_add(count).is_none_or(|end| end > bytes.len()) {
        return 0;
    }
    let mut value = [0; 4];
    value[..count].copy_from_slice(&bytes[start..start + count]);
    u32::from_le_bytes(value)
}
