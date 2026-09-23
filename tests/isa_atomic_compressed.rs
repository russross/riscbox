use riscbox::cpu::Cpu;
use riscbox::guest_memory::{AccessWidth, GuestAddress, RamFlags};
use riscbox::memory::PhysicalMemory;

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
