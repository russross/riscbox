use riscbox::tinyemu_core::Core;

const CODE: u64 = 0x1000;
const TRAP: u64 = 0x2000;
const LITERALS: u64 = 0x4000;
const TARGET: u64 = 0x6000;
const PMPCFG0: u32 = 0x3a0;
const PMPADDR0: u32 = 0x3b0;
const ALL_MEMORY: u64 = 0x003f_ffff_ffff_ffff;

struct Probe {
    instructions: Vec<u32>,
    literals: Vec<u64>,
}

impl Probe {
    fn new() -> Self {
        Self {
            instructions: vec![
                0x0000_4fb7, // lui x31, 4
                0x0000_22b7, // lui x5, 2
                0x3052_9073, // csrrw zero, mtvec, x5
            ],
            literals: Vec::new(),
        }
    }

    // Literal loads let the guest write complete RV64 CSR values without
    // relying on instruction encodings that only cover small immediates.
    fn csr_write(&mut self, csr: u32, value: u64) {
        let offset = u32::try_from(self.literals.len() * 8).unwrap();
        self.literals.push(value);
        self.instructions
            .push(offset << 20 | 31 << 15 | 3 << 12 | 5 << 7 | 0x03);
        self.instructions.push(csr << 20 | 5 << 15 | 1 << 12 | 0x73);
    }

    fn supervisor_data(&mut self) {
        self.csr_write(0x300, 0x20800); // MPRV with MPP=S
    }

    fn load(&mut self) {
        self.instructions.extend([0x0000_62b7, 0x0002_b503]);
    }

    // The trap handler and target instruction both wait, so a fault terminates
    // each probe and leaves its architectural cause available to the host.
    fn run(mut self) -> Core {
        self.instructions.push(0x1050_0073);
        let mut core = Core::new().unwrap();
        core.register_ram(0, 0x8000, 0).unwrap();
        put(&mut core, CODE, &self.instructions);
        put(&mut core, TRAP, &[0x1050_0073]);
        put(&mut core, TARGET, &[0x1050_0073]);
        for (bytes, value) in core
            .ram_range(LITERALS, self.literals.len() * 8, true)
            .unwrap()
            .as_chunks_mut::<8>()
            .0
            .iter_mut()
            .zip(self.literals)
        {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        core.run_cpu(1000);
        core
    }
}

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

pub fn memory_accesses() {
    // PMP configurations, including locked denials and partial ranges, do
    // not restrict effective S-mode loads or stores.
    for cfg in [0, 0x98, 0x90] {
        let mut probe = Probe::new();
        probe.csr_write(PMPADDR0, TARGET >> 2);
        probe.csr_write(PMPCFG0, cfg);
        probe.supervisor_data();
        probe.load();
        probe.instructions.extend([0x0070_0593, 0x00b2_b023]);
        let mut core = probe.run();
        assert_eq!(core.machine_cause(), 0);
        assert_eq!(core.register(10), 0x1050_0073);
        assert_eq!(
            core.ram_range(TARGET, 8, false).unwrap(),
            &7_u64.to_le_bytes()
        );
    }

    // Locked execute denials do not restrict instruction fetches, and a
    // locked write denial does not restrict cache-block zeroing.
    let mut probe = Probe::new();
    probe.csr_write(PMPADDR0, (TARGET >> 2) | 0x1ff);
    probe.csr_write(PMPCFG0, 0x98);
    probe.instructions.extend([0x0000_62b7, 0x0002_8067]);
    let core = probe.run();
    assert_eq!(core.machine_cause(), 0);
    assert_eq!(core.pc(), TARGET + 4);

    let mut probe = Probe::new();
    probe.csr_write(PMPADDR0, (TARGET >> 2) | 0x1ff);
    probe.csr_write(PMPCFG0, 0x98);
    probe.supervisor_data();
    probe.instructions.extend([0x0000_62b7, 0x0042_a00f]);
    let mut core = probe.run();
    assert_eq!(core.machine_cause(), 0);
    assert_eq!(core.ram_range(TARGET, 64, false).unwrap(), &[0; 64]);
}

pub fn csr_handlers() {
    // Address width and reserved configuration bits retain their WARL
    // behavior even though the stored permissions do not restrict memory.
    let mut probe = Probe::new();
    probe.csr_write(PMPADDR0, u64::MAX);
    probe.csr_write(PMPCFG0, 0x66);
    probe.instructions.extend([0x3b00_2573, 0x3a00_25f3]);
    let core = probe.run();
    assert_eq!(core.register(10), ALL_MEMORY);
    assert_eq!(core.register(11), 4);

    // Locks preserve both configuration CSRs and the addresses they protect.
    for entry in [0, 8] {
        let mut probe = Probe::new();
        let cfg_csr = PMPCFG0 + if entry == 0 { 0 } else { 2 };
        probe.csr_write(PMPADDR0 + entry, TARGET >> 2);
        probe.csr_write(cfg_csr, 0x98);
        probe.csr_write(PMPADDR0 + entry, 0);
        probe.csr_write(cfg_csr, 0);
        probe
            .instructions
            .push((PMPADDR0 + entry) << 20 | 2 << 12 | 10 << 7 | 0x73);
        probe
            .instructions
            .push(cfg_csr << 20 | 2 << 12 | 11 << 7 | 0x73);
        let core = probe.run();
        assert_eq!(core.register(10), TARGET >> 2);
        assert_eq!(core.register(11), 0x98);
    }

    // A locked TOR entry also freezes the preceding address register,
    // including when that predecessor's own configuration is OFF.
    let mut probe = Probe::new();
    probe.csr_write(PMPADDR0, TARGET >> 2);
    probe.csr_write(PMPADDR0 + 1, (TARGET + 0x1000) >> 2);
    probe.csr_write(PMPCFG0, 0x8900);
    probe.csr_write(PMPADDR0, 0);
    probe.csr_write(PMPADDR0 + 1, 0);
    probe.instructions.extend([0x3b00_2573, 0x3b10_25f3]);
    let core = probe.run();
    assert_eq!(core.register(10), TARGET >> 2);
    assert_eq!(core.register(11), (TARGET + 0x1000) >> 2);
}
