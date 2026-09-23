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

fn i(immediate: i32, rs1: u32, funct3: u32, rd: u32, opcode: u32) -> u32 {
    (immediate.cast_unsigned() & 0xfff) << 20 | rs1 << 15 | funct3 << 12 | rd << 7 | opcode
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

fn amo(operation: u32, rs2: u32, rs1: u32, width: u32, rd: u32) -> u32 {
    operation << 27 | rs2 << 20 | rs1 << 15 | width << 12 | rd << 7 | 0x2f
}

#[test]
fn tinyemu_atomic_word_and_doubleword_operations_return_old_values() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 0xffff_fffe);
    put(
        &mut core,
        CODE,
        &[
            0x0000_80b7, // lui x1, 8
            addi(2, 0, 5),
            amo(0, 2, 1, 2, 3),
            addi(2, 0, 9),
            amo(1, 2, 1, 3, 4),
            0x1050_0073,
        ],
    );
    core.run(20);
    assert_eq!(core.register(3), u64::MAX - 1);
    assert_eq!(core.register(4), 3);
    assert_eq!(read_u64(&mut core, 0x8000), 9);
}

#[test]
fn tinyemu_lr_sc_tracks_reservation_address_and_width() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 0x8000_0000);
    put(
        &mut core,
        CODE,
        &[
            0x0000_80b7, // lui x1, 8
            0x1234_5137, // lui x2, 0x12345
            addi(2, 2, 0x678),
            amo(2, 0, 1, 2, 3),
            amo(3, 2, 1, 2, 4),
            amo(3, 2, 1, 2, 5),
            0x1050_0073,
        ],
    );
    core.run(20);
    assert_eq!(core.register(3), 0xffff_ffff_8000_0000);
    assert_eq!(core.register(4), 0);
    assert_eq!(core.register(5), 1);
    assert_eq!(read_u64(&mut core, 0x8000), 0x1234_5678);
}

#[test]
fn tinyemu_compressed_reserved_encoding_traps() {
    let mut core = core();
    put(&mut core, 0x9000, &[0x1050_0073]);
    put(&mut core, CODE, &[0x0000_92b7, csrrw(CSR_MTVEC, 5)]);
    core.ram_range(CODE + 8, 2, true)
        .expect("compressed instruction range should be guest RAM")
        .copy_from_slice(&0_u16.to_le_bytes());
    core.run(10);
    assert_eq!(core.machine_cause(), 2);
    assert_eq!(core.machine_trap_value(), 0);
}

fn put_mixed(core: &mut Core, address: u64, pieces: &[&[u8]]) {
    let mut bytes = Vec::new();
    for piece in pieces {
        bytes.extend_from_slice(piece);
    }
    core.ram_range(address, bytes.len(), true)
        .expect("instruction stream should be guest RAM")
        .copy_from_slice(&bytes);
}

fn word(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

fn halfword(value: u16) -> [u8; 2] {
    value.to_le_bytes()
}

fn c_li(rd: u16, immediate: u16) -> u16 {
    0x4001 | rd << 7 | (immediate & 0x1f) << 2 | (immediate & 0x20) << 7
}

#[test]
fn tinyemu_compressed_control_and_stack_memory_use_two_byte_steps() {
    let mut core = core();
    put_mixed(
        &mut core,
        CODE,
        &[
            &word(0x0000_8137), // lui x2, 8
            &halfword(c_li(1, 31)),
            &halfword(0x0085),  // c.addi x1, 1
            &word(0x8123_41b7), // lui x3, 0x81234
            &word(addi(3, 3, 0x567)),
            &halfword(0xc00e),  // c.swsp x3, 0(sp)
            &halfword(0x4202),  // c.lwsp x4, 0(sp)
            &word(0x0000_22b7), // lui x5, 2
            &word(addi(5, 5, 2)),
            &halfword(0x8282), // c.jr x5
        ],
    );
    put_mixed(&mut core, 0x2002, &[&word(0x1050_0073)]);
    core.run(40);
    assert_eq!(core.register(1), 32);
    assert_eq!(core.register(4), 0xffff_ffff_8123_4567);
    assert_eq!(core.pc(), 0x2006);
}

#[test]
fn tinyemu_zcb_unary_multiply_and_byte_memory_operations_execute() {
    let mut core = core();
    put(&mut core, 0x9000, &[0x1050_0073]);
    put_mixed(
        &mut core,
        CODE,
        &[
            &word(0x0000_9637), // lui x12, 9
            &word(csrrw(CSR_MTVEC, 12)),
            &word(addi(8, 0, -128)),
            &halfword(0x9c61), // c.zext.b x8
            &word(addi(9, 0, 3)),
            &halfword(0x9c45),  // c.mul x8, x9
            &word(0x0000_8537), // lui x10, 8
            &word(addi(11, 0, 0xab)),
            &halfword(0x890c), // c.sb x11, 0(x10)
            &halfword(0x8100), // c.lbu x8, 0(x10)
            &halfword(0x9002), // c.ebreak
        ],
    );
    core.run(30);
    assert_eq!(core.register(8), 0xab);
    assert_eq!(core.register(10), 0x8000);
    assert_eq!(core.machine_cause(), 3);
    assert_eq!(core.machine_trap_value(), 0);
    assert_eq!(
        core.ram_range(0x8000, 1, false)
            .expect("byte target should be guest RAM")[0],
        0xab
    );
}

#[test]
fn tinyemu_compressed_mops_and_lui_hints_preserve_state() {
    for instruction in (0x6081_u16..=0x6781).step_by(0x100) {
        let mut core = core();
        put_mixed(
            &mut core,
            CODE,
            &[
                &word(0x0000_1237), // lui x4, 1
                &word(addi(4, 4, 0x234)),
                &halfword(instruction),
                &word(0x1050_0073),
            ],
        );
        core.run(20);
        assert_eq!(core.register(4), 0x1234);
    }

    let mut core = core();
    put_mixed(&mut core, CODE, &[&halfword(0x6005), &word(0x1050_0073)]);
    core.run(10);
    assert_eq!(core.pc(), CODE + 6);
}

fn csrrs(csr: u32, source: u32, destination: u32) -> u32 {
    csr << 20 | source << 15 | 2 << 12 | destination << 7 | 0x73
}

#[test]
fn tinyemu_advertises_b_and_executes_address_and_bit_manipulation() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 0x8000_0000_0000_00f1);
    put(
        &mut core,
        CODE,
        &[
            0x0000_80b7, // lui x1, 8
            load(1, 1, 3, 0),
            addi(2, 0, 3),
            r(0x10, 2, 1, 2, 3, 0x33),  // sh1add
            r(0x14, 2, 1, 1, 4, 0x33),  // bset
            r(0x24, 2, 1, 1, 5, 0x33),  // bclr
            r(0x34, 2, 1, 1, 6, 0x33),  // binv
            r(0x24, 2, 1, 5, 7, 0x33),  // bext
            r(0x20, 2, 1, 7, 8, 0x33),  // andn
            r(0x30, 2, 1, 1, 9, 0x33),  // rol
            r(0x05, 2, 1, 4, 10, 0x33), // min
            csrrs(0x301, 0, 20),        // read misa
            0x1050_0073,
        ],
    );
    core.run(30);
    assert_eq!(core.register(3), 0x1e5);
    assert_eq!(core.register(4), 0x8000_0000_0000_00f9);
    assert_eq!(core.register(5), 0x8000_0000_0000_00f1);
    assert_eq!(core.register(6), 0x8000_0000_0000_00f9);
    assert_eq!(core.register(7), 0);
    assert_eq!(core.register(8), 0x8000_0000_0000_00f0);
    assert_eq!(core.register(9), 0x78c);
    assert_eq!(core.register(10), 0x8000_0000_0000_00f1);
    assert_ne!(core.register(20) & (1 << (b'B' - b'A')), 0);
}

#[test]
fn tinyemu_executes_word_address_bit_and_conditional_operations() {
    let mut core = core();
    put(
        &mut core,
        CODE,
        &[
            0x8000_00b7, // lui x1, 0x80000
            addi(1, 1, 1),
            addi(2, 0, 4),
            r(0x04, 2, 1, 0, 3, 0x3b),
            r(0x10, 2, 1, 2, 4, 0x3b),
            r(0x04, 0, 1, 4, 5, 0x3b),
            r(0x30, 2, 1, 1, 6, 0x3b),
            addi(7, 0, 0x55),
            r(0x07, 0, 7, 5, 9, 0x33),
            r(0x07, 2, 7, 7, 10, 0x33),
            0x1050_0073,
        ],
    );
    core.run(30);
    assert_eq!(core.register(3), 0x8000_0005);
    assert_eq!(core.register(4), 0x1_0000_0006);
    assert_eq!(core.register(5), 1);
    assert_eq!(core.register(6), 0x18);
    assert_eq!(core.register(9), 0);
    assert_eq!(core.register(10), 0);
}

#[test]
fn tinyemu_executes_mops_waits_and_supervisor_invalidation() {
    let mut running = core();
    put(&mut running, 0x9000, &[0x1050_0073]);
    put(
        &mut running,
        CODE,
        &[
            0x0000_01b7, // lui x3, 0
            addi(3, 0, -1),
            0x81c2_41f3, // mop.r.0 x3,x4
            addi(3, 0, -1),
            0xce52_41f3, // mop.rr.7 x3,x4,x5
            0x00d0_0073, // wrs.nto
            0x01d0_0073, // wrs.sto
            0x1800_0073, // sfence.w.inval
            0x1810_0073, // sfence.inval.ir
            0x1600_0073, // sinval.vma x0,x0
            0x9002_0000, // reserved instruction traps after tested sequence
        ],
    );
    running.run(30);
    assert_eq!(running.register(3), 0);
    assert_eq!(running.machine_cause(), 2);

    let mut invalid = core();
    put(&mut invalid, 0x9000, &[0x1050_0073]);
    put(
        &mut invalid,
        CODE,
        &[0x0000_92b7, csrrw(CSR_MTVEC, 5), 0x8000_41f3],
    );
    invalid.run(10);
    assert_eq!(invalid.machine_cause(), 2);
    assert_eq!(invalid.machine_trap_value(), 0x8000_41f3);
}

#[test]
fn tinyemu_csr_read_set_traps_when_source_register_is_nonzero() {
    let mut core = core();
    put(&mut core, 0x9000, &[0x1050_0073]);
    put(
        &mut core,
        CODE,
        &[
            0x0000_92b7, // lui x5, 9
            csrrw(CSR_MTVEC, 5),
            addi(4, 0, 0),
            0xf112_21f3, // csrrs x3, mvendorid, x4
        ],
    );
    core.run(10);
    assert_eq!(core.machine_cause(), 2);
}

#[test]
fn tinyemu_enforces_cache_block_controls_and_zeroes_whole_blocks() {
    let mut core = core();
    core.ram_range(0x8040, 64, true)
        .expect("cache block should be guest RAM")
        .fill(0xa5);
    put(
        &mut core,
        CODE,
        &[
            0x0000_92b7, // lui x5, 9
            csrrw(CSR_MTVEC, 5),
            addi(1, 0, 8),
            slli(1, 12), // x1 = 0x8000
            addi(1, 1, 0x53),
            addi(3, 0, 14),
            slli(3, 4),
            csrrw(CSR_MENVCFG, 3),
            csrrs(CSR_MENVCFG, 0, 4),
            addi(5, 0, 10),
            slli(5, 4),
            csrrw(0x10a, 5),
            csrrs(0x10a, 0, 6),
            i(4, 1, 2, 0, 0x0f), // cbo.zero
            0x9002_0000,
        ],
    );
    core.run(30);
    assert_eq!(core.register(4) & 0xf0, 0xc0);
    assert_eq!(core.register(6), 1 << 7);
    assert!(
        core.ram_range(0x8040, 64, false)
            .expect("cache block should remain guest RAM")
            .iter()
            .all(|byte| *byte == 0)
    );
}

#[test]
fn tinyemu_checks_supervisor_cache_block_permission() {
    let mut core = core();
    put(&mut core, 0x9000, &[0x1050_0073]);
    put(&mut core, 0x2000, &[i(4, 1, 2, 0, 0x0f), 0x9002_0000]);
    put(
        &mut core,
        CODE,
        &[
            0x0000_92b7, // lui x5, 9
            csrrw(CSR_MTVEC, 5),
            0x0000_42b7, // lui x5, 4
            csrrw(CSR_PMPADDR0, 5),
            addi(6, 0, 15),
            csrrw(CSR_PMPCFG0, 6),
            addi(1, 0, 8),
            slli(1, 12),
            addi(1, 1, 0x53),
            addi(3, 0, 14),
            slli(3, 4),
            csrrw(CSR_MENVCFG, 3),
            addi(4, 0, 10),
            slli(4, 4),
            csrrw(0x10a, 4),
            0x0000_22b7, // lui x5, 2
            csrrw(CSR_MEPC, 5),
            addi(6, 0, 1),
            slli(6, 11),
            csrrw(CSR_MSTATUS, 6),
            0x3020_0073,
        ],
    );
    core.run(40);
    assert_eq!(core.machine_cause(), 2);
}

fn fp(funct7: u32, rs2: u32, rs1: u32, rm: u32, rd: u32) -> u32 {
    funct7 << 25 | rs2 << 20 | rs1 << 15 | rm << 12 | rd << 7 | 0x53
}

fn fs(funct7: u32, rs3: u32, rs2: u32, rs1: u32, rd: u32, opcode: u32) -> u32 {
    rs3 << 27 | funct7 << 25 | rs2 << 20 | rs1 << 15 | rd << 7 | opcode
}

fn enable_fp(code: &mut Vec<u32>) {
    code.extend([addi(5, 0, 3), slli(5, 13), csrrw(CSR_MSTATUS, 5)]);
}

fn fsw(source: u32, base: u32, offset: i32) -> u32 {
    let immediate = offset.cast_unsigned() & 0xfff;
    (immediate >> 5) << 25 | source << 20 | base << 15 | 2 << 12 | (immediate & 0x1f) << 7 | 0x27
}

fn fsd(source: u32, base: u32, offset: i32) -> u32 {
    let immediate = offset.cast_unsigned() & 0xfff;
    (immediate >> 5) << 25 | source << 20 | base << 15 | 3 << 12 | (immediate & 0x1f) << 7 | 0x27
}

#[test]
fn tinyemu_loads_stores_and_runs_basic_single_and_double_fp_operations() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 1.5_f32.to_bits().into());
    write_u64(&mut core, 0x8008, 2.25_f64.to_bits());
    let mut code = vec![0x0000_80b7]; // lui x1, 8
    enable_fp(&mut code);
    code.extend([
        i(0, 1, 2, 2, 0x07),  // flw f2,0(x1)
        i(8, 1, 3, 3, 0x07),  // fld f3,8(x1)
        fp(0x00, 2, 2, 0, 4), // fadd.s f4,f2,f2
        fp(0x09, 3, 3, 0, 5), // fmul.d f5,f3,f3
        fsw(4, 1, 16),
        fsd(5, 1, 24),
        0x1050_0073,
    ]);
    put(&mut core, CODE, &code);
    core.run(30);
    assert_eq!(read_u64(&mut core, 0x8010) as u32, 3.0_f32.to_bits());
    assert_eq!(read_u64(&mut core, 0x8018), 5.0625_f64.to_bits());
}

#[test]
fn tinyemu_fp_csrs_nan_boxing_and_extension_bits_follow_architecture() {
    let mut core = core();
    let mut code = Vec::new();
    enable_fp(&mut code);
    code.extend([
        csrrs(0x301, 0, 20),
        addi(6, 0, 0x7b),
        csrrw(0x003, 6),
        csrrs(0x002, 0, 7),
        csrrs(0x001, 0, 8),
        0x3f80_00b7,          // lui x1, 0x3f800
        fp(0x78, 0, 1, 0, 2), // fmv.w.x f2,x1
        fp(0x79, 0, 1, 0, 3), // fmv.d.x f3,x1, leaving f3 unboxed
        fp(0x00, 3, 2, 0, 4), // fadd.s f4,f2,f3
        0x0000_8537,          // lui x10,8
        fsw(4, 10, 0),
        0x1050_0073,
    ]);
    put(&mut core, CODE, &code);
    core.run(30);
    assert_eq!(core.register(20) & ((1 << 3) | (1 << 5)), 0x28);
    assert_eq!(core.register(7), 3);
    assert_eq!(core.register(8), 0x1b);
    assert_eq!(read_u64(&mut core, 0x8000) as u32, 0x7fc0_0000);
}

#[test]
fn tinyemu_sign_compare_class_and_conversion_instructions_cover_both_formats() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 1.5_f32.to_bits().into());
    write_u64(&mut core, 0x8004, (-2.0_f32).to_bits().into());
    let mut code = vec![0x0000_80b7];
    enable_fp(&mut code);
    code.extend([
        i(0, 1, 2, 1, 0x07),
        i(4, 1, 2, 2, 0x07),
        fp(0x10, 2, 1, 1, 3), // fsgnjn.s
        fp(0x50, 1, 2, 1, 4), // flt.s
        fp(0x70, 0, 2, 1, 5), // fclass.s
        fp(0x21, 0, 1, 0, 6), // fcvt.d.s
        fp(0x61, 2, 6, 1, 7), // fcvt.l.d, rtz
        0x8000_0437,          // lui x8, 0x80000 -> -2147483648 (not -1)
        addi(8, 0, -1),
        fp(0x69, 2, 8, 0, 9),  // fcvt.d.l
        fp(0x71, 0, 9, 0, 10), // fmv.x.d
        fsw(3, 1, 16),
        fsd(6, 1, 24),
        0x00e1_3023, // sd x14,0(x2), placeholder replaced below
        0x1050_0073,
    ]);
    // Capture integer comparison/class/conversion results as guest memory.
    *code.last_mut().expect("store placeholder") = store(4, 1, 2, 32);
    put(&mut core, CODE, &code);
    core.run(40);
    assert_eq!(read_u64(&mut core, 0x8010) as u32, 1.5_f32.to_bits());
    assert_eq!(read_u64(&mut core, 0x8018), 1.5_f64.to_bits());
    assert_eq!(core.register(4), 1);
    assert_eq!(core.register(5), 1 << 1);
    assert_eq!(core.register(7), 1);
    assert_eq!(core.register(10), (-1.0_f64).to_bits());
}

#[test]
fn tinyemu_fused_sqrt_min_max_and_compressed_double_memory_execute() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 2.0_f64.to_bits());
    write_u64(&mut core, 0x8008, 3.0_f64.to_bits());
    write_u64(&mut core, 0x8010, 4.0_f64.to_bits());
    write_u64(&mut core, 0x8100, 7.5_f64.to_bits());
    let mut code = vec![0x0000_80b7];
    enable_fp(&mut code);
    code.extend([
        i(0, 1, 3, 1, 0x07),
        i(8, 1, 3, 2, 0x07),
        i(16, 1, 3, 3, 0x07),
        fs(1, 3, 2, 1, 4, 0x43), // fmadd.d
        fp(0x2d, 0, 3, 0, 5),    // fsqrt.d
        fp(0x15, 2, 1, 0, 6),    // fmin.d
        fp(0x15, 2, 1, 1, 7),    // fmax.d
        fsd(4, 1, 24),
        fsd(5, 1, 32),
        fsd(6, 1, 40),
        fsd(7, 1, 48),
        0x0000_8437, // lui x8,8
    ]);
    let mut bytes = Vec::new();
    for instruction in &code {
        bytes.extend_from_slice(&instruction.to_le_bytes());
    }
    bytes.extend_from_slice(&halfword(0x2004)); // c.fld f9,0(x8)
    bytes.extend_from_slice(&halfword(0xa024)); // c.fsd f9,64(x8)
    bytes.extend_from_slice(&word(0x0000_8137)); // lui sp,8
    bytes.extend_from_slice(&word(addi(2, 2, 0x100)));
    bytes.extend_from_slice(&halfword(0x2502)); // c.fldsp f10,0(sp)
    bytes.extend_from_slice(&word(addi(2, 2, 0x100)));
    bytes.extend_from_slice(&halfword(0xa02a)); // c.fsdsp f10,0(sp)
    bytes.extend_from_slice(&word(0x1050_0073));
    put_mixed(&mut core, CODE, &[&bytes]);
    core.run(60);
    assert_eq!(read_u64(&mut core, 0x8018), 10.0_f64.to_bits());
    assert_eq!(read_u64(&mut core, 0x8020), 2.0_f64.to_bits());
    assert_eq!(read_u64(&mut core, 0x8028), 2.0_f64.to_bits());
    assert_eq!(read_u64(&mut core, 0x8030), 3.0_f64.to_bits());
    assert_eq!(read_u64(&mut core, 0x8040), 2.0_f64.to_bits());
    assert_eq!(read_u64(&mut core, 0x8200), 7.5_f64.to_bits());
}

#[test]
fn tinyemu_traps_when_fp_is_disabled_or_dynamic_rounding_is_reserved() {
    let mut disabled = core();
    put(&mut disabled, 0x9000, &[0x1050_0073]);
    put(
        &mut disabled,
        CODE,
        &[0x0000_92b7, csrrw(CSR_MTVEC, 5), fp(0x00, 2, 1, 0, 3)],
    );
    disabled.run(10);
    assert_eq!(disabled.machine_cause(), 2);

    let mut reserved = core();
    put(&mut reserved, 0x9000, &[0x1050_0073]);
    let mut code = vec![0x0000_92b7, csrrw(CSR_MTVEC, 5)];
    enable_fp(&mut code);
    code.extend([addi(6, 0, 5), csrrw(0x002, 6), fp(0x00, 2, 1, 7, 3)]);
    put(&mut reserved, CODE, &code);
    reserved.run(20);
    assert_eq!(reserved.machine_cause(), 2);
}

#[test]
fn tinyemu_fused_arithmetic_preserves_tiny_products_and_cancellation() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 0x26f0_0000_0000_0000);
    write_u64(&mut core, 0x8008, 0x26f0_0000_0000_0000);
    write_u64(&mut core, 0x8010, 0);
    write_u64(&mut core, 0x8018, 0x3ff0_0000_0000_0001);
    write_u64(&mut core, 0x8020, 0x3fef_ffff_ffff_fffe);
    write_u64(&mut core, 0x8028, 0xbff0_0000_0000_0000);
    let mut code = vec![0x0000_80b7];
    enable_fp(&mut code);
    code.extend([
        i(0, 1, 3, 1, 0x07),
        i(8, 1, 3, 2, 0x07),
        i(16, 1, 3, 3, 0x07),
        fs(1, 3, 2, 1, 4, 0x43),
        fsd(4, 1, 48),
        i(24, 1, 3, 1, 0x07),
        i(32, 1, 3, 2, 0x07),
        i(40, 1, 3, 3, 0x07),
        fs(1, 3, 2, 1, 4, 0x43),
        fsd(4, 1, 56),
        0x1050_0073,
    ]);
    put(&mut core, CODE, &code);
    core.run(40);
    assert_eq!(read_u64(&mut core, 0x8030), 0x0df0_0000_0000_0000);
    assert_eq!(read_u64(&mut core, 0x8038), 0xb970_0000_0000_0000);
}

#[test]
fn tinyemu_fp_rounding_modes_and_sticky_exception_flags_are_observable() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 1.0_f32.to_bits().into());
    write_u64(&mut core, 0x8004, 2.0_f32.powi(-25).to_bits().into());
    write_u64(&mut core, 0x8008, 0);
    let mut code = vec![0x0000_80b7];
    enable_fp(&mut code);
    code.extend([
        i(0, 1, 2, 1, 0x07),
        i(4, 1, 2, 2, 0x07),
        i(8, 1, 2, 3, 0x07),
        fp(0x00, 2, 1, 0, 4),
        fsw(4, 1, 16),
        fp(0x00, 2, 1, 3, 5),
        fsw(5, 1, 20),
        fp(0x0c, 3, 1, 0, 6), // fdiv.s one,zero
        fsw(6, 1, 24),
        csrrs(0x001, 0, 7),
        0x1050_0073,
    ]);
    put(&mut core, CODE, &code);
    core.run(40);
    assert_eq!(read_u64(&mut core, 0x8010) as u32, 1.0_f32.to_bits());
    assert_eq!(read_u64(&mut core, 0x8014) as u32, 1.0_f32.to_bits() + 1);
    assert_eq!(read_u64(&mut core, 0x8018) as u32, f32::INFINITY.to_bits());
    assert_eq!(core.register(7), 1 | 8);
}

#[test]
fn tinyemu_nan_comparisons_minimum_and_classification_follow_riscv() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 0x7fc0_0001);
    write_u64(&mut core, 0x8004, 0x7f80_0001);
    write_u64(&mut core, 0x8008, 1.0_f32.to_bits().into());
    let mut code = vec![0x0000_80b7];
    enable_fp(&mut code);
    code.extend([
        i(0, 1, 2, 1, 0x07),
        i(4, 1, 2, 2, 0x07),
        i(8, 1, 2, 3, 0x07),
        fp(0x14, 3, 1, 0, 4), // fmin.s qNaN,one
        fp(0x14, 3, 2, 1, 5), // fmax.s sNaN,one
        fp(0x50, 3, 1, 2, 6), // feq.s qNaN,one
        fp(0x50, 3, 1, 1, 7), // flt.s qNaN,one
        fp(0x70, 0, 2, 1, 8), // fclass.s sNaN
        fsw(4, 1, 16),
        fsw(5, 1, 20),
        csrrs(0x001, 0, 9),
        0x1050_0073,
    ]);
    put(&mut core, CODE, &code);
    core.run(40);
    assert_eq!(read_u64(&mut core, 0x8010) as u32, 1.0_f32.to_bits());
    assert_eq!(read_u64(&mut core, 0x8014) as u32, 1.0_f32.to_bits());
    assert_eq!(core.register(6), 0);
    assert_eq!(core.register(7), 0);
    assert_eq!(core.register(8), 1 << 8);
    assert_eq!(core.register(9), 1 << 4);
}

#[test]
fn tinyemu_double_division_normalizes_subnormal_operands() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 1);
    write_u64(&mut core, 0x8008, 0x0010_0000_0000_0001);
    let mut code = vec![0x0000_80b7];
    enable_fp(&mut code);
    code.extend([
        i(0, 1, 3, 1, 0x07),
        i(8, 1, 3, 2, 0x07),
        fp(0x0d, 2, 1, 0, 3), // fdiv.d
        fsd(3, 1, 16),
        0x1050_0073,
    ]);
    put(&mut core, CODE, &code);
    core.run(30);
    assert_eq!(read_u64(&mut core, 0x8010), 0x3caf_ffff_ffff_fffe);
}

#[test]
fn tinyemu_float_integer_and_format_conversions_cover_rounding_boundaries() {
    let mut core = core();
    write_u64(&mut core, 0x8000, 2.5_f32.to_bits().into());
    write_u64(&mut core, 0x8008, 1.5_f64.to_bits());
    let mut code = vec![0x0000_80b7];
    enable_fp(&mut code);
    code.extend([
        i(0, 1, 2, 1, 0x07),
        i(8, 1, 3, 2, 0x07),
        fp(0x60, 0, 1, 0, 4), // fcvt.w.s near-even
        fp(0x60, 0, 1, 4, 5), // fcvt.w.s near-max-magnitude
        fp(0x20, 1, 2, 0, 3), // fcvt.s.d
        fsw(3, 1, 16),
        0x8000_0437,          // lui x8, 0x80000
        slli(8, 32),          // i64::MIN
        fp(0x68, 2, 8, 0, 4), // fcvt.s.l
        fsw(4, 1, 20),
        addi(9, 0, -1),
        fp(0x69, 3, 9, 1, 5), // fcvt.d.lu, round toward zero
        fsd(5, 1, 24),
        0x1050_0073,
    ]);
    put(&mut core, CODE, &code);
    core.run(40);
    assert_eq!(core.register(4), 2);
    assert_eq!(core.register(5), 3);
    assert_eq!(read_u64(&mut core, 0x8010) as u32, 1.5_f32.to_bits());
    assert_eq!(
        read_u64(&mut core, 0x8014) as u32,
        (-9_223_372_036_854_775_808.0_f32).to_bits()
    );
    assert_eq!(read_u64(&mut core, 0x8018), 0x43ef_ffff_ffff_ffff);
}
