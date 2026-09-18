//! Composition and boot loading for the browser RISC-V virtual machine.

use core::fmt;

use crate::cpu::{BusError, Cpu, CpuBus, MIP_MEIP, MIP_MSIP, MIP_MTIP, MIP_SEIP, RunOutcome};
use crate::fdt::{FdtConfig, FramebufferDescription, build as build_fdt};
use crate::memory::{
    AccessWidth, ArenaOffset, GuestAddress, MemoryError, PhysicalMemory, RamFlags, RegionId,
};
use crate::platform::{Clint, FinishStatus, Finisher, GoldfishRtc, Plic, Uart16550};

pub const RAM_BASE: u64 = 0x8000_0000;
pub const RESET_RAM_SIZE: u64 = 0x1_0000;
pub const FINISHER_BASE: u64 = 0x10_0000;
pub const RTC_BASE: u64 = 0x10_1000;
pub const CLINT_BASE: u64 = 0x200_0000;
pub const FRAMEBUFFER_BASE: u64 = 0x410_0000;
pub const PLIC_BASE: u64 = 0xc00_0000;
pub const UART_BASE: u64 = 0x1000_0000;

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
    pub virtio_count: u8,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MachineError {
    Memory(MemoryError),
    InvalidRamSize(u64),
    InvalidFramebuffer,
    FirmwareTooLarge,
    FirmwareOverlapsKernel,
    KernelTooLarge,
    KernelOverlapsInitrd,
    InitrdTooLarge,
    DeviceTreeTooLarge,
}

impl fmt::Display for MachineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(error) => error.fmt(formatter),
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
        }
    }
}

impl std::error::Error for MachineError {}

impl From<MemoryError> for MachineError {
    fn from(value: MemoryError) -> Self {
        Self::Memory(value)
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

#[derive(Clone, Debug)]
pub struct PlatformBus {
    memory: PhysicalMemory,
    clint: Clint,
    plic: Plic,
    uart: Uart16550,
    rtc: GoldfishRtc,
    finisher: Finisher,
    framebuffer: Option<Framebuffer>,
    timer_ticks: u64,
    host_nanoseconds: u64,
}

impl PlatformBus {
    fn new(config: MachineConfig) -> Result<Self, MachineError> {
        if config.ram_size == 0 || !config.ram_size.is_multiple_of(4096) {
            return Err(MachineError::InvalidRamSize(config.ram_size));
        }
        let mut memory = PhysicalMemory::new();
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
        Ok(Self {
            memory,
            clint: Clint::default(),
            plic: Plic::default(),
            uart: Uart16550::default(),
            rtc: GoldfishRtc::default(),
            finisher: Finisher::default(),
            framebuffer,
            timer_ticks: 0,
            host_nanoseconds: 0,
        })
    }

    fn update_device_irqs(&mut self) {
        self.plic.set_irq(10, self.uart.irq());
        self.plic.set_irq(11, self.rtc.irq());
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
            && width == AccessWidth::Word
        {
            u64::from(
                self.clint
                    .read(offset(address, CLINT_BASE)?, self.timer_ticks),
            )
        } else if (PLIC_BASE..PLIC_BASE + 0x400_0000).contains(&address)
            && width == AccessWidth::Word
        {
            u64::from(self.plic.read(offset(address, PLIC_BASE)?))
        } else if (UART_BASE..UART_BASE + 0x100).contains(&address) && width == AccessWidth::Byte {
            u64::from(self.uart.read(offset(address, UART_BASE)?))
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
            && width == AccessWidth::Word
        {
            self.clint.write(offset(address, CLINT_BASE)?, value32);
        } else if (PLIC_BASE..PLIC_BASE + 0x400_0000).contains(&address)
            && width == AccessWidth::Word
        {
            self.plic.write(offset(address, PLIC_BASE)?, value32);
        } else if (UART_BASE..UART_BASE + 0x100).contains(&address) && width == AccessWidth::Byte {
            self.uart
                .write(offset(address, UART_BASE)?, value.to_le_bytes()[0]);
        } else {
            return Err(BusError::AccessFault);
        }
        self.update_device_irqs();
        Ok(())
    }
}

impl CpuBus for PlatformBus {
    fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, BusError> {
        self.memory
            .read(address, width)
            .map_err(|_| BusError::AccessFault)
            .or_else(|_| self.read_mmio(address.0, width))
    }

    fn write(
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

    fn ram_range(
        &mut self,
        address: GuestAddress,
        len: usize,
        write: bool,
    ) -> Result<ArenaOffset, BusError> {
        self.memory
            .ram_range(address, len, write)
            .map_err(|_| BusError::AccessFault)
    }

    fn arena(&self) -> &[u8] {
        self.memory.arena()
    }

    fn arena_mut(&mut self) -> &mut [u8] {
        self.memory.arena_mut()
    }
}

#[derive(Clone, Debug)]
pub struct Machine {
    cpu: Cpu,
    bus: PlatformBus,
    config: MachineConfig,
}

impl Machine {
    /// Creates an unbooted virtual platform.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid RAM or framebuffer sizes or an exhausted memory map.
    pub fn new(config: MachineConfig) -> Result<Self, MachineError> {
        Ok(Self {
            cpu: Cpu::new(0),
            bus: PlatformBus::new(config)?,
            config,
        })
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
        let tree = build_fdt(FdtConfig {
            ram_size: self.config.ram_size,
            command_line: images.command_line,
            initrd,
            virtio_count: self.config.virtio_count,
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
        let arena = self
            .bus
            .memory
            .ram_range(GuestAddress(address), bytes.len(), true)?
            .0 as usize;
        self.bus.memory.arena_mut()[arena..arena + bytes.len()].copy_from_slice(bytes);
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

    pub fn run(&mut self, cycles: u32) -> RunOutcome {
        self.sync_interrupts();
        let outcome = self.cpu.run(&mut self.bus, cycles);
        self.sync_interrupts();
        outcome
    }

    fn sync_interrupts(&mut self) {
        let external = self.bus.plic.cpu_interrupts();
        let mut direct = 0;
        if self.bus.clint.software_interrupt() {
            direct |= MIP_MSIP;
        }
        if self.bus.clint.timer_interrupt(self.bus.timer_ticks) {
            direct |= MIP_MTIP;
        }
        let mask = MIP_MSIP | MIP_MTIP | MIP_MEIP | MIP_SEIP;
        self.cpu.clear_interrupts(mask);
        self.cpu.set_interrupts(direct | external);
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
        let offset = self
            .bus
            .memory
            .ram_range(GuestAddress(address), len, false)?
            .0 as usize;
        Ok(&self.bus.memory.arena()[offset..offset + len])
    }

    #[must_use]
    pub const fn cpu(&self) -> &Cpu {
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
}

fn offset(address: u64, base: u64) -> Result<u32, BusError> {
    u32::try_from(address - base).map_err(|_| BusError::AccessFault)
}

fn low_u32(value: u64) -> u32 {
    let bytes = value.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
