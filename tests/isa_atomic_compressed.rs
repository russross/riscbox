use riscbox::cpu::Cpu;
use riscbox::memory::{AccessWidth, GuestAddress, PhysicalMemory, RamFlags};

fn machine() -> (Cpu, PhysicalMemory) {
    let mut memory = PhysicalMemory::new();
    memory
        .register_ram(GuestAddress(0), 0x1_0000, RamFlags::default())
        .unwrap();
    (Cpu::new(0), memory)
}

fn run32(cpu: &mut Cpu, memory: &mut PhysicalMemory, instruction: u32) {
    memory
        .write(
            GuestAddress(cpu.pc()),
            AccessWidth::Word,
            u64::from(instruction),
        )
        .unwrap();
    assert_eq!(cpu.run(memory, 1).cycles, 1);
}

fn run16(cpu: &mut Cpu, memory: &mut PhysicalMemory, instruction: u16) {
    memory
        .write(
            GuestAddress(cpu.pc()),
            AccessWidth::HalfWord,
            u64::from(instruction),
        )
        .unwrap();
    assert_eq!(cpu.run(memory, 1).cycles, 1);
}

fn amo(operation: u32, rs2: u32, rs1: u32, width: u32, rd: u32) -> u32 {
    operation << 27 | rs2 << 20 | rs1 << 15 | width << 12 | rd << 7 | 0x2f
}

fn c_li(rd: u16, immediate: u16) -> u16 {
    0x4001 | rd << 7 | (immediate & 0x1f) << 2 | (immediate & 0x20) << 7
}

#[test]
fn atomic_word_and_doubleword_operations_return_old_values() {
    let (mut cpu, mut memory) = machine();
    cpu.set_register(1, 0x8000);
    cpu.set_register(2, 5);
    memory
        .write(GuestAddress(0x8000), AccessWidth::Word, 0xffff_fffe)
        .unwrap();
    run32(&mut cpu, &mut memory, amo(0, 2, 1, 2, 3));
    assert_eq!(cpu.register(3), u64::MAX - 1);
    assert_eq!(memory.read(GuestAddress(0x8000), AccessWidth::Word), Ok(3));

    cpu.set_register(2, 9);
    run32(&mut cpu, &mut memory, amo(1, 2, 1, 3, 4));
    assert_eq!(cpu.register(4), 3);
    assert_eq!(
        memory.read(GuestAddress(0x8000), AccessWidth::DoubleWord),
        Ok(9)
    );
}

#[test]
fn load_reserved_store_conditional_tracks_address_and_width() {
    let (mut cpu, mut memory) = machine();
    cpu.set_register(1, 0x8000);
    cpu.set_register(2, 0x1234_5678);
    memory
        .write(GuestAddress(0x8000), AccessWidth::Word, 0x8000_0000)
        .unwrap();
    run32(&mut cpu, &mut memory, amo(2, 0, 1, 2, 3));
    assert_eq!(cpu.register(3), 0xffff_ffff_8000_0000);
    run32(&mut cpu, &mut memory, amo(3, 2, 1, 2, 4));
    assert_eq!(cpu.register(4), 0);
    assert_eq!(
        memory.read(GuestAddress(0x8000), AccessWidth::Word),
        Ok(0x1234_5678)
    );

    run32(&mut cpu, &mut memory, amo(3, 2, 1, 2, 5));
    assert_eq!(cpu.register(5), 1);
}

#[test]
fn compressed_integer_control_and_stack_memory_use_two_byte_pc_steps() {
    let (mut cpu, mut memory) = machine();
    run16(&mut cpu, &mut memory, c_li(1, 31));
    run16(&mut cpu, &mut memory, 0x0085); // c.addi x1, 1
    assert_eq!(cpu.register(1), 32);
    assert_eq!(cpu.pc(), 0x1004);

    cpu.set_register(2, 0x8000);
    cpu.set_register(3, 0xffff_ffff_8123_4567);
    run16(&mut cpu, &mut memory, 0xc00e); // c.swsp x3, 0(sp)
    run16(&mut cpu, &mut memory, 0x4202); // c.lwsp x4, 0(sp)
    assert_eq!(cpu.register(4), 0xffff_ffff_8123_4567);

    cpu.set_register(5, 0x2002);
    run16(&mut cpu, &mut memory, 0x8282); // c.jr x5
    assert_eq!(cpu.pc(), 0x2002);
}

#[test]
fn zcb_unary_multiply_and_byte_memory_operations_execute() {
    let (mut cpu, mut memory) = machine();
    cpu.set_register(8, 0xffff_ffff_ffff_ff80);
    run16(&mut cpu, &mut memory, 0x9c61); // c.zext.b x8
    assert_eq!(cpu.register(8), 0x80);

    cpu.set_register(9, 3);
    run16(&mut cpu, &mut memory, 0x9c45); // c.mul x8, x9
    assert_eq!(cpu.register(8), 0x180);

    cpu.set_register(10, 0x8000);
    cpu.set_register(11, 0xab);
    run16(&mut cpu, &mut memory, 0x890c); // c.sb x11, 0(x10)
    run16(&mut cpu, &mut memory, 0x8100); // c.lbu x8, 0(x10)
    assert_eq!(cpu.register(8), 0xab);
}

#[test]
fn compressed_reserved_encoding_traps_with_halfword_value() {
    let (mut cpu, mut memory) = machine();
    run16(&mut cpu, &mut memory, 0x0000);
    assert_eq!(cpu.read_csr(0x342), Ok(2));
    assert_eq!(cpu.read_csr(0x343), Ok(0));
}

#[test]
fn compressed_mops_and_lui_hints_retire_as_noops() {
    for instruction in (0x6081_u16..=0x6781).step_by(0x100) {
        let (mut cpu, mut memory) = machine();
        cpu.set_register(1, 0x1234);
        run16(&mut cpu, &mut memory, instruction);
        assert_eq!(cpu.pc(), 0x1002);
        assert_eq!(cpu.register(1), 0x1234);
    }

    let (mut cpu, mut memory) = machine();
    run16(&mut cpu, &mut memory, 0x6005); // c.lui x0,1 hint
    assert_eq!(cpu.pc(), 0x1002);
}
