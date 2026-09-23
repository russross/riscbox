use riscbox::tinyemu_core::Core;

const CODE: u64 = 0x1000;
const CSR_MSTATUS: u32 = 0x300;
const CSR_PMPCFG0: u32 = 0x3a0;
const CSR_PMPADDR0: u32 = 0x3b0;
const CSR_MEPC: u32 = 0x341;
const CSR_MENVCFG: u32 = 0x30a;
const CSR_MIE: u32 = 0x304;
const CSR_STIMECMP: u32 = 0x14d;
const CSR_MTVEC: u32 = 0x305;

fn core() -> Core {
    let mut core = Core::new().expect("TinyEMU core should initialize");
    core.register_ram(0, 0x1_0000, 0)
        .expect("guest RAM should register");
    core
}

fn put(core: &mut Core, address: u64, instructions: &[u32]) {
    let bytes = core
        .ram_range(address, instructions.len() * 4, true)
        .expect("instruction range should be guest RAM");
    for (word, instruction) in bytes.chunks_exact_mut(4).zip(instructions) {
        word.copy_from_slice(&instruction.to_le_bytes());
    }
}

fn addi(rd: u32, rs1: u32, immediate: i32) -> u32 {
    (immediate.cast_unsigned() & 0xfff) << 20 | rs1 << 15 | rd << 7 | 0x13
}

fn r(funct7: u32, rs2: u32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    funct7 << 25 | rs2 << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | opcode
}

fn csrrw(csr: u32, source: u32) -> u32 {
    csr << 20 | source << 15 | 1 << 12 | 0x73
}

fn store(rs2: u32, rs1: u32, funct3: u32, immediate: i32) -> u32 {
    let immediate = immediate.cast_unsigned() & 0xfff;
    (immediate >> 5) << 25 | rs2 << 20 | rs1 << 15 | funct3 << 12 | (immediate & 0x1f) << 7 | 0x23
}

#[test]
fn tinyemu_executes_rv64_integer_and_multiply_divide_instructions() {
    let mut core = core();
    put(
        &mut core,
        CODE,
        &[
            addi(1, 0, -8),
            addi(2, 0, 3),
            r(0, 2, 1, 0, 3, 0x33),
            r(0x20, 2, 1, 0, 4, 0x33),
            r(1, 2, 1, 0, 5, 0x33),
            r(1, 2, 1, 4, 6, 0x33),
            0x1050_0073,
        ],
    );
    assert_eq!(core.run(20).cycles, 7);
    assert_eq!(core.register(1), u64::MAX - 7);
    assert_eq!(core.register(3), u64::MAX - 4);
    assert_eq!(core.register(4), u64::MAX - 10);
    assert_eq!(core.register(5), u64::MAX - 23);
    assert_eq!(core.register(6), u64::MAX - 1);
    assert_eq!(core.register(0), 0);
}

#[test]
fn tinyemu_returns_to_supervisor_then_records_precise_illegal_instruction_trap() {
    let mut core = core();
    put(&mut core, 0x2000, &[0xffff_ffff]);
    // Set mepc to 0x2000 and MPP to supervisor before executing mret.
    put(&mut core, 0x3000, &[0x1050_0073]);
    put(
        &mut core,
        CODE,
        &[
            0x0000_32b7, // lui x5, 3
            csrrw(CSR_MTVEC, 5),
            0x0000_42b7, // lui x5, 4
            csrrw(CSR_PMPADDR0, 5),
            addi(6, 0, 15),
            csrrw(CSR_PMPCFG0, 6),
            0x0000_20b7, // lui x1, 2
            csrrw(CSR_MEPC, 1),
            addi(2, 0, 1),
            slli(2, 11),
            csrrw(CSR_MSTATUS, 2),
            0x3020_0073,
        ],
    );
    core.run(10);
    assert_eq!(core.pc(), 0x2000);
    core.run(1);
    assert_eq!(core.machine_cause(), 2);
    assert_eq!(core.machine_trap_value(), 0xffff_ffff);
}

fn slli(rd: u32, amount: u32) -> u32 {
    amount << 20 | rd << 15 | 1 << 12 | rd << 7 | 0x13
}

#[test]
fn tinyemu_enforces_pmp_on_supervisor_data_access() {
    let mut core = core();
    put(&mut core, 0x3000, &[0x1050_0073]);
    put(
        &mut core,
        CODE,
        &[
            0x0000_32b7, // lui x5, 3
            csrrw(CSR_MTVEC, 5),
            0x0000_42b7, // lui x5, 4
            csrrw(CSR_PMPADDR0, 5),
            addi(6, 0, 13),
            csrrw(CSR_PMPCFG0, 6),
            0x0000_82b7, // lui x5, 8
            addi(2, 0, 7),
            0x0002_11b7, // lui x3, 0x21
            addi(3, 3, -2048),
            csrrw(CSR_MSTATUS, 3),
            store(2, 5, 3, 0),
        ],
    );
    core.run(20);
    assert_eq!(core.machine_cause(), 7);
    assert_eq!(core.machine_trap_value(), 0x8000);
}

#[test]
fn tinyemu_sstc_timer_wakes_waiting_hart() {
    let mut core = core();
    put(
        &mut core,
        CODE,
        &[
            addi(1, 0, 1),
            slli(1, 31),
            slli(1, 32),
            csrrw(CSR_MENVCFG, 1),
            addi(2, 0, 10),
            csrrw(CSR_STIMECMP, 2),
            addi(3, 0, 32),
            csrrw(CSR_MIE, 3),
            addi(4, 0, 8),
            csrrw(CSR_MSTATUS, 4),
            0x1050_0073,
        ],
    );
    core.run(20);
    assert_eq!(core.run(1).waiting, 1);
    core.set_time(10);
    core.run(1);
    assert_eq!(core.machine_cause(), (1_u64 << 63) | 5);
}
