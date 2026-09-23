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

fn write_u64(core: &mut Core, address: u64, value: u64) {
    core.ram_range(address, 8, true)
        .expect("page table range should be guest RAM")
        .copy_from_slice(&value.to_le_bytes());
}

fn read_u64(core: &mut Core, address: u64) -> u64 {
    u64::from_le_bytes(
        core.ram_range(address, 8, false)
            .expect("page table range should be guest RAM")
            .try_into()
            .expect("eight-byte read"),
    )
}

fn sv39_probe(
    adue: bool,
    pbmte: bool,
    leaf_address: u64,
    leaf: u64,
    virtual_address: u32,
    data_address: u64,
    data: u64,
) -> Core {
    const CSR_SATP: u32 = 0x180;
    const CSR_MTVEC: u32 = 0x305;

    let mut core = core();
    write_u64(&mut core, 0x4008, (5 << 10) | 1);
    write_u64(&mut core, 0x5000, (6 << 10) | 1);
    write_u64(&mut core, leaf_address, leaf);
    write_u64(&mut core, data_address, data);
    put(&mut core, 0x9000, &[0x1050_0073]);

    let mut code = vec![
        0x0000_92b7, // lui x5, 9
        csrrw(CSR_MTVEC, 5),
        0x0000_42b7, // lui x5, 4
        csrrw(CSR_PMPADDR0, 5),
        addi(6, 0, 15),
        csrrw(CSR_PMPCFG0, 6),
        addi(3, 0, 8),
        slli(3, 60),
        addi(3, 3, 4),
        csrrw(CSR_SATP, 3),
    ];
    if adue || pbmte {
        code.extend([addi(4, 0, 1), slli(4, 31), slli(4, 30)]);
        if pbmte {
            code.extend([addi(7, 0, 1), slli(7, 31), slli(7, 31), add(4, 4, 7)]);
        }
        code.push(csrrw(CSR_MENVCFG, 4));
    }
    code.extend([
        0x0002_12b7, // lui x5, 0x21
        addi(5, 5, -2048),
        csrrw(CSR_MSTATUS, 5),
        0x4000_00b7, // lui x1, 0x40000
    ]);
    if virtual_address != 0x4000_0000 {
        code.extend([0x0000_33b7, add(1, 1, 7)]); // add a low-page offset
    }
    code.extend([load(2, 1, 3, 0), 0x1050_0073]);
    put(&mut core, CODE, &code);
    core.run(40);
    core
}

fn add(rd: u32, rs1: u32, rs2: u32) -> u32 {
    r(0, rs2, rs1, 0, rd, 0x33)
}

fn load(rd: u32, rs1: u32, funct3: u32, immediate: i32) -> u32 {
    (immediate.cast_unsigned() & 0xfff) << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | 0x03
}

#[test]
fn tinyemu_sv39_updates_accessed_bit_when_svadu_is_enabled() {
    let mut core = sv39_probe(
        true,
        false,
        0x6000,
        (8 << 10) | 0x07,
        0x4000_0000,
        0x8000,
        0x1122_3344_5566_7788,
    );
    assert_eq!(core.register(2), 0x1122_3344_5566_7788);
    assert_eq!(read_u64(&mut core, 0x6000), (8 << 10) | 0x47);
}

#[test]
fn tinyemu_sv39_faults_when_accessed_bit_is_clear_without_svadu() {
    let core = sv39_probe(
        false,
        false,
        0x6000,
        (8 << 10) | 0x07,
        0x4000_0000,
        0x8000,
        0,
    );
    assert_eq!(core.machine_cause(), 13);
    assert_eq!(core.machine_trap_value(), 0x4000_0000);
}

#[test]
fn tinyemu_sv39_requires_pbmt_enable_and_maps_napot_pages() {
    let mut pbmt = sv39_probe(
        false,
        false,
        0x6000,
        (8 << 10) | (1_u64 << 61) | 0xc7,
        0x4000_0000,
        0x8000,
        0x0123_4567_89ab_cdef,
    );
    assert_eq!(pbmt.machine_cause(), 13);

    pbmt = sv39_probe(
        true,
        true,
        0x6000,
        (8 << 10) | (1_u64 << 61) | 0xc7,
        0x4000_0000,
        0x8000,
        0x0123_4567_89ab_cdef,
    );
    assert_eq!(pbmt.register(2), 0x0123_4567_89ab_cdef);

    let napot = sv39_probe(
        true,
        false,
        0x6018,
        (8 << 10) | (1_u64 << 63) | 0xc7,
        0x4000_3000,
        0x3000,
        0xfedc_ba98_7654_3210,
    );
    assert_eq!(napot.register(2), 0xfedc_ba98_7654_3210);
}
