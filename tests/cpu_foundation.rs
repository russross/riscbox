use riscbox::cpu::Cpu;
use riscbox::guest_memory::{AccessWidth, GuestAddress, RamFlags};
use riscbox::memory::PhysicalMemory;

const CSR_MSTATUS: u16 = 0x300;
const CSR_MIDELEG: u16 = 0x303;
const CSR_MIE: u16 = 0x304;
const CSR_MENVCFG: u16 = 0x30a;
const CSR_PMPCFG0: u16 = 0x3a0;
const CSR_PMPADDR0: u16 = 0x3b0;
const CSR_MEPC: u16 = 0x341;
const CSR_MCAUSE: u16 = 0x342;
const CSR_MTVAL: u16 = 0x343;
const CSR_STIMECMP: u16 = 0x14d;
const CSR_SATP: u16 = 0x180;

const MIP_STIP: u64 = 1 << 5;

fn machine() -> (Cpu, PhysicalMemory) {
    let mut memory = PhysicalMemory::new();
    memory
        .register_ram(GuestAddress(0), 0x1_0000, RamFlags::default())
        .unwrap();
    (Cpu::new(0), memory)
}

fn instruction(memory: &mut PhysicalMemory, address: u64, value: u32) {
    memory
        .write(GuestAddress(address), AccessWidth::Word, u64::from(value))
        .unwrap();
}

fn r(funct7: u32, rs2: u32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    funct7 << 25 | rs2 << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | opcode
}

fn i(immediate: i32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    (immediate.cast_unsigned() & 0xfff) << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | opcode
}

fn run_one(cpu: &mut Cpu, memory: &mut PhysicalMemory, value: u32) {
    instruction(memory, cpu.pc(), value);
    assert_eq!(cpu.run(memory, 1).cycles, 1);
}

fn sv39_page_tables(memory: &mut PhysicalMemory, leaf_address: u64, leaf: u64) {
    memory
        .write(GuestAddress(0x4008), AccessWidth::DoubleWord, (5 << 10) | 1)
        .unwrap();
    memory
        .write(GuestAddress(0x5000), AccessWidth::DoubleWord, (6 << 10) | 1)
        .unwrap();
    memory
        .write(GuestAddress(leaf_address), AccessWidth::DoubleWord, leaf)
        .unwrap();
}

#[test]
fn applies_pmp_to_supervisor_data_access() {
    let (mut cpu, mut memory) = machine();
    cpu.write_csr(CSR_PMPADDR0, 0x4000).unwrap();
    cpu.write_csr(CSR_PMPCFG0, 0x0d).unwrap();
    cpu.write_csr(CSR_MEPC, 0x2000).unwrap();
    cpu.write_csr(CSR_MSTATUS, 1 << 11).unwrap();
    run_one(&mut cpu, &mut memory, 0x3020_0073);
    cpu.set_register(1, 0x8000);
    cpu.set_register(2, 7);
    instruction(&mut memory, 0x2000, r(0, 2, 1, 3, 0, 0x23));
    cpu.run(&mut memory, 1);
    assert_eq!(cpu.read_csr(CSR_MCAUSE), Ok(7));
    assert_eq!(cpu.read_csr(CSR_MTVAL), Ok(0x8000));
}

#[test]
fn sv39_updates_accessed_bits_when_svadu_is_enabled() {
    let (mut cpu, mut memory) = machine();
    cpu.write_csr(CSR_PMPADDR0, 0x4000).unwrap();
    cpu.write_csr(CSR_PMPCFG0, 0x0f).unwrap();
    memory
        .write(GuestAddress(0x4008), AccessWidth::DoubleWord, (5 << 10) | 1)
        .unwrap();
    memory
        .write(GuestAddress(0x5000), AccessWidth::DoubleWord, (6 << 10) | 1)
        .unwrap();
    memory
        .write(
            GuestAddress(0x6000),
            AccessWidth::DoubleWord,
            (8 << 10) | 0x07,
        )
        .unwrap();
    memory
        .write(
            GuestAddress(0x8000),
            AccessWidth::DoubleWord,
            0x1122_3344_5566_7788,
        )
        .unwrap();
    cpu.write_csr(CSR_SATP, (8_u64 << 60) | 4).unwrap();
    cpu.write_csr(CSR_MENVCFG, 1_u64 << 61).unwrap();
    cpu.write_csr(CSR_MSTATUS, (1 << 17) | (1 << 11)).unwrap();
    cpu.set_register(1, 0x4000_0000);
    run_one(&mut cpu, &mut memory, i(0, 1, 3, 2, 0x03));
    assert_eq!(cpu.register(2), 0x1122_3344_5566_7788);
    assert_eq!(
        memory.read(GuestAddress(0x6000), AccessWidth::DoubleWord),
        Ok((8 << 10) | 0x47)
    );
}

#[test]
fn sstc_timer_wakes_a_cpu_waiting_for_interrupt() {
    let (mut cpu, mut memory) = machine();
    cpu.write_csr(CSR_MENVCFG, 1_u64 << 63).unwrap();
    cpu.write_csr(CSR_STIMECMP, 10).unwrap();
    cpu.write_csr(CSR_MIDELEG, 0).unwrap();
    cpu.write_csr(CSR_MIE, MIP_STIP).unwrap();
    cpu.write_csr(CSR_MSTATUS, 1 << 3).unwrap();
    run_one(&mut cpu, &mut memory, 0x1050_0073);
    assert_eq!(cpu.run(&mut memory, 1).cycles, 0);

    cpu.set_time(10);
    assert_eq!(cpu.run(&mut memory, 1).cycles, 1);
    assert_eq!(cpu.read_csr(CSR_MCAUSE), Ok((1_u64 << 63) | 5));
}

#[test]
fn sv39_requires_accessed_bits_when_svadu_is_disabled() {
    let (mut cpu, mut memory) = machine();
    cpu.write_csr(CSR_PMPADDR0, 0x4000).unwrap();
    cpu.write_csr(CSR_PMPCFG0, 0x0f).unwrap();
    sv39_page_tables(&mut memory, 0x6000, (8 << 10) | 0x07);
    cpu.write_csr(CSR_SATP, (8_u64 << 60) | 4).unwrap();
    cpu.write_csr(CSR_MSTATUS, (1 << 17) | (1 << 11)).unwrap();
    cpu.set_register(1, 0x4000_0000);
    run_one(&mut cpu, &mut memory, i(0, 1, 3, 2, 0x03));
    assert_eq!(cpu.read_csr(CSR_MCAUSE), Ok(13));
    assert_eq!(cpu.read_csr(CSR_MTVAL), Ok(0x4000_0000));
}

#[test]
fn sv39_enforces_pbmt_enable_and_maps_napot_pages() {
    let (mut cpu, mut memory) = machine();
    cpu.write_csr(CSR_PMPADDR0, 0x4000).unwrap();
    cpu.write_csr(CSR_PMPCFG0, 0x0f).unwrap();
    sv39_page_tables(&mut memory, 0x6000, (8 << 10) | (1_u64 << 61) | 0xc7);
    memory
        .write(
            GuestAddress(0x8000),
            AccessWidth::DoubleWord,
            0x0123_4567_89ab_cdef,
        )
        .unwrap();
    cpu.write_csr(CSR_SATP, (8_u64 << 60) | 4).unwrap();
    cpu.write_csr(CSR_MSTATUS, (1 << 17) | (1 << 11)).unwrap();
    cpu.set_register(1, 0x4000_0000);
    run_one(&mut cpu, &mut memory, i(0, 1, 3, 2, 0x03));
    assert_eq!(cpu.read_csr(CSR_MCAUSE), Ok(13));

    cpu.set_pc(0x1000);
    cpu.write_csr(CSR_MENVCFG, (1_u64 << 62) | (1_u64 << 61))
        .unwrap();
    cpu.write_csr(CSR_MSTATUS, (1 << 17) | (1 << 11)).unwrap();
    run_one(&mut cpu, &mut memory, i(0, 1, 3, 2, 0x03));
    assert_eq!(cpu.register(2), 0x0123_4567_89ab_cdef);

    let (mut cpu, mut memory) = machine();
    cpu.write_csr(CSR_PMPADDR0, 0x4000).unwrap();
    cpu.write_csr(CSR_PMPCFG0, 0x0f).unwrap();
    sv39_page_tables(&mut memory, 0x6018, (8 << 10) | (1_u64 << 63) | 0xc7);
    memory
        .write(
            GuestAddress(0x3000),
            AccessWidth::DoubleWord,
            0xfedc_ba98_7654_3210,
        )
        .unwrap();
    cpu.write_csr(CSR_SATP, (8_u64 << 60) | 4).unwrap();
    cpu.write_csr(CSR_MSTATUS, (1 << 17) | (1 << 11)).unwrap();
    cpu.write_csr(CSR_MENVCFG, 1_u64 << 61).unwrap();
    cpu.set_register(1, 0x4000_3000);
    run_one(&mut cpu, &mut memory, i(0, 1, 3, 2, 0x03));
    assert_eq!(cpu.register(2), 0xfedc_ba98_7654_3210);
}
