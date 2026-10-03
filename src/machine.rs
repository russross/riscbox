//! Composition and boot loading for the browser RISC-V virtual machine.

use core::fmt;
use std::io::Read;

use flate2::read::MultiGzDecoder;

use crate::block_storage::{BlockStore, HttpRequest, StorageError};
use crate::entropy::{EntropyError, SharedEntropy, SystemEntropy};
use crate::fdt::{FdtConfig, FramebufferDescription, build as build_fdt};
use crate::guest_memory::GuestMemory;
use crate::guest_memory::{
    AccessWidth, ArenaOffset, DeviceWidths, GuestAddress, MemoryAccess, MemoryError, RamFlags,
    RegionId,
};
use crate::ninep_protocol::NinePEndpoint;
use crate::platform::{
    Aclint, FinishStatus, Finisher, GoldfishRtc, MIP_MSIP, MIP_MTIP, Plic, Uart16550,
};
use crate::tinyemu_core::{BusError, Core, CpuRunExitReason, CpuRunResult, PlatformCallbacks};
use crate::virtio::{MMIO_SIZE, VirtioTransport};
use crate::virtio_devices::{
    BlockBackend, BlockDevice, ConsoleDevice, DeviceError, EntropyDevice, InputDevice, InputKind,
    NetworkBackend, NetworkDevice, NetworkIngress, NinePDevice, VirtioMmioDevice,
};

pub const RAM_BASE: u64 = 0x8000_0000;
pub const RESET_RAM_SIZE: u64 = 0x1_0000;
pub const FINISHER_BASE: u64 = 0x10_0000;
pub const RTC_BASE: u64 = 0x10_1000;
pub const ACLINT_BASE: u64 = 0x200_0000;
pub const FRAMEBUFFER_BASE: u64 = 0x410_0000;
pub const PLIC_BASE: u64 = 0xc00_0000;
pub const UART_BASE: u64 = 0x1000_0000;
pub const VIRTIO_BASE: u64 = 0x1000_1000;

const KERNEL_OFFSET: u64 = 0x20_0000;
const FDT_ALIGNMENT: u64 = 0x20_0000;
const FW_DYNAMIC_INFO_ADDRESS: u64 = 0x1028;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferConfig {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MachineConfig {
    pub ram_size: u64,
    pub framebuffer: Option<FramebufferConfig>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootImages<'a> {
    pub firmware: Option<&'a [u8]>,
    pub kernel: Option<&'a [u8]>,
    pub initrd: Option<&'a [u8]>,
    pub command_line: &'a str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BootAddresses {
    pub firmware: Option<u64>,
    pub kernel: Option<u64>,
    pub initrd: Option<u64>,
    pub fdt: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootLayout {
    pub firmware_address: Option<u64>,
    pub kernel_address: Option<u64>,
    pub initrd_address: Option<u64>,
    pub fdt_address: u64,
    pub fdt_size: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RedrawSpan {
    pub y: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramebufferUpdate {
    pub arena_offset: ArenaOffset,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MachineError {
    Memory(MemoryError),
    CoreAllocation,
    InvalidRamSize(u64),
    InvalidFramebuffer,
    FirmwareTooLarge,
    KernelTooLarge,
    InvalidCompressedKernel,
    InvalidCompressedFirmware,
    DeviceTreeTooLarge,
    InvalidBootLayout(String),
    Virtio(DeviceError),
    VirtioSlotLimit,
    WrongVirtioDevice,
    Storage(StorageError),
    Entropy(EntropyError),
}

impl fmt::Display for MachineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(error) => error.fmt(formatter),
            Self::CoreAllocation => formatter.write_str("could not allocate TinyEMU core"),
            Self::InvalidRamSize(size) => write!(formatter, "invalid platform RAM size {size:#x}"),
            Self::InvalidFramebuffer => formatter.write_str("invalid framebuffer dimensions"),
            Self::FirmwareTooLarge => formatter.write_str("firmware does not fit in RAM"),
            Self::KernelTooLarge => formatter.write_str("kernel does not fit in RAM"),
            Self::InvalidCompressedKernel => formatter.write_str("invalid gzip-compressed kernel"),
            Self::InvalidCompressedFirmware => {
                formatter.write_str("invalid gzip-compressed firmware")
            }
            Self::DeviceTreeTooLarge => formatter.write_str("device tree does not fit in RAM"),
            Self::InvalidBootLayout(reason) => formatter.write_str(reason),
            Self::Virtio(error) => error.fmt(formatter),
            Self::VirtioSlotLimit => formatter.write_str("too many `VirtIO` MMIO devices"),
            Self::WrongVirtioDevice => {
                formatter.write_str("`VirtIO` slot has the wrong device type")
            }
            Self::Storage(error) => error.fmt(formatter),
            Self::Entropy(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for MachineError {}

impl From<MemoryError> for MachineError {
    fn from(value: MemoryError) -> Self {
        Self::Memory(value)
    }
}

impl From<DeviceError> for MachineError {
    fn from(value: DeviceError) -> Self {
        Self::Virtio(value)
    }
}

impl From<StorageError> for MachineError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<EntropyError> for MachineError {
    fn from(value: EntropyError) -> Self {
        Self::Entropy(value)
    }
}

#[derive(Clone, Debug)]
struct Framebuffer {
    region: RegionId,
    width: u32,
    height: u32,
    stride: u32,
    size: u32,
}

type DynBlock = VirtioMmioDevice<BlockDevice<Box<dyn BlockBackend>>>;
type StorageBlock = VirtioMmioDevice<BlockDevice<BlockStore>>;
type DynNetwork = VirtioMmioDevice<NetworkDevice<Box<dyn NetworkBackend>>>;
type ResidentNineP = VirtioMmioDevice<NinePDevice>;

enum VirtioSlot {
    Block(DynBlock),
    StorageBlock(StorageBlock),
    Console(VirtioMmioDevice<ConsoleDevice>),
    Network(DynNetwork),
    NineP(ResidentNineP),
    Input(VirtioMmioDevice<InputDevice>),
    Entropy(VirtioMmioDevice<EntropyDevice>),
}

impl VirtioSlot {
    fn reset(&mut self) {
        match self {
            Self::Block(device) => device.reset(),
            Self::StorageBlock(device) => device.reset(),
            Self::Console(device) => device.reset(),
            Self::Network(device) => device.reset(),
            Self::NineP(device) => device.reset(),
            Self::Input(device) => device.reset(),
            Self::Entropy(device) => device.reset(),
        }
    }

    fn needs_host(&self) -> bool {
        // Queue notifications handled entirely in Rust leave these queues empty.
        match self {
            Self::StorageBlock(device) => device.device.backend().has_outgoing(),
            _ => false,
        }
    }

    fn read(&self, offset: u32, width: AccessWidth) -> u32 {
        match self {
            Self::Block(device) => device.read(offset, width),
            Self::StorageBlock(device) => device.read(offset, width),
            Self::Console(device) => device.read(offset, width),
            Self::Network(device) => device.read(offset, width),
            Self::NineP(device) => device.read(offset, width),
            Self::Input(device) => device.read(offset, width),
            Self::Entropy(device) => device.read(offset, width),
        }
    }

    fn write(
        &mut self,
        memory: &mut dyn MemoryAccess,
        offset: u32,
        value: u32,
        width: AccessWidth,
    ) -> Result<(), DeviceError> {
        match self {
            Self::Block(device) => device.write(memory, offset, value, width),
            Self::StorageBlock(device) => device.write(memory, offset, value, width),
            Self::Console(device) => device.write(memory, offset, value, width),
            Self::Network(device) => device.write(memory, offset, value, width),
            Self::NineP(device) => device.write(memory, offset, value, width),
            Self::Input(device) => device.write(memory, offset, value, width),
            Self::Entropy(device) => device.write(memory, offset, value, width),
        }
    }

    fn irq(&self) -> bool {
        match self {
            Self::Block(device) => device.transport.irq(),
            Self::StorageBlock(device) => device.transport.irq(),
            Self::Console(device) => device.transport.irq(),
            Self::Network(device) => device.transport.irq(),
            Self::NineP(device) => device.transport.irq(),
            Self::Input(device) => device.transport.irq(),
            Self::Entropy(device) => device.transport.irq(),
        }
    }
}

pub struct PlatformBus {
    memory: GuestMemory,
    aclint: Aclint,
    plic: Plic,
    uart: Uart16550,
    rtc: GoldfishRtc,
    finisher: Finisher,
    framebuffer: Option<Framebuffer>,
    virtio: Vec<VirtioSlot>,
    guest_timer_ticks: u64,
    guest_rtc_ns: u64,
    host_service_requested: bool,
    timer_reprogrammed: bool,
}

impl PlatformBus {
    fn new(config: MachineConfig, core: &Core) -> Result<Self, MachineError> {
        if config.ram_size == 0 || !config.ram_size.is_multiple_of(4096) {
            return Err(MachineError::InvalidRamSize(config.ram_size));
        }
        let mut memory = GuestMemory::new(core);
        memory.register_ram(GuestAddress(RAM_BASE), config.ram_size, RamFlags::default())?;
        memory.register_ram(GuestAddress(0), RESET_RAM_SIZE, RamFlags::default())?;
        let framebuffer = config
            .framebuffer
            .map(|display| {
                let stride = display
                    .width
                    .checked_mul(4)
                    .ok_or(MachineError::InvalidFramebuffer)?;
                let used = display
                    .height
                    .checked_mul(stride)
                    .ok_or(MachineError::InvalidFramebuffer)?;
                if used == 0 {
                    return Err(MachineError::InvalidFramebuffer);
                }
                let size = used
                    .checked_add(0xffff)
                    .ok_or(MachineError::InvalidFramebuffer)?
                    & !0xffff;
                let region = memory.register_ram(
                    GuestAddress(FRAMEBUFFER_BASE),
                    u64::from(size),
                    RamFlags::DIRTY_TRACKING,
                )?;
                Ok(Framebuffer {
                    region,
                    width: display.width,
                    height: display.height,
                    stride,
                    size,
                })
            })
            .transpose()?;
        memory.register_device(GuestAddress(FINISHER_BASE), 0x1000, DeviceWidths::U32)?;
        memory.register_device(GuestAddress(RTC_BASE), 0x1000, DeviceWidths::U32)?;
        memory.register_device(GuestAddress(ACLINT_BASE), 0xc000, DeviceWidths::U32)?;
        memory.register_device(GuestAddress(PLIC_BASE), 0x400_0000, DeviceWidths::U32)?;
        memory.register_device(GuestAddress(UART_BASE), 0x100, DeviceWidths::U8)?;
        memory.register_device(
            GuestAddress(VIRTIO_BASE),
            MMIO_SIZE * 31,
            DeviceWidths::U8
                .union(DeviceWidths::U16)
                .union(DeviceWidths::U32),
        )?;
        Ok(Self {
            memory,
            aclint: Aclint::default(),
            plic: Plic::default(),
            uart: Uart16550::default(),
            rtc: GoldfishRtc::default(),
            finisher: Finisher::default(),
            framebuffer,
            virtio: Vec::new(),
            guest_timer_ticks: 0,
            guest_rtc_ns: 0,
            host_service_requested: false,
            timer_reprogrammed: false,
        })
    }

    fn update_device_irqs(&mut self) {
        self.plic.set_irq(10, self.uart.irq());
        self.plic.set_irq(11, self.rtc.irq());
        for (index, device) in self.virtio.iter().enumerate() {
            self.plic.set_irq(virtio_irq(index), device.irq());
        }
    }

    fn read_mmio(&mut self, address: u64, width: AccessWidth) -> Result<u64, BusError> {
        let value = if (FINISHER_BASE..FINISHER_BASE + 0x1000).contains(&address)
            && width == AccessWidth::Word
        {
            0
        } else if (RTC_BASE..RTC_BASE + 0x1000).contains(&address) && width == AccessWidth::Word {
            u64::from(self.rtc.read(offset(address, RTC_BASE)?, self.guest_rtc_ns))
        } else if (ACLINT_BASE..ACLINT_BASE + 0xc000).contains(&address)
            && matches!(width, AccessWidth::Word | AccessWidth::DoubleWord)
        {
            let device_offset = offset(address, ACLINT_BASE)?;
            let low = u64::from(self.aclint.read(device_offset, self.guest_timer_ticks));
            if width == AccessWidth::DoubleWord {
                low | (u64::from(self.aclint.read(device_offset + 4, self.guest_timer_ticks)) << 32)
            } else {
                low
            }
        } else if (PLIC_BASE..PLIC_BASE + 0x400_0000).contains(&address)
            && width == AccessWidth::Word
        {
            u64::from(self.plic.read(offset(address, PLIC_BASE)?))
        } else if (UART_BASE..UART_BASE + 0x100).contains(&address) && width == AccessWidth::Byte {
            u64::from(self.uart.read(offset(address, UART_BASE)?))
        } else if let Some((index, device_offset)) = virtio_location(address) {
            let device = self.virtio.get(index).ok_or(BusError::AccessFault)?;
            u64::from(device.read(device_offset, width))
        } else {
            return Err(BusError::AccessFault);
        };
        self.update_device_irqs();
        Ok(value)
    }

    fn write_mmio(&mut self, address: u64, width: AccessWidth, value: u64) -> Result<(), BusError> {
        let value32 = low_u32(value);
        if (FINISHER_BASE..FINISHER_BASE + 0x1000).contains(&address) && width == AccessWidth::Word
        {
            self.finisher
                .write(offset(address, FINISHER_BASE)?, value32);
            self.host_service_requested |= self.finisher.status() != FinishStatus::Running;
        } else if (RTC_BASE..RTC_BASE + 0x1000).contains(&address) && width == AccessWidth::Word {
            let device_offset = offset(address, RTC_BASE)?;
            self.rtc.write(device_offset, value32, self.guest_rtc_ns);
            if matches!(device_offset, 0 | 4 | 8 | 0x0c | 0x14) {
                self.host_service_requested = true;
                self.timer_reprogrammed = true;
            }
        } else if (ACLINT_BASE..ACLINT_BASE + 0xc000).contains(&address)
            && matches!(width, AccessWidth::Word | AccessWidth::DoubleWord)
        {
            let device_offset = offset(address, ACLINT_BASE)?;
            self.aclint.write(device_offset, value32);
            if width == AccessWidth::DoubleWord {
                self.aclint.write(device_offset + 4, (value >> 32) as u32);
            }
            if matches!(device_offset, 0x4000 | 0x4004) {
                self.host_service_requested = true;
                self.timer_reprogrammed = true;
            }
        } else if (PLIC_BASE..PLIC_BASE + 0x400_0000).contains(&address)
            && width == AccessWidth::Word
        {
            self.plic.write(offset(address, PLIC_BASE)?, value32);
        } else if (UART_BASE..UART_BASE + 0x100).contains(&address) && width == AccessWidth::Byte {
            self.uart
                .write(offset(address, UART_BASE)?, value.to_le_bytes()[0]);
        } else if let Some((index, device_offset)) = virtio_location(address) {
            let device = self.virtio.get_mut(index).ok_or(BusError::AccessFault)?;
            device
                .write(&mut self.memory, device_offset, value32, width)
                .map_err(|_| BusError::AccessFault)?;
            self.host_service_requested |= device.needs_host();
        } else {
            return Err(BusError::AccessFault);
        }
        self.update_device_irqs();
        Ok(())
    }
}

impl PlatformBus {
    /// Reads a physical RAM or device address.
    ///
    /// # Errors
    /// Returns an access fault for an unsupported address or width.
    pub fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, BusError> {
        self.memory
            .read(address, width)
            .map_err(|_| BusError::AccessFault)
            .or_else(|_| self.read_mmio(address.0, width))
    }

    /// Writes a physical RAM or device address.
    ///
    /// # Errors
    /// Returns an access fault for an unsupported address or width.
    pub fn write(
        &mut self,
        address: GuestAddress,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), BusError> {
        self.memory
            .write(address, width, value)
            .map_err(|_| BusError::AccessFault)
            .or_else(|_| self.write_mmio(address.0, width, value))
    }

    fn interrupt_mask(&self) -> u32 {
        let external = self.plic.cpu_interrupts();
        let mut direct = 0;
        if self.aclint.software_interrupt() {
            direct |= MIP_MSIP;
        }
        if self.aclint.timer_interrupt(self.guest_timer_ticks) {
            direct |= MIP_MTIP;
        }
        direct | external
    }
}

impl PlatformCallbacks for PlatformBus {
    fn read(&mut self, address: u64, width: u32) -> Result<u32, BusError> {
        let width = callback_width(width).ok_or(BusError::AccessFault)?;
        let value = self.read_mmio(address, width)?;
        u32::try_from(value).map_err(|_| BusError::AccessFault)
    }

    fn write(&mut self, address: u64, width: u32, value: u32) -> Result<(), BusError> {
        self.write_mmio(
            address,
            callback_width(width).ok_or(BusError::AccessFault)?,
            u64::from(value),
        )
    }

    fn interrupts(&self) -> u32 {
        self.interrupt_mask()
    }

    fn host_service_requested(&self) -> bool {
        self.host_service_requested
    }
}

pub struct Machine {
    bus: PlatformBus,
    cpu: Core,
    config: MachineConfig,
    entropy: SharedEntropy,
}

impl Machine {
    /// Borrows a platform disk between CPU activations.
    /// # Errors
    /// Rejects a missing slot or a non-storage device.
    pub fn storage_mut(&mut self, slot: usize) -> Result<&mut BlockStore, MachineError> {
        let Some(VirtioSlot::StorageBlock(device)) = self.bus.virtio.get_mut(slot) else {
            return Err(MachineError::WrongVirtioDevice);
        };
        Ok(device.device.backend_mut())
    }

    /// Retires storage requests and guest filesystem sessions at poweroff.
    pub fn retire_storage_work(&mut self) {
        for slot in &mut self.bus.virtio {
            if matches!(slot, VirtioSlot::StorageBlock(_) | VirtioSlot::NineP(_)) {
                slot.reset();
            }
        }
        self.bus.update_device_irqs();
    }

    /// Clears RAM while retaining its stable `TinyEMU` mappings.
    /// # Errors
    /// Reports a failed RAM write.
    pub fn clear_ram(&mut self) -> Result<(), MachineError> {
        let zeros = [0; 4096];
        for offset in (0..self.config.ram_size).step_by(zeros.len()) {
            self.write_ram(RAM_BASE + offset, &zeros)?;
        }
        Ok(())
    }
    /// Creates an unbooted virtual platform.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid RAM or framebuffer sizes or an exhausted memory map.
    pub fn new(config: MachineConfig) -> Result<Self, MachineError> {
        let entropy = std::rc::Rc::new(std::cell::RefCell::new(SystemEntropy::open()?));
        Self::new_with_entropy(config, entropy)
    }

    /// Creates an unbooted virtual platform with an explicit entropy source.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid RAM or framebuffer sizes or an exhausted memory map.
    pub fn new_with_entropy(
        config: MachineConfig,
        entropy: SharedEntropy,
    ) -> Result<Self, MachineError> {
        let cpu = Core::new().ok_or(MachineError::CoreAllocation)?;
        let bus = PlatformBus::new(config, &cpu)?;
        Ok(Self {
            bus,
            cpu,
            config,
            entropy,
        })
    }

    /// Resets guest-facing state while retaining RAM mappings and host backends.
    /// Boot images must be loaded again before the next CPU run.
    ///
    /// # Errors
    /// Returns an error if a replacement CPU cannot be allocated.
    pub fn reset(&mut self) -> Result<(), MachineError> {
        if !self.cpu.reset_cpu() {
            return Err(MachineError::CoreAllocation);
        }
        self.bus.aclint = Aclint::default();
        self.bus.plic = Plic::default();
        self.bus.uart = Uart16550::default();
        self.bus.rtc = GoldfishRtc::default();
        self.bus.finisher = Finisher::default();
        self.bus.guest_timer_ticks = 0;
        self.bus.guest_rtc_ns = 0;
        self.bus.host_service_requested = false;
        self.bus.timer_reprogrammed = false;
        for device in &mut self.bus.virtio {
            device.reset();
        }
        self.bus.update_device_irqs();
        self.sync_interrupts();
        Ok(())
    }

    /// Adds a `VirtIO` block device and returns its MMIO slot.
    ///
    /// # Errors
    ///
    /// Returns an error after the 32 available PLIC sources are exhausted.
    pub fn add_block_device(
        &mut self,
        backend: Box<dyn BlockBackend>,
        id: [u8; 20],
    ) -> Result<usize, MachineError> {
        self.add_virtio(VirtioSlot::Block(VirtioMmioDevice::new(
            VirtioTransport::new(2, 0, &[16]),
            BlockDevice::new(backend, id),
        )))
    }

    /// Adds an asynchronous HTTP-backed block device and returns its MMIO slot.
    ///
    /// # Errors
    ///
    /// Returns an error after the 32 available PLIC sources are exhausted.
    pub fn add_storage_block_device(
        &mut self,
        backend: BlockStore,
        id: [u8; 20],
    ) -> Result<usize, MachineError> {
        self.add_virtio(VirtioSlot::StorageBlock(VirtioMmioDevice::new(
            VirtioTransport::new(2, 0, &[16]),
            BlockDevice::new(backend, id),
        )))
    }

    /// Adds a `VirtIO` console and returns its MMIO slot.
    ///
    /// # Errors
    ///
    /// Returns an error after the 32 available PLIC sources are exhausted.
    pub fn add_console_device(&mut self, width: u16, height: u16) -> Result<usize, MachineError> {
        self.add_virtio(VirtioSlot::Console(VirtioMmioDevice::new(
            VirtioTransport::new(3, 1, &[16, 16]),
            ConsoleDevice::new(width, height),
        )))
    }

    /// Adds a `VirtIO` network device and returns its MMIO slot.
    ///
    /// # Errors
    ///
    /// Returns an error after the 32 available PLIC sources are exhausted.
    pub fn add_network_device(
        &mut self,
        backend: Box<dyn NetworkBackend>,
        mac: [u8; 6],
    ) -> Result<usize, MachineError> {
        self.add_virtio(VirtioSlot::Network(VirtioMmioDevice::new(
            VirtioTransport::new(1, (1 << 5) | (1 << 16), &[16, 16]),
            NetworkDevice::new(backend, mac),
        )))
    }

    /// Generates a locally administered unicast MAC address from machine entropy.
    ///
    /// # Errors
    ///
    /// Returns an error when the host entropy source fails.
    pub fn generate_network_mac(&mut self) -> Result<[u8; 6], MachineError> {
        let mut mac = [0; 6];
        self.entropy.borrow_mut().fill(&mut mac)?;
        mac[0] = (mac[0] | 0x02) & 0xfe;
        Ok(mac)
    }

    /// Adds a raw-protocol `VirtIO` 9p device and returns its MMIO slot.
    ///
    /// # Errors
    ///
    /// Returns an error after the 32 available PLIC sources are exhausted.
    pub fn add_ninep_device(
        &mut self,
        endpoint: NinePEndpoint,
        tag: &[u8],
    ) -> Result<usize, MachineError> {
        self.add_virtio(VirtioSlot::NineP(VirtioMmioDevice::new(
            VirtioTransport::new(9, 1, &[128]),
            NinePDevice::new(endpoint, tag),
        )))
    }

    /// Adds a `VirtIO` input device and returns its MMIO slot.
    ///
    /// # Errors
    ///
    /// Returns an error after the 32 available PLIC sources are exhausted.
    pub fn add_input_device(&mut self, kind: InputKind) -> Result<usize, MachineError> {
        self.add_virtio(VirtioSlot::Input(VirtioMmioDevice::new(
            VirtioTransport::new(18, 0, &[256, 256]),
            InputDevice::new(kind),
        )))
    }

    /// Adds a `VirtIO` entropy device and returns its MMIO slot.
    ///
    /// # Errors
    ///
    /// Returns an error after the 32 available PLIC sources are exhausted.
    pub fn add_entropy_device(&mut self) -> Result<usize, MachineError> {
        self.add_virtio(VirtioSlot::Entropy(VirtioMmioDevice::new(
            VirtioTransport::new(4, 0, &[16]),
            EntropyDevice::new(self.entropy.clone()),
        )))
    }

    fn add_virtio(&mut self, device: VirtioSlot) -> Result<usize, MachineError> {
        let index = self.bus.virtio.len();
        if virtio_irq_checked(index).is_none() {
            return Err(MachineError::VirtioSlotLimit);
        }
        self.bus.virtio.push(device);
        Ok(index)
    }

    /// Loads firmware and optional kernel/initrd images and installs the reset vector and FDT.
    ///
    /// # Errors
    ///
    /// Returns an error when images overlap, do not fit in guest RAM, or contain
    /// an invalid gzip-compressed payload.
    pub fn load_boot(&mut self, images: BootImages<'_>) -> Result<BootLayout, MachineError> {
        self.load_boot_at(images, BootAddresses::default())
    }

    /// Loads boot images at validated physical addresses. Absent firmware directly boots the kernel.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid images, addresses, or overlapping RAM regions.
    pub fn load_boot_at(
        &mut self,
        images: BootImages<'_>,
        addresses: BootAddresses,
    ) -> Result<BootLayout, MachineError> {
        // Resolve addresses before decompressing, so output is bounded by the
        // space actually available at each selected physical location.
        if images.firmware.is_some_and(<[u8]>::is_empty) {
            return Err(MachineError::InvalidBootLayout("firmware is empty".into()));
        }
        let firmware_address = images
            .firmware
            .map(|_| addresses.firmware.unwrap_or(RAM_BASE));
        let kernel_address = images
            .kernel
            .map(|_| addresses.kernel.unwrap_or(RAM_BASE + KERNEL_OFFSET));
        let initrd_address = images.initrd.map(|_| {
            addresses.initrd.unwrap_or_else(|| {
                kernel_address
                    .unwrap_or(RAM_BASE)
                    .saturating_add(self.config.ram_size.div_ceil(2).min(512 << 20))
            })
        });
        // Reject bad physical positions before subtracting from the RAM end
        // or reserving space for gzip output.
        let ram_end = RAM_BASE + self.config.ram_size;
        for (name, address) in [
            ("firmware", firmware_address),
            ("kernel", kernel_address),
            ("initrd", initrd_address),
        ] {
            if let Some(address) = address
                && (address < RAM_BASE || address >= ram_end)
            {
                return Err(MachineError::InvalidBootLayout(format!(
                    "{name} address is outside RAM"
                )));
            }
        }
        let decoded_firmware = images
            .firmware
            .filter(|image| image.starts_with(&[0x1f, 0x8b]))
            .map(|image| {
                decode_gzip(
                    image,
                    ram_end - firmware_address.unwrap_or(RAM_BASE),
                    MachineError::FirmwareTooLarge,
                    MachineError::InvalidCompressedFirmware,
                )
            })
            .transpose()?;
        let firmware = decoded_firmware.as_deref().or(images.firmware);
        // Linux and OpenSBI receive the final decoded bytes in guest RAM.
        // An initrd stays opaque because the guest owns its compression format.
        let kernel_limit = kernel_address.map_or(0, |address| ram_end - address);
        let decoded_kernel = images
            .kernel
            .filter(|image| image.starts_with(&[0x1f, 0x8b]))
            .map(|image| {
                decode_gzip(
                    image,
                    kernel_limit,
                    MachineError::KernelTooLarge,
                    MachineError::InvalidCompressedKernel,
                )
            })
            .transpose()?;
        let kernel = decoded_kernel.as_deref().or(images.kernel);
        // Check all payload intervals together, including configurations that
        // place an initrd below or between the other boot images.
        let mut regions = Vec::new();
        for (name, address, bytes) in [
            ("firmware", firmware_address, firmware),
            ("kernel", kernel_address, kernel),
            ("initrd", initrd_address, images.initrd),
        ] {
            if let (Some(address), Some(bytes)) = (address, bytes) {
                let end = address.checked_add(bytes.len() as u64).ok_or_else(|| {
                    MachineError::InvalidBootLayout(format!("{name} address overflows"))
                })?;
                if end > ram_end {
                    return Err(MachineError::InvalidBootLayout(format!(
                        "{name} does not fit in RAM"
                    )));
                }
                regions.push((name, address, end));
            }
        }
        if firmware.is_none() && kernel.is_none() {
            return Err(MachineError::InvalidBootLayout(
                "boot requires firmware or kernel".into(),
            ));
        }
        for left in 0..regions.len() {
            for right in left + 1..regions.len() {
                if regions[left].1 < regions[right].2 && regions[right].1 < regions[left].2 {
                    return Err(MachineError::InvalidBootLayout(format!(
                        "{} overlaps {}",
                        regions[left].0, regions[right].0
                    )));
                }
            }
        }
        let framebuffer = self
            .bus
            .framebuffer
            .as_ref()
            .map(|fb| FramebufferDescription {
                size: fb.size,
                width: fb.width,
                height: fb.height,
                stride: fb.stride,
            });
        let initrd = images
            .initrd
            .zip(initrd_address)
            .map(|(bytes, address)| (address, bytes.len() as u64));
        // The device tree contains the selected initrd range and current
        // platform devices, so build it after the payload locations settle.
        let mut rng_seed = [0; 32];
        self.entropy.borrow_mut().fill(&mut rng_seed)?;
        let tree = build_fdt(FdtConfig {
            ram_size: self.config.ram_size,
            command_line: images.command_line,
            initrd,
            virtio_count: u8::try_from(self.bus.virtio.len())
                .map_err(|_| MachineError::VirtioSlotLimit)?,
            rng_seed: Some(&rng_seed),
            framebuffer,
        });
        let limit = self.config.ram_size;
        let tree_len = u64::try_from(tree.len()).map_err(|_| MachineError::DeviceTreeTooLarge)?;
        if tree_len > limit {
            return Err(MachineError::DeviceTreeTooLarge);
        }
        let tree_address = addresses
            .fdt
            .unwrap_or(RAM_BASE + ((limit - tree_len) & !(FDT_ALIGNMENT - 1)));
        let tree_end = tree_address
            .checked_add(tree_len)
            .ok_or(MachineError::DeviceTreeTooLarge)?;
        // A caller-selected tree location follows the same bounds and
        // interval checks as payloads, plus the FDT's 8-byte alignment.
        if tree_address < RAM_BASE
            || tree_address & 7 != 0
            || tree_end > ram_end
            || regions
                .iter()
                .any(|(_, start, end)| tree_address < *end && *start < tree_end)
        {
            return Err(MachineError::DeviceTreeTooLarge);
        }
        if let Some((address, firmware)) = firmware_address.zip(firmware) {
            self.copy_to_ram(address, firmware)?;
        }
        if let Some((address, kernel)) = kernel_address.zip(kernel) {
            self.copy_to_ram(address, kernel)?;
        }
        if let Some((address, initrd_image)) = initrd_address.zip(images.initrd) {
            self.copy_to_ram(address, initrd_image)?;
        }
        // The ROM starts at the selected firmware or direct-kernel entry and
        // supplies the next-stage address through OpenSBI's dynamic-info ABI.
        self.copy_to_ram(tree_address, &tree)?;
        let entry = firmware_address.or(kernel_address).ok_or_else(|| {
            MachineError::InvalidBootLayout("boot requires firmware or kernel".into())
        })?;
        self.write_reset_vector(entry, tree_address, kernel_address)?;
        Ok(BootLayout {
            firmware_address,
            kernel_address,
            initrd_address,
            fdt_address: tree_address,
            fdt_size: u32::try_from(tree.len()).map_err(|_| MachineError::DeviceTreeTooLarge)?,
        })
    }

    fn copy_to_ram(&mut self, address: u64, bytes: &[u8]) -> Result<(), MachineError> {
        self.bus
            .memory
            .ram_range(GuestAddress(address), bytes.len(), true)?
            .copy_from_slice(bytes);
        Ok(())
    }

    fn write_reset_vector(
        &mut self,
        entry: u64,
        fdt_address: u64,
        next: Option<u64>,
    ) -> Result<(), MachineError> {
        let mut reset = Vec::with_capacity(88);
        // The ROM sets a2 to the dynamic-info words after its two pointers,
        // a0 to the hart ID, and a1 to the device tree pointer.
        for instruction in [
            0x0000_0297_u32,
            0x0282_8613,
            0xf140_2573,
            0x0202_b583,
            0x0182_b283,
            0x0002_8067,
        ] {
            reset.extend_from_slice(&instruction.to_le_bytes());
        }
        reset.extend_from_slice(&entry.to_le_bytes());
        reset.extend_from_slice(&fdt_address.to_le_bytes());
        // OpenSBI version 2 names the S-mode next stage and boot hart zero.
        // Other firmware may ignore a2 and use the same reset vector.
        for word in [0x4942_534f_u64, 2, next.unwrap_or(0), 1, 0, 0] {
            reset.extend_from_slice(&word.to_le_bytes());
        }
        debug_assert_eq!(0x1000 + 40, FW_DYNAMIC_INFO_ADDRESS);
        self.copy_to_ram(0x1000, &reset)
    }

    pub fn present_guest_clocks(&mut self, guest_timer_ticks: u64, guest_rtc_ns: u64) {
        self.bus.guest_timer_ticks = guest_timer_ticks;
        self.bus.guest_rtc_ns = guest_rtc_ns;
        self.bus.rtc.refresh_alarm(guest_rtc_ns);
        self.bus.update_device_irqs();
        self.sync_interrupts();
        self.cpu.set_guest_timer_ticks(guest_timer_ticks);
    }

    #[must_use]
    pub fn next_timer_remaining_guest_ticks(&self) -> Option<u64> {
        // Due compares are already reflected in interrupt state by present_guest_clocks.
        let now = self.bus.guest_timer_ticks;
        let mut delay = u64::MAX;
        if self.bus.aclint.timecmp() != u64::MAX && !self.bus.aclint.timer_interrupt(now) {
            delay = delay.min(self.bus.aclint.timecmp() - now);
        }
        let supervisor = self.cpu.stimecmp();
        if supervisor != u64::MAX && supervisor > now {
            delay = delay.min(supervisor - now);
        }
        if let Some(rtc_delay) = self
            .bus
            .rtc
            .alarm_remaining_guest_ticks(self.bus.guest_rtc_ns)
            && rtc_delay > 0
        {
            delay = delay.min(rtc_delay);
        }
        (delay != u64::MAX).then_some(delay)
    }

    #[must_use]
    pub fn is_wfi_sleeping(&self) -> bool {
        self.cpu.is_wfi_sleeping()
    }

    pub fn run_cpu(&mut self, cycle_limit_cycles: u32) -> CpuRunResult {
        self.bus.host_service_requested = false;
        self.bus.timer_reprogrammed = false;
        self.sync_interrupts();
        let result = self
            .cpu
            .run_cpu_with_platform(cycle_limit_cycles, &mut self.bus);
        self.sync_interrupts();
        CpuRunResult {
            consumed_cycles: result.consumed_cycles,
            state: match result.reason {
                1 => CpuRunExitReason::WfiSleep,
                2 if self.bus.timer_reprogrammed => CpuRunExitReason::TimerReprogrammed,
                2 => CpuRunExitReason::HostServiceRequested,
                3 => CpuRunExitReason::TimerReprogrammed,
                _ => CpuRunExitReason::CycleLimitReached,
            },
        }
    }

    fn sync_interrupts(&mut self) {
        self.cpu.set_interrupts(self.bus.interrupt_mask());
    }

    pub fn receive_console(&mut self, bytes: &[u8]) -> usize {
        let count = self.bus.uart.receive(bytes);
        self.bus.update_device_irqs();
        count
    }

    #[must_use]
    pub fn receive_space(&self) -> usize {
        self.bus.uart.receive_space()
    }

    pub fn take_console_output(&mut self) -> Vec<u8> {
        self.bus.uart.take_transmitted()
    }

    /// Delivers host input to a `VirtIO` console slot.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot, wrong device type, or malformed queue.
    pub fn virtio_console_receive(
        &mut self,
        slot: usize,
        bytes: &[u8],
    ) -> Result<(), MachineError> {
        let PlatformBus { memory, virtio, .. } = &mut self.bus;
        let VirtioSlot::Console(device) = virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        device
            .device
            .receive(&mut device.transport, memory, bytes)?;
        self.bus.update_device_irqs();
        Ok(())
    }

    /// Takes bytes transmitted by a `VirtIO` console slot.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot or wrong device type.
    pub fn take_virtio_console_output(&mut self, slot: usize) -> Result<Vec<u8>, MachineError> {
        let VirtioSlot::Console(device) = self
            .bus
            .virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        Ok(core::mem::take(&mut device.device.output))
    }

    /// Removes the next HTTP request queued by a block device.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot or wrong device type.
    pub fn next_http_block_request(
        &mut self,
        slot: usize,
    ) -> Result<Option<HttpRequest>, MachineError> {
        let VirtioSlot::StorageBlock(device) = self
            .bus
            .virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        Ok(device.device.backend_mut().next_request())
    }

    /// Supplies one HTTP block response and resumes its pending guest request.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot, response, device request, or queue.
    pub fn complete_http_block_request(
        &mut self,
        slot: usize,
        request: u32,
        data: Result<Vec<u8>, ()>,
        resume_guest: bool,
    ) -> Result<(), MachineError> {
        let PlatformBus { memory, virtio, .. } = &mut self.bus;
        let VirtioSlot::StorageBlock(device) = virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        match device.device.backend_mut().complete(request, data) {
            Ok(()) => {}
            Err(StorageError::UnknownRequest) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
        if resume_guest {
            device.device.resume(&mut device.transport, memory)?;
        }
        self.bus.update_device_irqs();
        Ok(())
    }

    /// Updates a `VirtIO` console's reported dimensions.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot or wrong device type.
    pub fn resize_virtio_console(
        &mut self,
        slot: usize,
        width: u16,
        height: u16,
    ) -> Result<(), MachineError> {
        let VirtioSlot::Console(device) = self
            .bus
            .virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        device.device.resize(&mut device.transport, width, height);
        self.bus.update_device_irqs();
        Ok(())
    }

    /// Delivers one host network packet to a `VirtIO` network slot.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot, wrong device type, or malformed queue.
    pub fn virtio_network_receive(
        &mut self,
        slot: usize,
        packet: Vec<u8>,
    ) -> Result<NetworkIngress, MachineError> {
        let PlatformBus { memory, virtio, .. } = &mut self.bus;
        let VirtioSlot::Network(device) = virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        let ingress = device
            .device
            .receive_packet(&mut device.transport, memory, packet)?;
        self.bus.update_device_irqs();
        Ok(ingress)
    }

    /// Updates the host carrier state of a `VirtIO` network slot.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot or wrong device type.
    pub fn virtio_network_set_carrier(
        &mut self,
        slot: usize,
        up: bool,
    ) -> Result<(), MachineError> {
        let device = self
            .bus
            .virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?;
        let VirtioSlot::Network(device) = device else {
            return Err(MachineError::WrongVirtioDevice);
        };
        device.device.set_carrier(&mut device.transport, up);
        self.bus.update_device_irqs();
        Ok(())
    }

    /// Sends a host key transition to a `VirtIO` keyboard slot.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot, wrong device type, or unavailable queue.
    pub fn virtio_key_event(
        &mut self,
        slot: usize,
        code: u16,
        down: bool,
    ) -> Result<(), MachineError> {
        let PlatformBus { memory, virtio, .. } = &mut self.bus;
        let VirtioSlot::Input(device) = virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        device
            .device
            .send_key(&mut device.transport, memory, code, down)?;
        self.bus.update_device_irqs();
        Ok(())
    }

    /// Sends movement, wheel, and button state to a `VirtIO` pointer slot.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot, wrong device type, or unavailable queue.
    pub fn virtio_pointer_event(
        &mut self,
        slot: usize,
        position: (i32, i32),
        wheel: i32,
        buttons: u32,
    ) -> Result<(), MachineError> {
        let PlatformBus { memory, virtio, .. } = &mut self.bus;
        let VirtioSlot::Input(device) = virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        device
            .device
            .send_pointer(&mut device.transport, memory, position, wheel, buttons)?;
        self.bus.update_device_irqs();
        Ok(())
    }

    #[must_use]
    pub const fn finish_status(&self) -> FinishStatus {
        self.bus.finisher.status()
    }

    /// Reads bytes from guest RAM for host services and validation.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested range is not RAM.
    pub fn read_ram(&mut self, address: u64, len: usize) -> Result<&[u8], MachineError> {
        Ok(self.bus.memory.ram_view(GuestAddress(address), len)?)
    }

    /// Writes bytes into guest RAM for host services and validation.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested range is not writable RAM.
    pub fn write_ram(&mut self, address: u64, bytes: &[u8]) -> Result<(), MachineError> {
        self.bus
            .memory
            .ram_range(GuestAddress(address), bytes.len(), true)?
            .copy_from_slice(bytes);
        Ok(())
    }

    #[must_use]
    pub fn cpu(&self) -> &Core {
        &self.cpu
    }

    #[must_use]
    pub const fn bus(&self) -> &PlatformBus {
        &self.bus
    }

    pub fn bus_mut(&mut self) -> &mut PlatformBus {
        &mut self.bus
    }

    /// Returns and consumes framebuffer dirty-page row spans.
    ///
    /// # Errors
    ///
    /// Returns a memory-map error if the framebuffer region is inconsistent.
    pub fn take_redraw_spans(&mut self) -> Result<Vec<RedrawSpan>, MachineError> {
        let Some(framebuffer) = self.bus.framebuffer.as_ref() else {
            return Ok(Vec::new());
        };
        let snapshot = self.bus.memory.take_dirty_pages(framebuffer.region)?;
        let mut result: Vec<RedrawSpan> = Vec::new();
        for (word_index, word) in snapshot.words.iter().copied().enumerate() {
            for bit in 0..32 {
                if word & (1_u32 << bit) == 0 {
                    continue;
                }
                let page = word_index * 32 + bit;
                let Ok(page) = u64::try_from(page) else {
                    continue;
                };
                let byte = page * 4096;
                if byte >= u64::from(framebuffer.size) {
                    continue;
                }
                let Ok(y) = u32::try_from(byte / u64::from(framebuffer.stride)) else {
                    continue;
                };
                let end_byte = (byte + 4095).min(u64::from(framebuffer.size - 1));
                let Ok(end) = u32::try_from(end_byte / u64::from(framebuffer.stride) + 1) else {
                    continue;
                };
                let end = end.min(framebuffer.height);
                if let Some(last) = result
                    .last_mut()
                    .filter(|last| y <= last.y + last.height + 3)
                {
                    last.height = last.height.max(end - last.y);
                } else {
                    result.push(RedrawSpan { y, height: end - y });
                }
            }
        }
        Ok(result)
    }

    /// Describes the framebuffer bytes covered by a dirty row span.
    ///
    /// # Errors
    ///
    /// Returns a memory-map error if the framebuffer range is inconsistent.
    pub fn framebuffer_update(
        &mut self,
        span: RedrawSpan,
    ) -> Result<Option<FramebufferUpdate>, MachineError> {
        let Some(framebuffer) = self.bus.framebuffer.as_ref() else {
            return Ok(None);
        };
        let byte_offset = span
            .y
            .checked_mul(framebuffer.stride)
            .ok_or(MachineError::InvalidFramebuffer)?;
        let len = span
            .height
            .checked_mul(framebuffer.stride)
            .ok_or(MachineError::InvalidFramebuffer)?;
        let bytes = self.bus.memory.ram_range(
            GuestAddress(FRAMEBUFFER_BASE + u64::from(byte_offset)),
            len as usize,
            false,
        )?;
        #[cfg(target_arch = "wasm32")]
        let arena_offset = ArenaOffset(
            u32::try_from(bytes.as_ptr() as usize).map_err(|_| MachineError::InvalidFramebuffer)?,
        );
        #[cfg(not(target_arch = "wasm32"))]
        let arena_offset = {
            let _ = bytes;
            ArenaOffset(0)
        };
        Ok(Some(FramebufferUpdate {
            arena_offset,
            x: 0,
            y: span.y,
            width: framebuffer.width,
            height: span.height,
            stride: framebuffer.stride,
        }))
    }

    #[must_use]
    pub fn framebuffer_bytes(&self, update: FramebufferUpdate) -> Option<&[u8]> {
        let len = update.height.checked_mul(update.stride)? as usize;
        let offset = update.y.checked_mul(update.stride)?;
        self.bus
            .memory
            .ram_view(GuestAddress(FRAMEBUFFER_BASE + u64::from(offset)), len)
            .ok()
    }
}

fn decode_gzip(
    image: &[u8],
    limit: u64,
    oversized: MachineError,
    invalid: MachineError,
) -> Result<Vec<u8>, MachineError> {
    // Bound decoded output before it can consume more memory than the boot
    // layout permits. Reading one extra byte distinguishes a full region
    // from a kernel that would cross its end.
    let mut decoded = Vec::new();
    MultiGzDecoder::new(image)
        .take(limit.saturating_add(1))
        .read_to_end(&mut decoded)
        .map_err(|_| invalid)?;
    if decoded.len() as u64 > limit {
        return Err(oversized);
    }
    Ok(decoded)
}

fn offset(address: u64, base: u64) -> Result<u32, BusError> {
    u32::try_from(address - base).map_err(|_| BusError::AccessFault)
}

fn callback_width(width: u32) -> Option<AccessWidth> {
    match width {
        1 => Some(AccessWidth::Byte),
        2 => Some(AccessWidth::HalfWord),
        4 => Some(AccessWidth::Word),
        8 => Some(AccessWidth::DoubleWord),
        _ => None,
    }
}

fn virtio_location(address: u64) -> Option<(usize, u32)> {
    let relative = address.checked_sub(VIRTIO_BASE)?;
    let index = usize::try_from(relative / MMIO_SIZE).ok()?;
    let device_offset = u32::try_from(relative % MMIO_SIZE).ok()?;
    virtio_irq_checked(index).map(|_| (index, device_offset))
}

fn virtio_irq(index: usize) -> u8 {
    virtio_irq_checked(index).expect("registered `VirtIO` slot has a PLIC source")
}

fn virtio_irq_checked(index: usize) -> Option<u8> {
    let mut irq = u8::try_from(index).ok()?.checked_add(1)?;
    if irq >= 10 {
        irq = irq.checked_add(1)?;
    }
    if irq >= 11 {
        irq = irq.checked_add(1)?;
    }
    (irq < 32).then_some(irq)
}

fn low_u32(value: u64) -> u32 {
    let bytes = value.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
