use riscbox::tinyemu_core::{BusError, Core, PlatformCallbacks};

const CODE: u64 = 0x1000;
const DATA: u64 = 0x4000;

fn core(instructions: &[u32]) -> Core {
    let mut core = Core::new().unwrap();
    core.register_ram(0, 0x8000, 0).unwrap();
    put(&mut core, CODE, instructions);
    core
}

// Each probe starts at the reset PC and observes guest registers after WFI.
fn put(core: &mut Core, address: u64, instructions: &[u32]) {
    for (bytes, instruction) in core
        .ram_range(address, instructions.len() * 4, true)
        .unwrap()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(instructions)
    {
        bytes.copy_from_slice(&instruction.to_le_bytes());
    }
}

fn read_counter(rd: u32) -> u32 {
    0xb02 << 20 | rd << 7 | 2 << 12 | 0x73
}

pub fn retirement() {
    // Writing minstret suppresses retirement of that CSR instruction. The
    // following compressed instructions and taken jump each retire normally.
    let mut core = core(&[
        0xfff0_0093, // addi x1, zero, -1
        0xb020_9073, // csrrw zero, minstret, x1
        0x0001_0001, // two c.nop instructions
        0x0080_006f, // jal zero, +8
        0xffff_ffff, // skipped illegal instruction
        read_counter(2),
        0xb020_90f3, // csrrw x1, minstret, x1
        read_counter(3),
        0x1050_0073, // wfi
    ]);
    core.run_cpu(100);
    assert_eq!(core.register(2), 2);
    assert_eq!(core.register(1), 3);
    assert_eq!(core.register(3), u64::MAX);

    // Faults consume cycles without retiring. The trap handler reads the
    // counter before its own read and WFI instructions retire.
    let mut trapped = self::core(&[
        0x0000_20b7, // lui x1, 2
        0x3050_9073, // csrrw zero, mtvec, x1
        0x0001_0001, // two c.nop instructions
        0xffff_ffff,
    ]);
    put(&mut trapped, 0x2000, &[read_counter(2), 0x1050_0073]);
    trapped.run_cpu(100);
    assert_eq!(trapped.machine_cause(), 2);
    assert_eq!(trapped.register(2), 4);

    // A block-boundary exit must publish the accumulator before the next run.
    let mut bounded = self::core(&[0x0040_006f, read_counter(2), 0x1050_0073]);
    assert_eq!(bounded.run_cpu(1).consumed_cycles, 1);
    bounded.run_cpu(100);
    assert_eq!(bounded.register(2), 1);

    // Read-modify-write CSR forms suppress their own increment just like
    // CSRRW, while the user alias sees the same architectural counter.
    let mut modified = self::core(&[
        0x00a0_0093, // addi x1, zero, 10
        0xb020_9073, // csrrw zero, minstret, x1
        0xb020_a173, // csrrs x2, minstret, x1
        0xb020_b1f3, // csrrc x3, minstret, x1
        0xc020_2273, // csrrs x4, instret, zero
        0x1050_0073,
    ]);
    modified.run_cpu(100);
    assert_eq!(modified.register(2), 10);
    assert_eq!(modified.register(3), 10);
    assert_eq!(modified.register(4), 0);
}

struct Handoff;

impl PlatformCallbacks for Handoff {
    fn read(&mut self, _address: u64, _width: u32) -> Result<u32, BusError> {
        Ok(0)
    }

    fn write(&mut self, _address: u64, _width: u32, _value: u32) -> Result<(), BusError> {
        Ok(())
    }

    fn interrupts(&self) -> u32 {
        0
    }

    fn host_service_requested(&self) -> bool {
        true
    }
}

pub fn retirement_handoff() {
    // The completed MMIO store retires before control returns to the host.
    let mut core = core(&[
        0x1000_00b7, // lui x1, 0x10000
        0x0000_a023, // sw zero, 0(x1)
        read_counter(2),
        0x1050_0073,
    ]);
    core.register_device(0x1000_0000, 0x1000, 4).unwrap();
    let result = core.run_cpu_with_platform(100, &mut Handoff);
    assert_eq!((result.consumed_cycles, result.reason), (2, 2));
    core.run_cpu(100);
    assert_eq!(core.register(2), 2);
}

pub fn high_multiply() {
    // Limb carries, signed extremes, and mixed signs exercise all three high
    // products. Rust's wide arithmetic supplies an independent expected value.
    let values = [
        0,
        1,
        2,
        0xffff_ffff,
        0x1_0000_0000,
        0x1_0000_0001,
        0x7fff_ffff_ffff_ffff,
        0x8000_0000_0000_0000,
        0x8000_0000_0000_0001,
        0xffff_ffff_0000_0000,
        0xffff_ffff_ffff_fffe,
        u64::MAX,
    ];
    for a in values {
        for b in values {
            multiply_pair(a, b);
        }
    }

    // Deterministic operands add carry combinations beyond the boundary table.
    let mut state = 0x243f_6a88_85a3_08d3_u64;
    for _ in 0..128 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let a = state;
        state = state.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        multiply_pair(a, state);
    }
}

fn multiply_pair(a: u64, b: u64) {
    let mut core = core(&[
        0x0000_42b7, // lui x5, 4
        0x0002_b083, // ld x1, 0(x5)
        0x0082_b103, // ld x2, 8(x5)
        0x0220_91b3, // mulh x3, x1, x2
        0x0220_a233, // mulhsu x4, x1, x2
        0x0220_b2b3, // mulhu x5, x1, x2
        0x1050_0073,
    ]);
    let data = core.ram_range(DATA, 16, true).unwrap();
    data[..8].copy_from_slice(&a.to_le_bytes());
    data[8..].copy_from_slice(&b.to_le_bytes());
    core.run_cpu(100);
    let signed_a = i128::from(a.cast_signed());
    let signed_b = i128::from(b.cast_signed());
    let high = |value: u128| u64::try_from(value >> 64).unwrap();
    assert_eq!(
        core.register(3),
        high((signed_a * signed_b).cast_unsigned())
    );
    assert_eq!(
        core.register(4),
        high((signed_a * i128::from(b)).cast_unsigned())
    );
    assert_eq!(core.register(5), high(u128::from(a) * u128::from(b)));
}
