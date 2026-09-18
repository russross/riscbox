use riscbox::cpu::{Cpu, CsrError};
use riscbox::memory::{AccessWidth, GuestAddress, PhysicalMemory, RamFlags};

const CSR_FFLAGS: u16 = 0x001;
const CSR_FRM: u16 = 0x002;
const CSR_FCSR: u16 = 0x003;
const CSR_MSTATUS: u16 = 0x300;
const CSR_MISA: u16 = 0x301;
const CSR_MCAUSE: u16 = 0x342;
const FS_INITIAL: u64 = 1 << 13;

fn machine() -> (Cpu, PhysicalMemory) {
    let mut memory = PhysicalMemory::new();
    memory
        .register_ram(GuestAddress(0), 0x1_0000, RamFlags::default())
        .unwrap();
    let mut cpu = Cpu::new(0);
    cpu.write_csr(CSR_MSTATUS, FS_INITIAL).unwrap();
    (cpu, memory)
}

fn run(cpu: &mut Cpu, memory: &mut PhysicalMemory, instruction: u32) {
    memory
        .write(
            GuestAddress(cpu.pc()),
            AccessWidth::Word,
            instruction.into(),
        )
        .unwrap();
    assert_eq!(cpu.run(memory, 1).cycles, 1);
}

fn run16(cpu: &mut Cpu, memory: &mut PhysicalMemory, instruction: u16) {
    memory
        .write(
            GuestAddress(cpu.pc()),
            AccessWidth::HalfWord,
            instruction.into(),
        )
        .unwrap();
    assert_eq!(cpu.run(memory, 1).cycles, 1);
}

fn fp(funct7: u32, rs2: u32, rs1: u32, rm: u32, rd: u32) -> u32 {
    funct7 << 25 | rs2 << 20 | rs1 << 15 | rm << 12 | rd << 7 | 0x53
}

fn fused(opcode: u32, rs3: u32, format: u32, rs2: u32, rs1: u32, rd: u32) -> u32 {
    rs3 << 27 | format << 25 | rs2 << 20 | rs1 << 15 | rd << 7 | opcode
}

#[test]
fn floating_state_csrs_and_nan_boxing_follow_architecture() {
    let mut disabled = Cpu::new(0);
    assert_eq!(disabled.read_csr(CSR_FCSR), Err(CsrError::IllegalAccess));

    let (mut cpu, mut memory) = machine();
    assert_eq!(
        cpu.read_csr(CSR_MISA).unwrap() & ((1 << 3) | (1 << 5)),
        0x28
    );
    cpu.write_csr(CSR_FCSR, (3 << 5) | 0x1b).unwrap();
    assert_eq!(cpu.read_csr(CSR_FRM), Ok(3));
    assert_eq!(cpu.read_csr(CSR_FFLAGS), Ok(0x1b));
    assert_ne!(cpu.read_csr(CSR_MSTATUS).unwrap() & (1 << 63), 0);

    cpu.set_register(1, u64::from(1.5_f32.to_bits()));
    run(&mut cpu, &mut memory, fp(0x78, 0, 1, 0, 2)); // fmv.w.x
    assert_eq!(cpu.fp_register_bits(2), 0xffff_ffff_3fc0_0000);

    cpu.set_fp_register_bits(3, u64::from(1.0_f32.to_bits()));
    run(&mut cpu, &mut memory, fp(0x00, 3, 2, 0, 4)); // fadd.s, unboxed rhs
    assert_eq!(cpu.fp_register_bits(4), 0xffff_ffff_7fc0_0000);
}

#[test]
fn loads_stores_and_basic_single_and_double_operations() {
    let (mut cpu, mut memory) = machine();
    cpu.set_register(1, 0x8000);
    memory
        .write(GuestAddress(0x8000), AccessWidth::Word, 0x3fc0_0000)
        .unwrap();
    memory
        .write(
            GuestAddress(0x8008),
            AccessWidth::DoubleWord,
            2.25_f64.to_bits(),
        )
        .unwrap();

    run(&mut cpu, &mut memory, 1 << 15 | 2 << 12 | 2 << 7 | 0x07); // flw
    run(
        &mut cpu,
        &mut memory,
        (8 << 20) | 1 << 15 | 3 << 12 | 3 << 7 | 0x07,
    ); // fld
    assert_eq!(cpu.fp_register_bits(2), 0xffff_ffff_3fc0_0000);
    assert_eq!(cpu.fp_register_bits(3), 2.25_f64.to_bits());

    run(&mut cpu, &mut memory, fp(0x00, 2, 2, 0, 4)); // fadd.s
    run(&mut cpu, &mut memory, fp(0x09, 3, 3, 0, 5)); // fmul.d
    assert_eq!(cpu.fp_register_bits(4), 0xffff_ffff_4040_0000);
    assert_eq!(cpu.fp_register_bits(5), 5.0625_f64.to_bits());

    run(
        &mut cpu,
        &mut memory,
        4 << 20 | 1 << 15 | 2 << 12 | 4 << 7 | 0x27,
    ); // fsw 4(x1)
    assert_eq!(
        memory.read(GuestAddress(0x8004), AccessWidth::Word),
        Ok(0x4040_0000)
    );
}

#[test]
fn sign_compare_class_moves_and_conversions_cover_both_formats() {
    let (mut cpu, mut memory) = machine();
    cpu.set_fp_register_bits(1, 0xffff_ffff_3fc0_0000);
    cpu.set_fp_register_bits(2, 0xffff_ffff_c000_0000);
    run(&mut cpu, &mut memory, fp(0x10, 2, 1, 1, 3)); // fsgnjn.s
    assert_eq!(cpu.fp_register_bits(3), 0xffff_ffff_3fc0_0000);
    run(&mut cpu, &mut memory, fp(0x50, 1, 2, 1, 4)); // flt.s
    assert_eq!(cpu.register(4), 1);
    run(&mut cpu, &mut memory, fp(0x70, 0, 2, 1, 5)); // fclass.s
    assert_eq!(cpu.register(5), 1 << 1);

    run(&mut cpu, &mut memory, fp(0x21, 0, 1, 0, 6)); // fcvt.d.s
    assert_eq!(cpu.fp_register_bits(6), 1.5_f64.to_bits());
    run(&mut cpu, &mut memory, fp(0x61, 2, 6, 1, 7)); // fcvt.l.d, rtz
    assert_eq!(cpu.register(7), 1);
    cpu.set_register(8, u64::MAX);
    run(&mut cpu, &mut memory, fp(0x69, 2, 8, 0, 9)); // fcvt.d.l
    assert_eq!(cpu.fp_register_bits(9), (-1.0_f64).to_bits());
    run(&mut cpu, &mut memory, fp(0x71, 0, 9, 0, 10)); // fmv.x.d
    assert_eq!(cpu.register(10), (-1.0_f64).to_bits());
}

#[test]
fn fused_sqrt_min_max_and_compressed_double_memory_execute() {
    let (mut cpu, mut memory) = machine();
    cpu.set_fp_register_bits(1, 2.0_f64.to_bits());
    cpu.set_fp_register_bits(2, 3.0_f64.to_bits());
    cpu.set_fp_register_bits(3, 4.0_f64.to_bits());
    run(&mut cpu, &mut memory, fused(0x43, 3, 1, 2, 1, 4));
    assert_eq!(cpu.fp_register_bits(4), 10.0_f64.to_bits());
    run(&mut cpu, &mut memory, fp(0x2d, 0, 3, 0, 5)); // fsqrt.d
    assert_eq!(cpu.fp_register_bits(5), 2.0_f64.to_bits());
    run(&mut cpu, &mut memory, fp(0x15, 2, 1, 0, 6)); // fmin.d
    run(&mut cpu, &mut memory, fp(0x15, 2, 1, 1, 7)); // fmax.d
    assert_eq!(cpu.fp_register_bits(6), 2.0_f64.to_bits());
    assert_eq!(cpu.fp_register_bits(7), 3.0_f64.to_bits());

    cpu.set_register(8, 0x8000);
    memory
        .write(
            GuestAddress(0x8000),
            AccessWidth::DoubleWord,
            6.5_f64.to_bits(),
        )
        .unwrap();
    run16(&mut cpu, &mut memory, 0x2004); // c.fld f9,0(x8)
    assert_eq!(cpu.fp_register_bits(9), 6.5_f64.to_bits());
    run16(&mut cpu, &mut memory, 0xa024); // c.fsd f9,64(x8)
    assert_eq!(
        memory.read(GuestAddress(0x8040), AccessWidth::DoubleWord),
        Ok(6.5_f64.to_bits())
    );
    cpu.set_register(2, 0x8100);
    memory
        .write(
            GuestAddress(0x8100),
            AccessWidth::DoubleWord,
            7.5_f64.to_bits(),
        )
        .unwrap();
    run16(&mut cpu, &mut memory, 0x2502); // c.fldsp f10,0(sp)
    cpu.set_register(2, 0x8200);
    run16(&mut cpu, &mut memory, 0xa02a); // c.fsdsp f10,0(sp)
    assert_eq!(
        memory.read(GuestAddress(0x8200), AccessWidth::DoubleWord),
        Ok(7.5_f64.to_bits())
    );
}

#[test]
fn disabled_state_and_reserved_dynamic_rounding_trap() {
    let mut memory = PhysicalMemory::new();
    memory
        .register_ram(GuestAddress(0), 0x1_0000, RamFlags::default())
        .unwrap();
    let mut cpu = Cpu::new(0);
    run(&mut cpu, &mut memory, fp(0x00, 2, 1, 0, 3));
    assert_eq!(cpu.read_csr(CSR_MCAUSE), Ok(2));

    cpu.set_pc(0x1000);
    cpu.write_csr(CSR_MSTATUS, FS_INITIAL).unwrap();
    cpu.write_csr(CSR_FRM, 5).unwrap();
    cpu.set_fp_register_bits(1, 0xffff_ffff_3f80_0000);
    cpu.set_fp_register_bits(2, 0xffff_ffff_4000_0000);
    run(&mut cpu, &mut memory, fp(0x00, 2, 1, 7, 3));
    assert_eq!(cpu.read_csr(CSR_MCAUSE), Ok(2));
}
