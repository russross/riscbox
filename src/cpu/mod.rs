//! RV64 CPU state, traps, interrupts, counters, and control registers.

mod compressed;
mod execute;
mod floating;
mod mmu;

use crate::memory::{
    AccessWidth, ArenaOffset, GuestAddress, MemoryError, PhysicalMemory, read_u16, read_u32,
};

pub const MIP_SSIP: u32 = 1 << 1;
pub const MIP_MSIP: u32 = 1 << 3;
pub const MIP_STIP: u32 = 1 << 5;
pub const MIP_MTIP: u32 = 1 << 7;
pub const MIP_SEIP: u32 = 1 << 9;
pub const MIP_MEIP: u32 = 1 << 11;

const MSTATUS_SIE: u64 = 1 << 1;
const MSTATUS_MIE: u64 = 1 << 3;
const MSTATUS_SPIE: u64 = 1 << 5;
const MSTATUS_MPIE: u64 = 1 << 7;
const MSTATUS_SPP: u64 = 1 << 8;
const MSTATUS_MPP: u64 = 3 << 11;
const MSTATUS_FS: u64 = 3 << 13;
const MSTATUS_MPRV: u64 = 1 << 17;
const MSTATUS_SUM: u64 = 1 << 18;
const MSTATUS_MXR: u64 = 1 << 19;
const MSTATUS_TVM: u64 = 1 << 20;
const MSTATUS_TW: u64 = 1 << 21;
const MSTATUS_TSR: u64 = 1 << 22;
const MSTATUS_UXL: u64 = 3 << 32;
const MSTATUS_MASK: u64 = MSTATUS_SIE
    | MSTATUS_MIE
    | MSTATUS_SPIE
    | MSTATUS_MPIE
    | MSTATUS_SPP
    | MSTATUS_MPP
    | MSTATUS_FS
    | MSTATUS_MPRV
    | MSTATUS_SUM
    | MSTATUS_MXR
    | MSTATUS_TVM
    | MSTATUS_TW
    | MSTATUS_TSR;
const SSTATUS_MASK: u64 =
    MSTATUS_SIE | MSTATUS_SPIE | MSTATUS_SPP | MSTATUS_FS | MSTATUS_SUM | MSTATUS_MXR;

const MENVCFG_ADUE: u64 = 1 << 61;
const MENVCFG_PBMTE: u64 = 1 << 62;
const MENVCFG_STCE: u64 = 1 << 63;
const ENVCFG_CBIE: u64 = 3 << 4;
const ENVCFG_CBCFE: u64 = 1 << 6;
const ENVCFG_CBZE: u64 = 1 << 7;
const ENVCFG_CBO_MASK: u64 = ENVCFG_CBIE | ENVCFG_CBCFE | ENVCFG_CBZE;
const COUNTEREN_MASK: u32 = 0b111;

const PMP_ENTRY_COUNT: usize = 16;
const PMP_CFG_R: u8 = 1 << 0;
const PMP_CFG_W: u8 = 1 << 1;
const PMP_CFG_X: u8 = 1 << 2;
const PMP_CFG_A_MASK: u8 = 3 << 3;
const PMP_CFG_A_TOR: u8 = 1 << 3;
const PMP_CFG_L: u8 = 1 << 7;
const PMP_ADDR_MASK: u64 = (1 << 54) - 1;

const MISA_I: u64 = 1 << (b'I' - b'A');
const MISA_A: u64 = 1;
const MISA_B: u64 = 1 << (b'B' - b'A');
const MISA_C: u64 = 1 << (b'C' - b'A');
const MISA_D: u64 = 1 << (b'D' - b'A');
const MISA_F: u64 = 1 << (b'F' - b'A');
const MISA_M: u64 = 1 << (b'M' - b'A');
const MISA_S: u64 = 1 << (b'S' - b'A');
const MISA_U: u64 = 1 << (b'U' - b'A');

const CSR_SSTATUS: u16 = 0x100;
const CSR_FFLAGS: u16 = 0x001;
const CSR_FRM: u16 = 0x002;
const CSR_FCSR: u16 = 0x003;
const CSR_SIE: u16 = 0x104;
const CSR_STVEC: u16 = 0x105;
const CSR_SCOUNTEREN: u16 = 0x106;
const CSR_SENVCFG: u16 = 0x10a;
const CSR_SSCRATCH: u16 = 0x140;
const CSR_SEPC: u16 = 0x141;
const CSR_SCAUSE: u16 = 0x142;
const CSR_STVAL: u16 = 0x143;
const CSR_SIP: u16 = 0x144;
const CSR_STIMECMP: u16 = 0x14d;
const CSR_SATP: u16 = 0x180;
const CSR_MSTATUS: u16 = 0x300;
const CSR_MISA: u16 = 0x301;
const CSR_MEDELEG: u16 = 0x302;
const CSR_MIDELEG: u16 = 0x303;
const CSR_MIE: u16 = 0x304;
const CSR_MTVEC: u16 = 0x305;
const CSR_MCOUNTEREN: u16 = 0x306;
const CSR_MENVCFG: u16 = 0x30a;
const CSR_MCOUNTINHIBIT: u16 = 0x320;
const CSR_PMPCFG0: u16 = 0x3a0;
const CSR_PMPCFG2: u16 = 0x3a2;
const CSR_MSCRATCH: u16 = 0x340;
const CSR_MEPC: u16 = 0x341;
const CSR_MCAUSE: u16 = 0x342;
const CSR_MTVAL: u16 = 0x343;
const CSR_MIP: u16 = 0x344;
const CSR_MCYCLE: u16 = 0xb00;
const CSR_MINSTRET: u16 = 0xb02;
const CSR_CYCLE: u16 = 0xc00;
const CSR_TIME: u16 = 0xc01;
const CSR_INSTRET: u16 = 0xc02;

const TLB_SIZE: usize = 256;
const PAGE_SHIFT: u32 = 12;
const PAGE_SIZE: u64 = 1 << PAGE_SHIFT;
const PAGE_SIZE_USIZE: usize = 1 << PAGE_SHIFT;
const PAGE_MASK: u64 = PAGE_SIZE - 1;

fn low_u32(value: u64) -> u32 {
    let bytes = value.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum Privilege {
    User = 0,
    Supervisor = 1,
    Machine = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunState {
    Running,
    Waiting,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunOutcome {
    pub cycles: u32,
    pub state: RunState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusError {
    AccessFault,
}

pub trait CpuBus {
    /// Reads a naturally sized little-endian value.
    ///
    /// # Errors
    ///
    /// Returns [`BusError::AccessFault`] when the range is not readable.
    fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, BusError>;
    /// Writes a naturally sized little-endian value.
    ///
    /// # Errors
    ///
    /// Returns [`BusError::AccessFault`] when the range is not writable.
    fn write(
        &mut self,
        address: GuestAddress,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), BusError>;
    /// Resolves a contiguous RAM range to its arena offset.
    ///
    /// # Errors
    ///
    /// Returns [`BusError::AccessFault`] when the range is not suitable RAM.
    /// A successful full-page lookup must identify bytes present in `arena()`
    /// and remain valid until the current [`Cpu::run`] call returns.
    fn ram_range(
        &mut self,
        address: GuestAddress,
        len: usize,
        write: bool,
    ) -> Result<ArenaOffset, BusError>;
    fn arena(&self) -> &[u8];
    fn arena_mut(&mut self) -> &mut [u8];
    fn take_interrupt_state_changed(&mut self) -> bool {
        false
    }
}

impl CpuBus for PhysicalMemory {
    fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, BusError> {
        Self::read(self, address, width).map_err(|_| BusError::AccessFault)
    }

    fn write(
        &mut self,
        address: GuestAddress,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), BusError> {
        Self::write(self, address, width, value).map_err(|_| BusError::AccessFault)
    }

    fn ram_range(
        &mut self,
        address: GuestAddress,
        len: usize,
        write: bool,
    ) -> Result<ArenaOffset, BusError> {
        Self::ram_range(self, address, len, write).map_err(|_| BusError::AccessFault)
    }

    fn arena(&self) -> &[u8] {
        Self::arena(self)
    }

    fn arena_mut(&mut self) -> &mut [u8] {
        Self::arena_mut(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CsrError {
    IllegalAccess,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Exception {
    InstructionAddressMisaligned = 0,
    InstructionAccessFault = 1,
    IllegalInstruction = 2,
    Breakpoint = 3,
    LoadAddressMisaligned = 4,
    LoadAccessFault = 5,
    StoreAddressMisaligned = 6,
    StoreAccessFault = 7,
    UserEnvironmentCall = 8,
    SupervisorEnvironmentCall = 9,
    MachineEnvironmentCall = 11,
    InstructionPageFault = 12,
    LoadPageFault = 13,
    StorePageFault = 15,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Trap {
    exception: Exception,
    value: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstructionFlow {
    Sequential,
    Exit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InstructionOutcome {
    next_pc: u64,
    retired: bool,
    flow: InstructionFlow,
}

impl InstructionOutcome {
    const fn sequential(next_pc: u64) -> Self {
        Self {
            next_pc,
            retired: true,
            flow: InstructionFlow::Sequential,
        }
    }

    const fn exit(next_pc: u64, retired: bool) -> Self {
        Self {
            next_pc,
            retired,
            flow: InstructionFlow::Exit,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TlbEntry {
    virtual_page: u64,
    arena_page: ArenaOffset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExecuteChunk {
    virtual_page: u64,
    arena_page: ArenaOffset,
    next_offset: u16,
}

impl Default for TlbEntry {
    fn default() -> Self {
        Self {
            virtual_page: u64::MAX,
            arena_page: ArenaOffset(0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Cpu {
    pc: u64,
    registers: [u64; 32],
    fp_registers: [u64; 32],
    fflags: u8,
    frm: u8,
    privilege: Privilege,
    power_down: bool,
    elapsed_cycles: u64,
    cycle: u64,
    instret: u64,
    time: Option<u64>,
    mstatus: u64,
    mtvec: u64,
    mscratch: u64,
    mepc: u64,
    mcause: u64,
    mtval: u64,
    mhartid: u64,
    misa: u64,
    mie: u32,
    mip: u32,
    medeleg: u32,
    mideleg: u32,
    mcounteren: u32,
    menvcfg: u64,
    senvcfg: u64,
    stimecmp: u64,
    pmpcfg: [u8; PMP_ENTRY_COUNT],
    pmpaddr: [u64; PMP_ENTRY_COUNT],
    stvec: u64,
    sscratch: u64,
    sepc: u64,
    scause: u64,
    stval: u64,
    satp: u64,
    scounteren: u32,
    tlb_read: [TlbEntry; TLB_SIZE],
    tlb_write: [TlbEntry; TLB_SIZE],
    tlb_execute: [TlbEntry; TLB_SIZE],
    reservation: Option<(u64, AccessWidth)>,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Cpu {
    #[must_use]
    pub fn new(hart_id: u64) -> Self {
        Self {
            pc: 0x1000,
            registers: [0; 32],
            fp_registers: [u64::MAX; 32],
            fflags: 0,
            frm: 0,
            privilege: Privilege::Machine,
            power_down: false,
            elapsed_cycles: 0,
            cycle: 0,
            instret: 0,
            time: None,
            mstatus: 0,
            mtvec: 0,
            mscratch: 0,
            mepc: 0,
            mcause: 0,
            mtval: 0,
            mhartid: hart_id,
            misa: MISA_A | MISA_B | MISA_C | MISA_D | MISA_F | MISA_I | MISA_M | MISA_S | MISA_U,
            mie: 0,
            mip: 0,
            medeleg: 0,
            mideleg: 0,
            mcounteren: 0,
            menvcfg: 0,
            senvcfg: 0,
            stimecmp: u64::MAX,
            pmpcfg: [0; PMP_ENTRY_COUNT],
            pmpaddr: [0; PMP_ENTRY_COUNT],
            stvec: 0,
            sscratch: 0,
            sepc: 0,
            scause: 0,
            stval: 0,
            satp: 0,
            scounteren: 0,
            tlb_read: [TlbEntry::default(); TLB_SIZE],
            tlb_write: [TlbEntry::default(); TLB_SIZE],
            tlb_execute: [TlbEntry::default(); TLB_SIZE],
            reservation: None,
        }
    }

    fn commit_run_state(&mut self, pc: u64, pending_cycles: &mut u32, pending_retired: &mut u32) {
        self.pc = pc;
        self.elapsed_cycles = self.elapsed_cycles.wrapping_add(u64::from(*pending_cycles));
        self.cycle = self.cycle.wrapping_add(u64::from(*pending_cycles));
        self.instret = self.instret.wrapping_add(u64::from(*pending_retired));
        *pending_cycles = 0;
        *pending_retired = 0;
    }

    fn execute_chunk<B: CpuBus>(
        &mut self,
        bus: &mut B,
        pc: u64,
    ) -> Result<Option<ExecuteChunk>, Trap> {
        if pc & 1 != 0 {
            return Err(Trap {
                exception: Exception::InstructionAddressMisaligned,
                value: pc,
            });
        }
        let virtual_page = pc & !PAGE_MASK;
        let index = ((pc >> PAGE_SHIFT) as usize) & (TLB_SIZE - 1);
        if self.tlb_execute[index].virtual_page != virtual_page {
            self.load(bus, pc, AccessWidth::HalfWord, mmu::Access::Execute)?;
        }
        let entry = self.tlb_execute[index];
        if entry.virtual_page != virtual_page {
            return Ok(None);
        }
        Ok(Some(ExecuteChunk {
            virtual_page,
            arena_page: entry.arena_page,
            next_offset: page_offset(pc),
        }))
    }

    fn fetch_chunk<B: CpuBus>(
        &mut self,
        bus: &mut B,
        chunk: ExecuteChunk,
    ) -> Result<(u32, u64), Trap> {
        let page_offset = usize::from(chunk.next_offset);
        let arena_offset = chunk.arena_page.0 as usize + page_offset;
        if page_offset <= PAGE_SIZE_USIZE - 4 {
            let instruction = read_u32(bus.arena(), arena_offset);
            return Ok(if instruction & 3 == 3 {
                (instruction, 4)
            } else {
                (instruction & 0xffff, 2)
            });
        }
        let low = read_u16(bus.arena(), arena_offset);
        if low & 3 != 3 {
            return Ok((u32::from(low), 2));
        }
        let high = u16::try_from(self.load(
            bus,
            (chunk.virtual_page | u64::from(chunk.next_offset)).wrapping_add(2),
            AccessWidth::HalfWord,
            mmu::Access::Execute,
        )?)
        .expect("a halfword load fits u16");
        Ok((u32::from(low) | u32::from(high) << 16, 4))
    }

    #[must_use]
    pub const fn pc(&self) -> u64 {
        self.pc
    }

    pub fn set_pc(&mut self, pc: u64) {
        self.pc = pc;
        self.flush_tlb();
    }

    #[must_use]
    pub const fn register(&self, index: usize) -> u64 {
        self.registers[index]
    }

    pub fn set_register(&mut self, index: usize, value: u64) {
        if index != 0 {
            self.registers[index] = value;
        }
    }

    #[must_use]
    pub const fn fp_register_bits(&self, index: usize) -> u64 {
        self.fp_registers[index]
    }

    pub fn set_fp_register_bits(&mut self, index: usize, value: u64) {
        self.fp_registers[index] = value;
    }

    fn write_register(&mut self, index: usize, value: u64) {
        self.set_register(index, value);
    }

    #[must_use]
    pub const fn privilege(&self) -> Privilege {
        self.privilege
    }

    #[must_use]
    pub const fn elapsed_cycles(&self) -> u64 {
        self.elapsed_cycles
    }

    #[must_use]
    pub const fn retired_instructions(&self) -> u64 {
        self.instret
    }

    pub fn set_time(&mut self, time: u64) {
        self.time = Some(time);
        if self.menvcfg & MENVCFG_STCE != 0 {
            if time >= self.stimecmp {
                self.mip |= MIP_STIP;
            } else {
                self.mip &= !MIP_STIP;
            }
        }
        if self.mip & self.mie != 0 {
            self.power_down = false;
        }
    }

    pub fn set_interrupts(&mut self, mask: u32) {
        self.mip |= mask;
        if self.mip & self.mie != 0 {
            self.power_down = false;
        }
    }

    pub fn clear_interrupts(&mut self, mask: u32) {
        self.mip &= !mask;
    }

    /// Reads a CSR using the CPU's current privilege.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown CSR, insufficient privilege, or a
    /// counter disabled for the current privilege.
    pub fn read_csr(&mut self, csr: u16) -> Result<u64, CsrError> {
        self.csr_read(csr, false)
    }

    /// Writes a CSR using the CPU's current privilege.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown or read-only CSR or insufficient
    /// privilege.
    pub fn write_csr(&mut self, csr: u16, value: u64) -> Result<(), CsrError> {
        self.csr_read(csr, true)?;
        self.csr_write(csr, value)?;
        Ok(())
    }

    fn fetch<B: CpuBus>(&mut self, bus: &mut B, pc: u64) -> Result<(u32, u64), Trap> {
        if pc & 1 != 0 {
            return Err(Trap {
                exception: Exception::InstructionAddressMisaligned,
                value: pc,
            });
        }
        let low = u32::try_from(self.load(bus, pc, AccessWidth::HalfWord, mmu::Access::Execute)?)
            .expect("a halfword load fits u32");
        if low & 3 != 3 {
            return Ok((low, 2));
        }
        let high = u32::try_from(self.load(
            bus,
            pc.wrapping_add(2),
            AccessWidth::HalfWord,
            mmu::Access::Execute,
        )?)
        .expect("a halfword load fits u32");
        Ok((low | high << 16, 4))
    }

    fn take_exception(&mut self, trap: Trap) {
        self.take_trap(trap.exception as u64, false, trap.value);
    }

    fn take_interrupt(&mut self, interrupt: u32) {
        self.take_trap(u64::from(interrupt), true, 0);
    }

    fn take_trap(&mut self, cause: u64, interrupt: bool, value: u64) {
        let delegated = self.privilege <= Privilege::Supervisor
            && if interrupt {
                self.mideleg & (1 << cause) != 0
            } else {
                self.medeleg & (1 << cause) != 0
            };
        let encoded_cause = cause | if interrupt { 1 << 63 } else { 0 };
        if delegated {
            self.scause = encoded_cause;
            self.sepc = self.pc;
            self.stval = value;
            let old_ie = (self.mstatus >> self.privilege as u8) & 1;
            self.mstatus = (self.mstatus & !MSTATUS_SPIE) | (old_ie << 5);
            self.mstatus = (self.mstatus & !MSTATUS_SPP)
                | if self.privilege == Privilege::Supervisor {
                    MSTATUS_SPP
                } else {
                    0
                };
            self.mstatus &= !MSTATUS_SIE;
            self.set_privilege(Privilege::Supervisor);
            self.pc = self.stvec;
        } else {
            self.mcause = encoded_cause;
            self.mepc = self.pc;
            self.mtval = value;
            let old_ie = (self.mstatus >> self.privilege as u8) & 1;
            self.mstatus = (self.mstatus & !MSTATUS_MPIE) | (old_ie << 7);
            self.mstatus = (self.mstatus & !MSTATUS_MPP) | ((self.privilege as u64) << 11);
            self.mstatus &= !MSTATUS_MIE;
            self.set_privilege(Privilege::Machine);
            self.pc = self.mtvec;
        }
    }

    fn pending_interrupt(&self) -> Option<u32> {
        let pending = self.mip & self.mie;
        let enabled = match self.privilege {
            Privilege::Machine => {
                if self.mstatus & MSTATUS_MIE != 0 {
                    !self.mideleg
                } else {
                    0
                }
            }
            Privilege::Supervisor => {
                let mut enabled = !self.mideleg;
                if self.mstatus & MSTATUS_SIE != 0 {
                    enabled |= self.mideleg;
                }
                enabled
            }
            Privilege::User => u32::MAX,
        };
        let active = pending & enabled;
        [11_u32, 3, 7, 9, 1, 5]
            .into_iter()
            .find(|interrupt| active & (1 << interrupt) != 0)
    }

    fn set_privilege(&mut self, privilege: Privilege) {
        if self.privilege != privilege {
            self.privilege = privilege;
            self.flush_tlb();
        }
    }

    fn flush_tlb(&mut self) {
        self.tlb_read.fill(TlbEntry::default());
        self.tlb_write.fill(TlbEntry::default());
        self.tlb_execute.fill(TlbEntry::default());
    }

    fn get_mstatus(&self, mask: u64) -> u64 {
        let value = self.mstatus | (2_u64 << 32) | (2_u64 << 34);
        (value
            | if value & MSTATUS_FS == MSTATUS_FS {
                1 << 63
            } else {
                0
            })
            & mask
    }

    fn set_mstatus(&mut self, value: u64) {
        let changed = self.mstatus ^ value;
        if changed & (MSTATUS_MPRV | MSTATUS_SUM | MSTATUS_MXR) != 0
            || (self.mstatus & MSTATUS_MPRV != 0 && changed & MSTATUS_MPP != 0)
        {
            self.flush_tlb();
        }
        self.mstatus = (self.mstatus & !MSTATUS_MASK) | (value & MSTATUS_MASK);
    }

    fn counter_enabled(&self, index: u16) -> bool {
        if self.privilege == Privilege::Machine {
            return true;
        }
        let mask = 1_u32 << index;
        self.mcounteren & mask != 0
            && (self.privilege != Privilege::User || self.scounteren & mask != 0)
    }

    fn csr_read(&mut self, csr: u16, will_write: bool) -> Result<u64, CsrError> {
        if csr & 0xc00 == 0xc00 && will_write
            || (self.privilege as u16) < ((csr >> 8) & 3)
            || (csr == CSR_SATP
                && self.privilege == Privilege::Supervisor
                && self.mstatus & MSTATUS_TVM != 0)
        {
            return Err(CsrError::IllegalAccess);
        }
        if csr == CSR_STIMECMP
            && self.privilege != Privilege::Machine
            && (self.menvcfg & MENVCFG_STCE == 0 || self.mcounteren & 2 == 0)
        {
            return Err(CsrError::IllegalAccess);
        }
        let value = match csr {
            CSR_FFLAGS if self.fp_enabled() => u64::from(self.fflags),
            CSR_FRM if self.fp_enabled() => u64::from(self.frm),
            CSR_FCSR if self.fp_enabled() => u64::from(self.fflags | self.frm << 5),
            CSR_CYCLE if self.counter_enabled(0) => self.cycle,
            CSR_TIME if self.counter_enabled(1) => self.time.ok_or(CsrError::IllegalAccess)?,
            CSR_INSTRET if self.counter_enabled(2) => self.instret,
            CSR_SSTATUS => self.get_mstatus(SSTATUS_MASK | MSTATUS_UXL),
            CSR_SIE => u64::from(self.mie & self.mideleg),
            CSR_STVEC => self.stvec,
            CSR_SCOUNTEREN => u64::from(self.scounteren),
            CSR_SENVCFG => self.senvcfg,
            CSR_SSCRATCH => self.sscratch,
            CSR_SEPC => self.sepc,
            CSR_SCAUSE => self.scause,
            CSR_STVAL => self.stval,
            CSR_SIP => u64::from(self.mip & self.mideleg),
            CSR_STIMECMP => self.stimecmp,
            CSR_SATP => self.satp,
            CSR_MSTATUS => self.get_mstatus(u64::MAX),
            CSR_MISA => self.misa | (2_u64 << 62),
            CSR_MEDELEG => u64::from(self.medeleg),
            CSR_MIDELEG => u64::from(self.mideleg),
            CSR_MIE => u64::from(self.mie),
            CSR_MTVEC => self.mtvec,
            CSR_MCOUNTEREN => u64::from(self.mcounteren),
            CSR_MENVCFG => self.menvcfg,
            CSR_MCOUNTINHIBIT => 0,
            CSR_PMPCFG0 => self.pmpcfg_value(0),
            CSR_PMPCFG2 => self.pmpcfg_value(8),
            CSR_MSCRATCH => self.mscratch,
            CSR_MEPC => self.mepc,
            CSR_MCAUSE => self.mcause,
            CSR_MTVAL => self.mtval,
            CSR_MIP => u64::from(self.mip),
            CSR_MCYCLE => self.cycle,
            CSR_MINSTRET => self.instret,
            0x3b0..=0x3bf => self.pmpaddr[usize::from(csr - 0x3b0)],
            0xf11..=0xf13 | 0xf15 => 0,
            0xf14 => self.mhartid,
            _ => return Err(CsrError::IllegalAccess),
        };
        Ok(value)
    }

    fn csr_write(&mut self, csr: u16, value: u64) -> Result<(), CsrError> {
        match csr {
            CSR_FFLAGS => {
                self.require_fp()?;
                self.fflags = u8::try_from(value & 0x1f).expect("masked flags fit u8");
                self.mark_fp_dirty();
            }
            CSR_FRM => {
                self.require_fp()?;
                self.frm = u8::try_from(value & 7).expect("masked rounding mode fits u8");
                self.mark_fp_dirty();
            }
            CSR_FCSR => {
                self.require_fp()?;
                self.fflags = u8::try_from(value & 0x1f).expect("masked flags fit u8");
                self.frm = u8::try_from(value >> 5 & 7).expect("masked rounding mode fits u8");
                self.mark_fp_dirty();
            }
            CSR_SSTATUS => {
                self.set_mstatus((self.mstatus & !SSTATUS_MASK) | (value & SSTATUS_MASK));
            }
            CSR_SIE => {
                self.mie = (self.mie & !self.mideleg) | (low_u32(value) & self.mideleg);
            }
            CSR_STVEC => self.stvec = value & !3,
            CSR_SCOUNTEREN => self.scounteren = low_u32(value) & COUNTEREN_MASK,
            CSR_SENVCFG => self.senvcfg = valid_envcfg(value),
            CSR_SSCRATCH => self.sscratch = value,
            CSR_SEPC => self.sepc = value & !1,
            CSR_SCAUSE => self.scause = value,
            CSR_STVAL => self.stval = value,
            CSR_SIP => {
                let mut mask = self.mideleg;
                if self.menvcfg & MENVCFG_STCE != 0 {
                    mask &= !MIP_STIP;
                }
                self.mip = (self.mip & !mask) | (low_u32(value) & mask);
            }
            CSR_STIMECMP => {
                self.stimecmp = value;
                if let Some(time) = self.time {
                    self.set_time(time);
                }
            }
            CSR_SATP => {
                let current_mode = self.satp >> 60;
                let requested_mode = value >> 60;
                let mode = if requested_mode == 0 || requested_mode == 8 {
                    requested_mode
                } else {
                    current_mode
                };
                self.satp = (value & ((1_u64 << 44) - 1)) | (mode << 60);
                self.flush_tlb();
            }
            CSR_MSTATUS => self.set_mstatus(value),
            CSR_MISA | CSR_MCOUNTINHIBIT => {}
            CSR_MEDELEG => self.medeleg = low_u32(value) & ((1 << 16) - 1),
            CSR_MIDELEG => self.mideleg = low_u32(value) & (MIP_SSIP | MIP_STIP | MIP_SEIP),
            CSR_MIE => {
                let mask = MIP_MSIP | MIP_MTIP | MIP_SSIP | MIP_STIP | MIP_SEIP;
                self.mie = (self.mie & !mask) | (low_u32(value) & mask);
            }
            CSR_MTVEC => self.mtvec = value & !3,
            CSR_MCOUNTEREN => self.mcounteren = low_u32(value) & COUNTEREN_MASK,
            CSR_MENVCFG => {
                let old = self.menvcfg;
                self.menvcfg =
                    valid_envcfg(value) | value & (MENVCFG_ADUE | MENVCFG_PBMTE | MENVCFG_STCE);
                if (old ^ self.menvcfg) & (MENVCFG_ADUE | MENVCFG_PBMTE) != 0 {
                    self.flush_tlb();
                }
                if self.menvcfg & MENVCFG_STCE == 0 {
                    self.mip &= !MIP_STIP;
                } else if let Some(time) = self.time {
                    self.set_time(time);
                }
            }
            CSR_PMPCFG0 => self.set_pmpcfg(0, value),
            CSR_PMPCFG2 => self.set_pmpcfg(8, value),
            CSR_MSCRATCH => self.mscratch = value,
            CSR_MEPC => self.mepc = value & !1,
            CSR_MCAUSE => self.mcause = value,
            CSR_MTVAL => self.mtval = value,
            CSR_MIP => {
                let mut mask = MIP_SSIP | MIP_STIP;
                if self.menvcfg & MENVCFG_STCE != 0 {
                    mask &= !MIP_STIP;
                }
                self.mip = (self.mip & !mask) | (low_u32(value) & mask);
            }
            CSR_MCYCLE => self.cycle = value,
            CSR_MINSTRET => self.instret = value,
            0x3b0..=0x3bf => self.set_pmpaddr(usize::from(csr - 0x3b0), value),
            _ => return Err(CsrError::IllegalAccess),
        }
        Ok(())
    }

    fn pmpcfg_value(&self, first: usize) -> u64 {
        self.pmpcfg[first..first + 8]
            .iter()
            .enumerate()
            .fold(0, |value, (index, byte)| {
                value | (u64::from(*byte) << (index * 8))
            })
    }

    fn set_pmpcfg(&mut self, first: usize, value: u64) {
        let mut changed = false;
        for index in 0..8 {
            let entry = first + index;
            if self.pmpcfg[entry] & PMP_CFG_L != 0 {
                continue;
            }
            let mut config = u8::try_from((value >> (index * 8)) & 0xff)
                .expect("masked PMP configuration fits u8")
                & (PMP_CFG_L | PMP_CFG_A_MASK | PMP_CFG_X | PMP_CFG_W | PMP_CFG_R);
            if config & (PMP_CFG_R | PMP_CFG_W) == PMP_CFG_W {
                config &= !PMP_CFG_W;
            }
            changed |= self.pmpcfg[entry] != config;
            self.pmpcfg[entry] = config;
        }
        if changed {
            self.flush_tlb();
        }
    }

    fn set_pmpaddr(&mut self, entry: usize, value: u64) {
        if self.pmpcfg[entry] & PMP_CFG_L != 0
            || entry + 1 < PMP_ENTRY_COUNT
                && self.pmpcfg[entry + 1] & (PMP_CFG_L | PMP_CFG_A_MASK)
                    == PMP_CFG_L | PMP_CFG_A_TOR
        {
            return;
        }
        let value = value & PMP_ADDR_MASK;
        if self.pmpaddr[entry] != value {
            self.pmpaddr[entry] = value;
            self.flush_tlb();
        }
    }
}

fn page_offset(address: u64) -> u16 {
    let bytes = (address & PAGE_MASK).to_le_bytes();
    u16::from_le_bytes([bytes[0], bytes[1]])
}

impl From<MemoryError> for BusError {
    fn from(_: MemoryError) -> Self {
        Self::AccessFault
    }
}

fn valid_envcfg(value: u64) -> u64 {
    let mut value = value & ENVCFG_CBO_MASK;
    if value & ENVCFG_CBIE == 2 << 4 {
        value &= !ENVCFG_CBIE;
    }
    value
}
