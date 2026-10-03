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

pub fn priority_and_permissions() {
    // A lower-priority permissive region cannot override a denial or a
    // partial match from the first region that overlaps the access.
    for cfg in [0x10_u64, 0x11] {
        let mut probe = Probe::new();
        probe.csr_write(PMPADDR0, TARGET >> 2);
        probe.csr_write(PMPADDR0 + 1, ALL_MEMORY);
        probe.csr_write(PMPCFG0, cfg | 0x1f00);
        probe.supervisor_data();
        probe.load();
        let core = probe.run();
        assert_eq!(core.machine_cause(), 5);
        assert_eq!(core.machine_trap_value(), TARGET);
    }

    // Unmatched S-mode accesses fail, while unlocked regions cannot deny M.
    for (cfg, supervisor, cause) in [
        (0, true, 5),
        (0, false, 0),
        (0x18, false, 0),
        (0x98, false, 5),
    ] {
        let mut probe = Probe::new();
        probe.csr_write(PMPADDR0, (TARGET >> 2) | 0x1ff);
        probe.csr_write(PMPCFG0, cfg);
        if supervisor {
            probe.supervisor_data();
        }
        probe.load();
        assert_eq!(probe.run().machine_cause(), cause);
    }

    // Locked regions enforce execute permission on instruction fetches too.
    let mut probe = Probe::new();
    probe.csr_write(PMPADDR0, (TARGET >> 2) | 0x1ff);
    probe.csr_write(PMPCFG0, 0x99);
    probe.instructions.extend([0x0000_62b7, 0x0002_8067]);
    assert_eq!(probe.run().machine_cause(), 1);
}

pub fn tor_and_locks() {
    // An inverted TOR range matches no bytes, including large cache-block
    // accesses that straddle both encoded bounds.
    let mut probe = Probe::new();
    probe.csr_write(PMPADDR0, (TARGET + 8) >> 2);
    probe.csr_write(PMPADDR0 + 1, (TARGET + 4) >> 2);
    probe.csr_write(PMPADDR0 + 2, ALL_MEMORY);
    probe.csr_write(PMPCFG0, 0x001f_0f00);
    probe.supervisor_data();
    probe.instructions.extend([0x0000_62b7, 0x0042_a00f]); // cbo.zero (x5)
    let mut core = probe.run();
    assert_eq!(core.machine_cause(), 0);
    assert_eq!(core.ram_range(TARGET, 64, false).unwrap(), &[0; 64]);

    // Changing an OFF entry's address changes the next TOR region's lower
    // bound. A locked TOR entry freezes that predecessor address as well.
    for locked in [false, true] {
        let mut probe = Probe::new();
        probe.csr_write(PMPADDR0, TARGET >> 2);
        probe.csr_write(PMPADDR0 + 1, (TARGET + 0x1000) >> 2);
        probe.csr_write(PMPCFG0, if locked { 0x8900 } else { 0x0900 });
        probe.csr_write(PMPADDR0, (TARGET + 0x1000) >> 2);
        if locked {
            probe.csr_write(PMPADDR0 + 1, 0);
            probe.csr_write(PMPCFG0, 0);
        }
        probe.supervisor_data();
        probe.load();
        let core = probe.run();
        assert_eq!(core.machine_cause(), if locked { 0 } else { 5 });
    }
}

pub fn invalidation() {
    // Populate a read TLB entry, then revoke the region's permission. The
    // second load must fault despite the previous cached translation.
    let mut probe = Probe::new();
    probe.csr_write(PMPADDR0, ALL_MEMORY);
    probe.csr_write(PMPCFG0, 0x1f);
    probe.supervisor_data();
    probe.load();
    probe.csr_write(0x300, 0); // restore M-mode data accesses for literal loads
    probe.csr_write(PMPCFG0, 0x18);
    probe.supervisor_data();
    probe.load();
    let core = probe.run();
    assert_eq!(core.register(10), 0x1050_0073);
    assert_eq!(core.machine_cause(), 5);

    // Changes in the upper configuration CSR rebuild the same ordered list.
    let mut probe = Probe::new();
    probe.csr_write(PMPADDR0 + 8, ALL_MEMORY);
    probe.csr_write(PMPCFG0 + 2, 0x1f);
    probe.supervisor_data();
    probe.load();
    assert_eq!(probe.run().machine_cause(), 0);
}
