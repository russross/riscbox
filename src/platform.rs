//! Core devices for the QEMU-compatible RISC-V virtual platform.

/// Machine software interrupt pending bit.
pub const MIP_MSIP: u32 = 1 << 3;
/// Machine timer interrupt pending bit.
pub const MIP_MTIP: u32 = 1 << 7;
/// Supervisor timer interrupt pending bit.
pub const MIP_STIP: u32 = 1 << 5;
/// Supervisor software interrupt pending bit.
pub const MIP_SSIP: u32 = 1 << 1;
/// Supervisor external interrupt pending bit.
pub const MIP_SEIP: u32 = 1 << 9;
/// Machine external interrupt pending bit.
pub const MIP_MEIP: u32 = 1 << 11;

pub const TEST_FINISHER_FAIL: u16 = 0x3333;
pub const TEST_FINISHER_PASS: u16 = 0x5555;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FinishStatus {
    #[default]
    Running,
    Passed,
    Failed(u16),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Finisher {
    status: FinishStatus,
}

impl Finisher {
    pub fn write(&mut self, offset: u32, value: u32) {
        if offset != 0 {
            return;
        }
        let bytes = value.to_le_bytes();
        self.status = match u16::from_le_bytes([bytes[0], bytes[1]]) {
            TEST_FINISHER_PASS => FinishStatus::Passed,
            TEST_FINISHER_FAIL => FinishStatus::Failed((value >> 16) as u16),
            _ => self.status,
        };
    }

    #[must_use]
    pub const fn status(self) -> FinishStatus {
        self.status
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Clint {
    msip: bool,
    timecmp: u64,
}

impl Default for Clint {
    fn default() -> Self {
        Self {
            msip: false,
            timecmp: u64::MAX,
        }
    }
}

impl Clint {
    #[must_use]
    pub fn read(&self, offset: u32, now: u64) -> u32 {
        match offset {
            0 => u32::from(self.msip),
            0x4000 => low_u32(self.timecmp),
            0x4004 => (self.timecmp >> 32) as u32,
            0xbff8 => low_u32(now),
            0xbffc => (now >> 32) as u32,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, value: u32) {
        match offset {
            0 => self.msip = value & 1 != 0,
            0x4000 => self.timecmp = (self.timecmp & !u64::from(u32::MAX)) | u64::from(value),
            0x4004 => {
                self.timecmp = (self.timecmp & u64::from(u32::MAX)) | (u64::from(value) << 32);
            }
            _ => {}
        }
    }

    #[must_use]
    pub const fn software_interrupt(self) -> bool {
        self.msip
    }

    #[must_use]
    pub const fn timer_interrupt(self, now: u64) -> bool {
        now >= self.timecmp
    }

    #[must_use]
    pub const fn timecmp(self) -> u64 {
        self.timecmp
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Plic {
    level: u32,
    pending: u32,
    claimed: [u32; 2],
    priority: [u8; 32],
    enable: [u32; 2],
    threshold: [u8; 2],
}

impl Plic {
    fn best_irq(&self, context: usize, check_threshold: bool) -> u32 {
        let candidates = self.pending & self.enable[context];
        let mut best = 0;
        let mut best_priority = 0;
        for irq in 1..32 {
            let irq_number = u32::try_from(irq).expect("PLIC source index fits in u32");
            let priority = self.priority[irq];
            if candidates & (1_u32 << irq_number) != 0
                && priority > best_priority
                && (!check_threshold || priority > self.threshold[context])
            {
                best = irq_number;
                best_priority = priority;
            }
        }
        best
    }

    pub fn set_irq(&mut self, irq: u8, asserted: bool) {
        if !(1..32).contains(&irq) {
            return;
        }
        let mask = 1_u32 << irq;
        if asserted {
            self.level |= mask;
            if self.claimed[0] & mask == 0 && self.claimed[1] & mask == 0 {
                self.pending |= mask;
            }
        } else {
            self.level &= !mask;
        }
    }

    fn claim(&mut self, context: usize) -> u32 {
        let irq = self.best_irq(context, true);
        if irq != 0 {
            let mask = 1_u32 << irq;
            self.pending &= !mask;
            self.claimed[context] |= mask;
        }
        irq
    }

    fn complete(&mut self, context: usize, irq: u32) {
        if !(1..32).contains(&irq) {
            return;
        }
        let mask = 1_u32 << irq;
        if self.claimed[context] & mask == 0 {
            return;
        }
        self.claimed[context] &= !mask;
        if self.level & mask != 0 {
            self.pending |= mask;
        }
    }

    #[must_use]
    pub fn read(&mut self, offset: u32) -> u32 {
        if offset < 128 && offset.is_multiple_of(4) {
            return u32::from(self.priority[(offset / 4) as usize]);
        }
        match offset {
            0x1000 => self.pending,
            0x2000 => self.enable[0],
            0x2080 => self.enable[1],
            0x20_0000 => u32::from(self.threshold[0]),
            0x20_0004 => self.claim(0),
            0x20_1000 => u32::from(self.threshold[1]),
            0x20_1004 => self.claim(1),
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, value: u32) {
        if (4..128).contains(&offset) && offset.is_multiple_of(4) {
            self.priority[(offset / 4) as usize] = (value & 7) as u8;
            return;
        }
        match offset {
            0x2000 => self.enable[0] = value & !1,
            0x2080 => self.enable[1] = value & !1,
            0x20_0000 => self.threshold[0] = (value & 7) as u8,
            0x20_0004 => self.complete(0, value),
            0x20_1000 => self.threshold[1] = (value & 7) as u8,
            0x20_1004 => self.complete(1, value),
            _ => {}
        }
    }

    #[must_use]
    pub fn cpu_interrupts(&self) -> u32 {
        let mut mask = 0;
        if self.best_irq(0, true) != 0 {
            mask |= MIP_MEIP;
        }
        if self.best_irq(1, true) != 0 {
            mask |= MIP_SEIP;
        }
        mask
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Uart16550 {
    dll: u8,
    dlm: u8,
    ier: u8,
    fcr: u8,
    lcr: u8,
    mcr: u8,
    scratch: u8,
    rx: [u8; 16],
    rx_head: u8,
    rx_count: u8,
    overrun: bool,
    transmitter_pending: bool,
    transmitted: Vec<u8>,
}

impl Uart16550 {
    fn capacity(&self) -> u8 {
        if self.fcr & 1 != 0 { 16 } else { 1 }
    }

    fn trigger(&self) -> u8 {
        if self.fcr & 1 == 0 {
            1
        } else {
            [1, 4, 8, 14][usize::from(self.fcr >> 6)]
        }
    }

    fn pending_id(&self) -> u8 {
        if self.ier & 4 != 0 && self.overrun {
            6
        } else if self.ier & 1 != 0 && self.rx_count >= self.trigger() {
            4
        } else if self.ier & 1 != 0 && self.rx_count != 0 {
            0x0c
        } else if self.ier & 2 != 0 && self.transmitter_pending {
            2
        } else {
            1
        }
    }

    #[must_use]
    pub fn irq(&self) -> bool {
        self.pending_id() != 1
    }

    pub fn read(&mut self, offset: u32) -> u8 {
        if offset >= 8 {
            return 0;
        }
        if self.lcr & 0x80 != 0 && offset <= 1 {
            return if offset == 0 { self.dll } else { self.dlm };
        }
        match offset {
            0 => {
                if self.rx_count == 0 {
                    0
                } else {
                    let value = self.rx[usize::from(self.rx_head)];
                    self.rx_head = (self.rx_head + 1) % 16;
                    self.rx_count -= 1;
                    value
                }
            }
            1 => self.ier,
            2 => {
                let id = self.pending_id();
                if id == 2 {
                    self.transmitter_pending = false;
                }
                id | if self.fcr & 1 != 0 { 0xc0 } else { 0 }
            }
            3 => self.lcr,
            4 => self.mcr,
            5 => {
                let value = 0x60 | u8::from(self.rx_count != 0) | (u8::from(self.overrun) << 1);
                self.overrun = false;
                value
            }
            6 if self.mcr & 0x10 == 0 => 0xb0,
            6 => {
                ((self.mcr & 2) << 3)
                    | ((self.mcr & 1) << 5)
                    | ((self.mcr & 4) << 4)
                    | ((self.mcr & 8) << 4)
            }
            7 => self.scratch,
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, value: u8) {
        if offset >= 8 {
            return;
        }
        if self.lcr & 0x80 != 0 && offset <= 1 {
            if offset == 0 {
                self.dll = value;
            } else {
                self.dlm = value;
            }
            return;
        }
        match offset {
            0 => {
                if self.mcr & 0x10 != 0 {
                    self.push(value);
                } else {
                    self.transmitted.push(value);
                }
                self.transmitter_pending = true;
            }
            1 => {
                let old = self.ier;
                self.ier = value & 0x0f;
                if old & 2 == 0 && self.ier & 2 != 0 {
                    self.transmitter_pending = true;
                } else if self.ier & 2 == 0 {
                    self.transmitter_pending = false;
                }
            }
            2 => {
                let new_fcr = value & 0xc9;
                let clear = if (self.fcr ^ new_fcr) & 1 != 0 {
                    value | 6
                } else {
                    value
                };
                self.fcr = new_fcr;
                if clear & 2 != 0 {
                    self.rx_head = 0;
                    self.rx_count = 0;
                    self.overrun = false;
                }
                if clear & 4 != 0 {
                    self.transmitter_pending = true;
                }
            }
            3 => self.lcr = value,
            4 => self.mcr = value & 0x1f,
            7 => self.scratch = value,
            _ => {}
        }
    }

    fn push(&mut self, value: u8) {
        if self.rx_count >= self.capacity() {
            self.overrun = true;
        } else {
            let index = (self.rx_head + self.rx_count) % 16;
            self.rx[usize::from(index)] = value;
            self.rx_count += 1;
        }
    }

    #[must_use]
    pub fn receive_space(&self) -> usize {
        if self.mcr & 0x10 != 0 {
            0
        } else {
            usize::from(self.capacity() - self.rx_count)
        }
    }

    pub fn receive(&mut self, bytes: &[u8]) -> usize {
        let count = bytes.len().min(self.receive_space());
        for &byte in &bytes[..count] {
            self.push(byte);
        }
        count
    }

    pub fn take_transmitted(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.transmitted)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GoldfishRtc {
    time_offset: u64,
    latched_time_high: u32,
    alarm: u64,
    alarm_running: bool,
    irq_pending: bool,
    irq_enabled: bool,
}

impl GoldfishRtc {
    const fn count(&self, host_nanoseconds: u64) -> u64 {
        host_nanoseconds.wrapping_add(self.time_offset)
    }

    fn check_alarm(&mut self, host_nanoseconds: u64) {
        if self.alarm_running && self.alarm <= self.count(host_nanoseconds) {
            self.alarm_running = false;
            self.irq_pending = true;
        }
    }

    #[must_use]
    pub const fn irq(&self) -> bool {
        self.irq_pending && self.irq_enabled
    }

    pub fn read(&mut self, offset: u32, host_nanoseconds: u64) -> u32 {
        match offset {
            0 => {
                let time = self.count(host_nanoseconds);
                self.latched_time_high = (time >> 32) as u32;
                low_u32(time)
            }
            4 => self.latched_time_high,
            8 => low_u32(self.alarm),
            0x0c => (self.alarm >> 32) as u32,
            0x10 => u32::from(self.irq_enabled),
            0x18 => {
                self.check_alarm(host_nanoseconds);
                u32::from(self.alarm_running)
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, offset: u32, value: u32, host_nanoseconds: u64) {
        match offset {
            0 | 4 => {
                let current = self.count(host_nanoseconds);
                let new = if offset == 0 {
                    (current & !u64::from(u32::MAX)) | u64::from(value)
                } else {
                    (current & u64::from(u32::MAX)) | (u64::from(value) << 32)
                };
                self.time_offset = self.time_offset.wrapping_add(new.wrapping_sub(current));
            }
            8 => {
                self.alarm = (self.alarm & !u64::from(u32::MAX)) | u64::from(value);
                self.alarm_running = true;
                self.check_alarm(host_nanoseconds);
            }
            0x0c => self.alarm = (self.alarm & u64::from(u32::MAX)) | (u64::from(value) << 32),
            0x10 => self.irq_enabled = value & 1 != 0,
            0x14 => self.alarm_running = false,
            0x1c => self.irq_pending = false,
            _ => {}
        }
    }

    #[must_use]
    pub fn limit_delay_ms(&mut self, delay_ms: u32, host_nanoseconds: u64) -> u32 {
        self.check_alarm(host_nanoseconds);
        if !self.alarm_running {
            return delay_ms;
        }
        let alarm_delay = self.alarm.saturating_sub(self.count(host_nanoseconds)) / 1_000_000;
        delay_ms.min(u32::try_from(alarm_delay).unwrap_or(u32::MAX))
    }
}

fn low_u32(value: u64) -> u32 {
    let bytes = value.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
