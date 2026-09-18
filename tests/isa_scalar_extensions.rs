use riscbox::cpu::{Cpu, Privilege};
use riscbox::memory::{AccessWidth, GuestAddress, PhysicalMemory, RamFlags};

const CSR_SENVCFG: u16 = 0x10a;
const CSR_MSTATUS: u16 = 0x300;
const CSR_MENVCFG: u16 = 0x30a;
const CSR_PMPCFG0: u16 = 0x3a0;
const CSR_PMPADDR0: u16 = 0x3b0;
const CSR_MEPC: u16 = 0x341;
const CSR_MCAUSE: u16 = 0x342;

fn machine() -> (Cpu, PhysicalMemory) {
    let mut memory = PhysicalMemory::new();
    memory
        .register_ram(GuestAddress(0), 0x1_0000, RamFlags::default())
        .unwrap();
    (Cpu::new(0), memory)
}

fn r(funct7: u32, rs2: u32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    funct7 << 25 | rs2 << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | opcode
}

fn i_bits(immediate: u32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    immediate << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | opcode
}

fn run_one(cpu: &mut Cpu, memory: &mut PhysicalMemory, instruction: u32) {
    memory
        .write(
            GuestAddress(cpu.pc()),
            AccessWidth::Word,
            u64::from(instruction),
        )
        .unwrap();
    assert_eq!(cpu.run(memory, 1).cycles, 1);
}

#[test]
fn executes_address_and_basic_bit_manipulation() {
    let (mut cpu, mut memory) = machine();
    cpu.set_register(1, 0x8000_0000_0000_00f1);
    cpu.set_register(2, 3);

    let cases = [
        (r(0x10, 2, 1, 2, 3, 0x33), 0x0000_0000_0000_01e5), // sh1add
        (r(0x14, 2, 1, 1, 4, 0x33), 0x8000_0000_0000_00f9), // bset
        (r(0x24, 2, 1, 1, 5, 0x33), 0x8000_0000_0000_00f1), // bclr
        (r(0x34, 2, 1, 1, 6, 0x33), 0x8000_0000_0000_00f9), // binv
        (r(0x24, 2, 1, 5, 7, 0x33), 0),                     // bext
        (r(0x20, 2, 1, 7, 8, 0x33), 0x8000_0000_0000_00f0), // andn
        (r(0x30, 2, 1, 1, 9, 0x33), 0x0000_0000_0000_078c), // rol
        (r(0x05, 2, 1, 4, 10, 0x33), 0x8000_0000_0000_00f1), // min
    ];
    for (instruction, expected) in cases {
        run_one(&mut cpu, &mut memory, instruction);
        assert_eq!(cpu.register(((instruction >> 7) & 0x1f) as usize), expected);
    }

    run_one(&mut cpu, &mut memory, i_bits(0x600, 1, 1, 11, 0x13));
    run_one(&mut cpu, &mut memory, i_bits(0x602, 1, 1, 12, 0x13));
    run_one(&mut cpu, &mut memory, i_bits(0x287, 1, 5, 13, 0x13));
    run_one(&mut cpu, &mut memory, i_bits(0x6b8, 1, 5, 14, 0x13));
    assert_eq!(cpu.register(11), 0);
    assert_eq!(cpu.register(12), 6);
    assert_eq!(cpu.register(13), 0xff00_0000_0000_00ff);
    assert_eq!(cpu.register(14), 0xf100_0000_0000_0080);
}

#[test]
fn executes_word_address_bit_and_conditional_operations() {
    let (mut cpu, mut memory) = machine();
    cpu.set_register(1, 0xffff_ffff_8000_0001);
    cpu.set_register(2, 4);

    run_one(&mut cpu, &mut memory, r(0x04, 2, 1, 0, 3, 0x3b));
    run_one(&mut cpu, &mut memory, r(0x10, 2, 1, 2, 4, 0x3b));
    run_one(&mut cpu, &mut memory, r(0x04, 0, 1, 4, 5, 0x3b));
    run_one(&mut cpu, &mut memory, r(0x30, 2, 1, 1, 6, 0x3b));
    assert_eq!(cpu.register(3), 0x8000_0005);
    assert_eq!(cpu.register(4), 0x1_0000_0006);
    assert_eq!(cpu.register(5), 1);
    assert_eq!(cpu.register(6), 0x18);

    cpu.set_register(7, 0x55aa);
    cpu.set_register(8, 0);
    run_one(&mut cpu, &mut memory, r(0x07, 8, 7, 5, 9, 0x33));
    run_one(&mut cpu, &mut memory, r(0x07, 2, 7, 7, 10, 0x33));
    assert_eq!(cpu.register(9), 0);
    assert_eq!(cpu.register(10), 0);
}

#[test]
fn executes_mops_waits_and_supervisor_invalidation() {
    let (mut cpu, mut memory) = machine();
    cpu.set_register(3, u64::MAX);
    run_one(&mut cpu, &mut memory, 0x81c2_41f3); // mop.r.0 x3,x4
    assert_eq!(cpu.register(3), 0);
    cpu.set_register(3, u64::MAX);
    run_one(&mut cpu, &mut memory, 0xce52_41f3); // mop.rr.7 x3,x4,x5
    assert_eq!(cpu.register(3), 0);

    run_one(&mut cpu, &mut memory, 0x00d0_0073); // wrs.nto
    run_one(&mut cpu, &mut memory, 0x01d0_0073); // wrs.sto
    run_one(&mut cpu, &mut memory, 0x1800_0073); // sfence.w.inval
    run_one(&mut cpu, &mut memory, 0x1810_0073); // sfence.inval.ir
    run_one(&mut cpu, &mut memory, 0x1600_0073); // sinval.vma x0,x0
}

#[test]
fn enforces_cache_block_controls_and_zeroes_complete_blocks() {
    let (mut cpu, mut memory) = machine();
    for address in 0x8040..0x8080 {
        memory
            .write(GuestAddress(address), AccessWidth::Byte, 0xa5)
            .unwrap();
    }
    cpu.set_register(1, 0x8053);
    run_one(&mut cpu, &mut memory, i_bits(4, 1, 2, 0, 0x0f));
    assert!(
        (0x8040..0x8080)
            .all(|address| { memory.read(GuestAddress(address), AccessWidth::Byte) == Ok(0) })
    );

    cpu.write_csr(CSR_MENVCFG, (2 << 4) | (1 << 6) | (1 << 7))
        .unwrap();
    assert_eq!(cpu.read_csr(CSR_MENVCFG).unwrap() & 0xf0, 0xc0);
    cpu.write_csr(CSR_SENVCFG, (2 << 4) | (1 << 7)).unwrap();
    assert_eq!(cpu.read_csr(CSR_SENVCFG), Ok(1 << 7));

    cpu.write_csr(CSR_PMPADDR0, 0x4000).unwrap();
    cpu.write_csr(CSR_PMPCFG0, 0x0f).unwrap();
    cpu.write_csr(CSR_MEPC, cpu.pc()).unwrap();
    cpu.write_csr(CSR_MSTATUS, 1 << 11).unwrap();
    run_one(&mut cpu, &mut memory, 0x3020_0073);
    assert_eq!(cpu.privilege(), Privilege::Supervisor);
    run_one(&mut cpu, &mut memory, i_bits(0, 1, 2, 0, 0x0f));
    assert_eq!(cpu.privilege(), Privilege::Machine);
    assert_eq!(cpu.read_csr(CSR_MCAUSE), Ok(2));
}
