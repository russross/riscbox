use riscbox::cpu::{BusError, Cpu, CpuBus, MIP_MTIP};
use riscbox::memory::{AccessWidth, ArenaOffset, GuestAddress, PhysicalMemory, RamFlags};

const CSR_MCAUSE: u16 = 0x342;
const CSR_MEPC: u16 = 0x341;
const CSR_MIE: u16 = 0x304;
const CSR_MSTATUS: u16 = 0x300;
const CSR_MTVAL: u16 = 0x343;

struct UncachedMemory(PhysicalMemory);

struct CountingMemory {
    memory: PhysicalMemory,
    event_polls: u32,
}

impl CpuBus for UncachedMemory {
    fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, BusError> {
        self.0
            .read(address, width)
            .map_err(|_| BusError::AccessFault)
    }

    fn write(
        &mut self,
        address: GuestAddress,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), BusError> {
        self.0
            .write(address, width, value)
            .map_err(|_| BusError::AccessFault)
    }

    fn ram_range(
        &mut self,
        _address: GuestAddress,
        _len: usize,
        _write: bool,
    ) -> Result<ArenaOffset, BusError> {
        Err(BusError::AccessFault)
    }

    fn arena(&self) -> &[u8] {
        self.0.arena()
    }

    fn arena_mut(&mut self) -> &mut [u8] {
        self.0.arena_mut()
    }
}

impl CpuBus for CountingMemory {
    fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, BusError> {
        self.memory
            .read(address, width)
            .map_err(|_| BusError::AccessFault)
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

    fn take_interrupt_state_changed(&mut self) -> bool {
        self.event_polls += 1;
        false
    }
}

fn i(immediate: i32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    (immediate.cast_unsigned() & 0xfff) << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | opcode
}

fn store(rs2: u32, rs1: u32, funct3: u32) -> u32 {
    rs2 << 20 | rs1 << 15 | funct3 << 12 | 0x23
}

fn amo(operation: u32, rs2: u32, rs1: u32, width: u32, rd: u32) -> u32 {
    operation << 27 | rs2 << 20 | rs1 << 15 | width << 12 | rd << 7 | 0x2f
}

fn write_instruction(memory: &mut PhysicalMemory, address: u64, instruction: u32) {
    memory
        .write(
            GuestAddress(address),
            AccessWidth::Word,
            u64::from(instruction),
        )
        .expect("instruction write");
}

fn assert_cpu_equal(fast: &mut Cpu, slow: &mut Cpu) {
    assert_eq!(fast.pc(), slow.pc());
    assert_eq!(fast.elapsed_cycles(), slow.elapsed_cycles());
    assert_eq!(fast.retired_instructions(), slow.retired_instructions());
    assert_eq!(fast.privilege(), slow.privilege());
    for register in 0..32 {
        assert_eq!(fast.register(register), slow.register(register));
    }
    for csr in [CSR_MCAUSE, CSR_MEPC, CSR_MTVAL] {
        assert_eq!(fast.read_csr(csr), slow.read_csr(csr));
    }
}

#[test]
fn tlb_fast_path_matches_checked_bus_execution() {
    let mut fast_memory = PhysicalMemory::new();
    fast_memory
        .register_ram(GuestAddress(0), 0x1_0000, RamFlags::default())
        .expect("test RAM");
    let mut slow_memory = UncachedMemory(fast_memory.clone());
    let mut fast = Cpu::new(0);
    let mut slow = Cpu::new(0);
    for (register, value) in [(1, 7), (2, 0x101), (5, 0x200)] {
        fast.set_register(register, value);
        slow.set_register(register, value);
    }
    fast_memory
        .write(GuestAddress(0x200), AccessWidth::Word, 11)
        .expect("atomic operand");
    slow_memory
        .0
        .write(GuestAddress(0x200), AccessWidth::Word, 11)
        .expect("atomic operand");

    let instructions = [
        i(5, 1, 0, 1, 0x13),
        store(1, 2, 2),
        i(0, 2, 2, 3, 0x03),
        amo(0, 1, 5, 2, 4),
        0xffff_ffff,
    ];
    for instruction in instructions {
        let pc = fast.pc();
        assert_eq!(pc, slow.pc());
        write_instruction(&mut fast_memory, pc, instruction);
        write_instruction(&mut slow_memory.0, pc, instruction);
        assert_eq!(fast.run(&mut fast_memory, 1), slow.run(&mut slow_memory, 1));
        assert_cpu_equal(&mut fast, &mut slow);
    }

    for (address, width) in [(0x101, AccessWidth::Word), (0x200, AccessWidth::Word)] {
        assert_eq!(
            fast_memory.read(GuestAddress(address), width),
            slow_memory.0.read(GuestAddress(address), width)
        );
    }

    fast.set_pc(0x100);
    slow.set_pc(0x100);
    write_instruction(&mut fast_memory, 0x100, i(1, 0, 0, 6, 0x13));
    write_instruction(&mut slow_memory.0, 0x100, i(1, 0, 0, 6, 0x13));
    fast.write_csr(CSR_MIE, u64::from(MIP_MTIP)).unwrap();
    slow.write_csr(CSR_MIE, u64::from(MIP_MTIP)).unwrap();
    fast.write_csr(CSR_MSTATUS, 1 << 3).unwrap();
    slow.write_csr(CSR_MSTATUS, 1 << 3).unwrap();
    fast.set_interrupts(MIP_MTIP);
    slow.set_interrupts(MIP_MTIP);
    assert_eq!(fast.run(&mut fast_memory, 1), slow.run(&mut slow_memory, 1));
    assert_cpu_equal(&mut fast, &mut slow);
}

#[test]
fn sequential_page_chunk_polls_events_once() {
    let mut memory = PhysicalMemory::new();
    memory
        .register_ram(GuestAddress(0), 0x2000, RamFlags::default())
        .expect("test RAM");
    for address in (0x1000..0x1040).step_by(4) {
        write_instruction(&mut memory, address, i(1, 1, 0, 1, 0x13));
    }
    let mut bus = CountingMemory {
        memory,
        event_polls: 0,
    };
    let mut cpu = Cpu::new(0);

    assert_eq!(cpu.run(&mut bus, 16).cycles, 16);
    assert_eq!(cpu.register(1), 16);
    assert_eq!(bus.event_polls, 1);
}

#[test]
fn instruction_fetch_straddles_pages_and_reports_second_page_faults() {
    let instruction = i(7, 0, 0, 1, 0x13);
    let mut memory = PhysicalMemory::new();
    memory
        .register_ram(GuestAddress(0x1000), 0x2000, RamFlags::default())
        .expect("test RAM");
    memory
        .write(
            GuestAddress(0x1ffe),
            AccessWidth::HalfWord,
            u64::from(instruction & 0xffff),
        )
        .expect("low instruction halfword");
    memory
        .write(
            GuestAddress(0x2000),
            AccessWidth::HalfWord,
            u64::from(instruction >> 16),
        )
        .expect("high instruction halfword");
    let mut cpu = Cpu::new(0);
    cpu.set_pc(0x1ffe);

    assert_eq!(cpu.run(&mut memory, 1).cycles, 1);
    assert_eq!(cpu.register(1), 7);
    assert_eq!(cpu.pc(), 0x2002);

    let mut fault_memory = PhysicalMemory::new();
    fault_memory
        .register_ram(GuestAddress(0x1000), 0x1000, RamFlags::default())
        .expect("test RAM");
    fault_memory
        .write(
            GuestAddress(0x1ffe),
            AccessWidth::HalfWord,
            u64::from(instruction & 0xffff),
        )
        .expect("low instruction halfword");
    let mut fault_cpu = Cpu::new(0);
    fault_cpu.set_pc(0x1ffe);

    assert_eq!(fault_cpu.run(&mut fault_memory, 1).cycles, 1);
    assert_eq!(fault_cpu.read_csr(CSR_MEPC), Ok(0x1ffe));
    assert_eq!(fault_cpu.read_csr(CSR_MTVAL), Ok(0x2000));
}

#[test]
fn full_width_instruction_at_page_end_refills_before_the_next_fetch() {
    let mut memory = PhysicalMemory::new();
    memory
        .register_ram(GuestAddress(0x1000), 0x2000, RamFlags::default())
        .expect("test RAM");
    write_instruction(&mut memory, 0x1ffc, i(1, 0, 0, 1, 0x13));
    write_instruction(&mut memory, 0x2000, i(2, 1, 0, 1, 0x13));
    let mut cpu = Cpu::new(0);
    cpu.set_pc(0x1ffc);

    assert_eq!(cpu.run(&mut memory, 2).cycles, 2);
    assert_eq!(cpu.register(1), 3);
    assert_eq!(cpu.pc(), 0x2004);
}
