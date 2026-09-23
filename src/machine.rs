//! Composition and boot loading for the browser RISC-V virtual machine.

use core::fmt;

use crate::browser_storage::{HttpBlockStore, HttpRequest, StorageError};
use crate::entropy::{EntropyError, SharedEntropy, SystemEntropy};
use crate::fdt::{FdtConfig, FramebufferDescription, build as build_fdt};
use crate::guest_memory::GuestMemory;
use crate::guest_memory::{
    AccessWidth, ArenaOffset, DeviceWidths, GuestAddress, MemoryAccess, MemoryError, RamFlags,
    RegionId,
};
use crate::platform::{
    Clint, FinishStatus, Finisher, GoldfishRtc, MIP_MSIP, MIP_MTIP, Plic, Uart16550,
};
use crate::tinyemu_core::{BusError, Core, HostCallbacks, RunOutcome, RunState};
use crate::virtio::{MMIO_SIZE, VirtioTransport};
use crate::virtio_devices::{
    BlockBackend, BlockDevice, ConsoleDevice, DeviceError, EntropyDevice, InputDevice, InputKind,
    NetworkBackend, NetworkDevice, NetworkIngress, NinePBackend, NinePDevice, NinePGeneration,
    NinePOutcome, NinePRequestId, NinePTransportAction, VirtioMmioDevice,
};

pub const RAM_BASE: u64 = 0x8000_0000;
pub const RESET_RAM_SIZE: u64 = 0x1_0000;
pub const FINISHER_BASE: u64 = 0x10_0000;
pub const RTC_BASE: u64 = 0x10_1000;
pub const CLINT_BASE: u64 = 0x200_0000;
pub const FRAMEBUFFER_BASE: u64 = 0x410_0000;
pub const PLIC_BASE: u64 = 0xc00_0000;
pub const UART_BASE: u64 = 0x1000_0000;
pub const VIRTIO_BASE: u64 = 0x1000_1000;

const KERNEL_OFFSET: u64 = 0x20_0000;
const FDT_ALIGNMENT: u64 = 0x20_0000;
const FDT_MAX_OFFSET: u64 = 0x4000_0000;

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
    pub firmware: &'a [u8],
    pub kernel: Option<&'a [u8]>,
    pub initrd: Option<&'a [u8]>,
    pub command_line: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootLayout {
    pub firmware_address: u64,
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
    FirmwareOverlapsKernel,
    KernelTooLarge,
    KernelOverlapsInitrd,
    InitrdTooLarge,
    DeviceTreeTooLarge,
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
            Self::FirmwareOverlapsKernel => {
                formatter.write_str("firmware overlaps the kernel load address")
            }
            Self::KernelTooLarge => formatter.write_str("kernel does not fit in RAM"),
            Self::KernelOverlapsInitrd => formatter.write_str("kernel overlaps initrd"),
            Self::InitrdTooLarge => formatter.write_str("initrd does not fit in RAM"),
            Self::DeviceTreeTooLarge => formatter.write_str("device tree does not fit in RAM"),
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
type HttpBlock = VirtioMmioDevice<BlockDevice<HttpBlockStore>>;
type DynNetwork = VirtioMmioDevice<NetworkDevice<Box<dyn NetworkBackend>>>;
type DynNineP = VirtioMmioDevice<NinePDevice<Box<dyn NinePBackend>>>;

enum VirtioSlot {
    Block(DynBlock),
    HttpBlock(HttpBlock),
    Console(VirtioMmioDevice<ConsoleDevice>),
    Network(DynNetwork),
    NineP(DynNineP),
    Input(VirtioMmioDevice<InputDevice>),
    Entropy(VirtioMmioDevice<EntropyDevice>),
}

impl VirtioSlot {
    fn read(&self, offset: u32, width: AccessWidth) -> u32 {
        match self {
            Self::Block(device) => device.read(offset, width),
            Self::HttpBlock(device) => device.read(offset, width),
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
            Self::HttpBlock(device) => device.write(memory, offset, value, width),
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
            Self::HttpBlock(device) => device.transport.irq(),
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
    clint: Clint,
    plic: Plic,
    uart: Uart16550,
    rtc: GoldfishRtc,
    finisher: Finisher,
    framebuffer: Option<Framebuffer>,
    virtio: Vec<VirtioSlot>,
    timer_ticks: u64,
    host_nanoseconds: u64,
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
        memory.register_device(
            GuestAddress(FINISHER_BASE),
            0x1000,
            DeviceWidths::U32,
        )?;
        memory.register_device(GuestAddress(RTC_BASE), 0x1000, DeviceWidths::U32)?;
        memory.register_device(GuestAddress(CLINT_BASE), 0x1_0000, DeviceWidths::U32)?;
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
            clint: Clint::default(),
            plic: Plic::default(),
            uart: Uart16550::default(),
            rtc: GoldfishRtc::default(),
            finisher: Finisher::default(),
            framebuffer,
            virtio: Vec::new(),
            timer_ticks: 0,
            host_nanoseconds: 0,
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
            u64::from(
                self.rtc
                    .read(offset(address, RTC_BASE)?, self.host_nanoseconds),
            )
        } else if (CLINT_BASE..CLINT_BASE + 0x1_0000).contains(&address)
            && matches!(width, AccessWidth::Word | AccessWidth::DoubleWord)
        {
            let device_offset = offset(address, CLINT_BASE)?;
            let low = u64::from(self.clint.read(device_offset, self.timer_ticks));
            if width == AccessWidth::DoubleWord {
                low | (u64::from(self.clint.read(device_offset + 4, self.timer_ticks)) << 32)
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
        } else if (RTC_BASE..RTC_BASE + 0x1000).contains(&address) && width == AccessWidth::Word {
            self.rtc
                .write(offset(address, RTC_BASE)?, value32, self.host_nanoseconds);
        } else if (CLINT_BASE..CLINT_BASE + 0x1_0000).contains(&address)
            && matches!(width, AccessWidth::Word | AccessWidth::DoubleWord)
        {
            let device_offset = offset(address, CLINT_BASE)?;
            self.clint.write(device_offset, value32);
            if width == AccessWidth::DoubleWord {
                self.clint.write(device_offset + 4, (value >> 32) as u32);
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
        if self.clint.software_interrupt() {
            direct |= MIP_MSIP;
        }
        if self.clint.timer_interrupt(self.timer_ticks) {
            direct |= MIP_MTIP;
        }
        direct | external
    }
}

impl HostCallbacks for PlatformBus {
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
}

pub struct Machine {
    bus: PlatformBus,
    cpu: Core,
    config: MachineConfig,
    entropy: SharedEntropy,
}

impl Machine {
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
    pub fn add_http_block_device(
        &mut self,
        backend: HttpBlockStore,
        id: [u8; 20],
    ) -> Result<usize, MachineError> {
        self.add_virtio(VirtioSlot::HttpBlock(VirtioMmioDevice::new(
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
        backend: Box<dyn NinePBackend>,
        tag: &[u8],
    ) -> Result<usize, MachineError> {
        self.add_virtio(VirtioSlot::NineP(VirtioMmioDevice::new(
            VirtioTransport::new(9, 1, &[128]),
            NinePDevice::new(backend, tag),
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
    /// Returns an error when images overlap or do not fit in guest RAM.
    pub fn load_boot(&mut self, images: BootImages<'_>) -> Result<BootLayout, MachineError> {
        let firmware_end =
            u64::try_from(images.firmware.len()).map_err(|_| MachineError::FirmwareTooLarge)?;
        if firmware_end > self.config.ram_size {
            return Err(MachineError::FirmwareTooLarge);
        }
        let kernel_size = images.kernel.map_or(0, |image| image.len() as u64);
        if images.kernel.is_some() && firmware_end > KERNEL_OFFSET {
            return Err(MachineError::FirmwareOverlapsKernel);
        }
        let kernel_end = if images.kernel.is_some() {
            KERNEL_OFFSET
                .checked_add(kernel_size)
                .ok_or(MachineError::KernelTooLarge)?
        } else {
            firmware_end
        };
        if kernel_end > self.config.ram_size {
            return Err(MachineError::KernelTooLarge);
        }
        let initrd_size = images.initrd.map_or(0, |image| image.len() as u64);
        let initrd_offset = self.config.ram_size.div_ceil(2).min(128 << 20);
        if images.initrd.is_some() && kernel_end > initrd_offset {
            return Err(MachineError::KernelOverlapsInitrd);
        }
        let initrd_end = initrd_offset
            .checked_add(initrd_size)
            .ok_or(MachineError::InitrdTooLarge)?;
        if initrd_end > self.config.ram_size {
            return Err(MachineError::InitrdTooLarge);
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
            .map(|_| (RAM_BASE + initrd_offset, initrd_size));
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
        let limit = self.config.ram_size.min(FDT_MAX_OFFSET);
        let tree_len = u64::try_from(tree.len()).map_err(|_| MachineError::DeviceTreeTooLarge)?;
        if tree_len > limit {
            return Err(MachineError::DeviceTreeTooLarge);
        }
        let tree_offset = (limit - tree_len) & !(FDT_ALIGNMENT - 1);
        if tree_offset < kernel_end
            || (images.initrd.is_some()
                && initrd_offset < tree_offset + tree_len
                && tree_offset < initrd_end)
        {
            return Err(MachineError::DeviceTreeTooLarge);
        }
        self.copy_to_ram(RAM_BASE, images.firmware)?;
        if let Some(kernel) = images.kernel {
            self.copy_to_ram(RAM_BASE + KERNEL_OFFSET, kernel)?;
        }
        if let Some(initrd_image) = images.initrd {
            self.copy_to_ram(RAM_BASE + initrd_offset, initrd_image)?;
        }
        self.copy_to_ram(RAM_BASE + tree_offset, &tree)?;
        self.write_reset_vector(RAM_BASE + tree_offset)?;
        Ok(BootLayout {
            firmware_address: RAM_BASE,
            kernel_address: images.kernel.map(|_| RAM_BASE + KERNEL_OFFSET),
            initrd_address: images.initrd.map(|_| RAM_BASE + initrd_offset),
            fdt_address: RAM_BASE + tree_offset,
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

    fn write_reset_vector(&mut self, fdt_address: u64) -> Result<(), MachineError> {
        let mut reset = Vec::with_capacity(40);
        for instruction in [
            0x0000_0297_u32,
            0x0182_b283,
            0x0000_0597,
            0x0185_b583,
            0xf140_2573,
            0x0002_8067,
        ] {
            reset.extend_from_slice(&instruction.to_le_bytes());
        }
        reset.extend_from_slice(&RAM_BASE.to_le_bytes());
        reset.extend_from_slice(&fdt_address.to_le_bytes());
        self.copy_to_ram(0x1000, &reset)
    }

    pub fn update_time(&mut self, timer_ticks: u64, host_nanoseconds: u64) {
        self.bus.timer_ticks = timer_ticks;
        self.bus.host_nanoseconds = host_nanoseconds;
        let _ = self.bus.rtc.limit_delay_ms(u32::MAX, host_nanoseconds);
        self.bus.update_device_irqs();
        self.sync_interrupts();
        self.cpu.set_time(timer_ticks);
    }

    #[must_use]
    pub fn sleep_duration_ms(&mut self, maximum_delay_ms: u32) -> u32 {
        let now = self.bus.timer_ticks;
        let mut delay = self
            .bus
            .rtc
            .limit_delay_ms(maximum_delay_ms, self.bus.host_nanoseconds);
        if !self.bus.clint.timer_interrupt(now) {
            delay = delay.min(timer_delay_ms(self.bus.clint.timecmp(), now));
        }
        delay.min(timer_delay_ms(self.cpu.stimecmp(), now))
    }

    pub fn run(&mut self, cycles: u32) -> RunOutcome {
        self.sync_interrupts();
        let result = self.cpu.run_host(cycles, &mut self.bus);
        self.sync_interrupts();
        RunOutcome {
            cycles: result.cycles,
            state: if result.waiting != 0 {
                RunState::Waiting
            } else {
                RunState::Running
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
        let VirtioSlot::HttpBlock(device) = self
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
        data: Vec<u8>,
    ) -> Result<(), MachineError> {
        let PlatformBus { memory, virtio, .. } = &mut self.bus;
        let VirtioSlot::HttpBlock(device) = virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        device.device.backend_mut().complete(request, data)?;
        device.device.resume(&mut device.transport, memory)?;
        self.bus.update_device_irqs();
        Ok(())
    }

    /// Returns the next generic browser 9p transport action.
    ///
    /// # Errors
    ///
    /// Returns an error when `slot` does not identify a 9p device.
    pub fn next_ninep_transport_action(
        &mut self,
        slot: usize,
    ) -> Result<Option<NinePTransportAction>, MachineError> {
        let VirtioSlot::NineP(device) = self
            .bus
            .virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        Ok(device.device.backend_mut().next_transport_action())
    }

    /// Completes one generic browser 9p transport request.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid slot, generation, request, response, or
    /// guest descriptor.
    pub fn complete_ninep_transport_request(
        &mut self,
        slot: usize,
        generation: NinePGeneration,
        request_id: NinePRequestId,
        outcome: NinePOutcome,
    ) -> Result<(), MachineError> {
        let PlatformBus { memory, virtio, .. } = &mut self.bus;
        let VirtioSlot::NineP(device) = virtio
            .get_mut(slot)
            .ok_or(MachineError::WrongVirtioDevice)?
        else {
            return Err(MachineError::WrongVirtioDevice);
        };
        device.device.complete(
            &mut device.transport,
            memory,
            generation,
            request_id,
            outcome,
        )?;
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
            u32::try_from(bytes.as_ptr() as usize)
                .map_err(|_| MachineError::InvalidFramebuffer)?,
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

fn timer_delay_ms(compare: u64, now: u64) -> u32 {
    let ticks = compare.saturating_sub(now) / 10_000;
    u32::try_from(ticks).unwrap_or(u32::MAX)
}
