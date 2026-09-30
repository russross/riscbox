//! Exercises actual guest rings and Rust endpoint state on native and WASM.

use riscbox::entropy::{EntropyError, EntropySource};
use riscbox::guest_memory::{AccessWidth, GuestAddress};
use riscbox::machine::{BootImages, Machine, MachineConfig, VIRTIO_BASE};
use riscbox::ninep::{FileRead, Filesystem, Limits, SourceId};
use riscbox::ninep_backend::{RustFilesystem, RustNineP};
use riscbox::tinyemu_core::CpuRunExitReason;
use std::cell::RefCell;
use std::rc::Rc;

const RAM: u64 = 0x8000_0000;

struct TestEntropy;

impl EntropySource for TestEntropy {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), EntropyError> {
        bytes.fill(42);
        Ok(())
    }
}

fn new_machine() -> Machine {
    // Browser machines always receive host entropy. Deterministic bytes make
    // this isolated transport probe independent of operating-system services.
    Machine::new_with_entropy(
        MachineConfig {
            ram_size: 32 << 20,
            framebuffer: None,
        },
        Rc::new(RefCell::new(TestEntropy)),
    )
    .unwrap()
}

struct Ring {
    slot: usize,
    base: u64,
    available: u16,
}

fn write(machine: &mut Machine, address: u64, width: AccessWidth, value: u64) {
    machine
        .bus_mut()
        .write(GuestAddress(address), width, value)
        .unwrap();
}

fn word(machine: &mut Machine, address: u64) -> u32 {
    u32::from_le_bytes(machine.read_ram(address, 4).unwrap().try_into().unwrap())
}

impl Ring {
    fn new(machine: &mut Machine, tree: &RustFilesystem, index: u32) -> Self {
        let slot = machine
            .add_ninep_device(Box::new(RustNineP::new(tree.clone()).unwrap()), b"share")
            .unwrap();
        let ring = Self {
            slot,
            base: RAM + 0x1000 + u64::from(index) * 0x10000,
            available: 0,
        };
        ring.configure(machine);
        ring
    }

    fn mmio(&self) -> u64 {
        VIRTIO_BASE + u64::try_from(self.slot).unwrap() * 0x1000
    }

    fn configure(&self, machine: &mut Machine) {
        write(machine, self.mmio() + 0x30, AccessWidth::Word, 0);
        write(machine, self.mmio() + 0x38, AccessWidth::Word, 16);
        for (offset, address) in [
            (0x80, self.base),
            (0x90, self.base + 0x1000),
            (0xa0, self.base + 0x2000),
        ] {
            write(machine, self.mmio() + offset, AccessWidth::Word, address);
            write(
                machine,
                self.mmio() + offset + 4,
                AccessWidth::Word,
                address >> 32,
            );
        }
        write(machine, self.mmio() + 0x44, AccessWidth::Word, 1);
        write(machine, self.mmio() + 0x70, AccessWidth::Word, 4);
    }

    fn enqueue(&mut self, machine: &mut Machine, pair: u16, kind: u8, tag: u16, body: &[u8]) {
        let mut request = u32::try_from(body.len() + 7)
            .unwrap()
            .to_le_bytes()
            .to_vec();
        request.push(kind);
        request.extend(tag.to_le_bytes());
        request.extend(body);
        let input = self.base + 0x3000 + u64::from(pair) * 0x800;
        machine.write_ram(input, &request).unwrap();
        let head = pair * 2;
        for (descriptor, address, length, flags, next) in [
            (
                head,
                input,
                u32::try_from(request.len()).unwrap(),
                1,
                head + 1,
            ),
            (head + 1, input + 0x400, 1024, 2, 0),
        ] {
            let location = self.base + u64::from(descriptor) * 16;
            write(machine, location, AccessWidth::DoubleWord, address);
            write(machine, location + 8, AccessWidth::Word, u64::from(length));
            write(machine, location + 12, AccessWidth::HalfWord, flags);
            write(
                machine,
                location + 14,
                AccessWidth::HalfWord,
                u64::from(next),
            );
        }
        write(
            machine,
            self.base + 0x1004 + u64::from(self.available % 16) * 2,
            AccessWidth::HalfWord,
            u64::from(head),
        );
        self.available += 1;
        write(
            machine,
            self.base + 0x1002,
            AccessWidth::HalfWord,
            u64::from(self.available),
        );
    }

    fn submit(&mut self, machine: &mut Machine, pair: u16, kind: u8, tag: u16, body: &[u8]) {
        self.enqueue(machine, pair, kind, tag, body);
        write(machine, self.mmio() + 0x50, AccessWidth::Word, 0);
    }

    fn cpu_kick(&self, machine: &mut Machine) -> CpuRunExitReason {
        // A small firmware probe notifies the device then sleeps. Resident
        // service reaches WFI in this CPU run; a source load exits at MMIO.
        let address = u32::try_from(self.mmio()).unwrap();
        let instructions = [
            (address & 0xffff_f000) | (5 << 7) | 0x37,
            ((address & 0xfff) << 20) | (5 << 15) | (5 << 7) | 0x13,
            (2 << 25) | (5 << 15) | (2 << 12) | (16 << 7) | 0x23,
            0x1050_0073,
        ];
        let firmware: Vec<u8> = instructions
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        machine
            .load_boot(BootImages {
                firmware: Some(&firmware),
                kernel: None,
                initrd: None,
                command_line: "",
            })
            .unwrap();
        machine.run_cpu(1000).state
    }

    fn count(&self, machine: &mut Machine) -> u16 {
        u16::from_le_bytes(
            machine
                .read_ram(self.base + 0x2002, 2)
                .unwrap()
                .try_into()
                .unwrap(),
        )
    }

    fn used(&self, machine: &mut Machine, index: u16) -> (u32, u32) {
        let address = self.base + 0x2004 + u64::from(index % 16) * 8;
        (word(machine, address), word(machine, address + 4))
    }

    fn reply<'a>(&self, machine: &'a mut Machine, pair: u16) -> &'a [u8] {
        let address = self.base + 0x3400 + u64::from(pair) * 0x800;
        let length = usize::try_from(word(machine, address)).unwrap();
        machine.read_ram(address, length).unwrap()
    }

    fn open(&mut self, machine: &mut Machine, name: &str, fid: u32) {
        self.open_flags(machine, name, fid, 0);
    }

    fn open_flags(&mut self, machine: &mut Machine, name: &str, fid: u32, flags: u32) {
        let mut walk = 1_u32.to_le_bytes().to_vec();
        walk.extend(fid.to_le_bytes());
        walk.extend(1_u16.to_le_bytes());
        string(&mut walk, name);
        self.submit(machine, 0, 110, 2, &walk);
        assert_eq!(self.reply(machine, 0)[4], 111);
        let mut open = fid.to_le_bytes().to_vec();
        open.extend(flags.to_le_bytes());
        self.submit(machine, 0, 12, 3, &open);
        assert_eq!(self.reply(machine, 0)[4], 13);
    }

    fn negotiate(&mut self, machine: &mut Machine) {
        let mut version = 4096_u32.to_le_bytes().to_vec();
        string(&mut version, "9P2000.L");
        self.submit(machine, 0, 100, u16::MAX, &version);
        assert_eq!(self.reply(machine, 0)[4], 101);
        let mut attach = 1_u32.to_le_bytes().to_vec();
        attach.extend(u32::MAX.to_le_bytes());
        string(&mut attach, "host");
        string(&mut attach, "");
        attach.extend(1000_u32.to_le_bytes());
        self.submit(machine, 0, 104, 1, &attach);
        assert_eq!(self.reply(machine, 0)[4], 105);
    }
}

fn string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend(u16::try_from(value.len()).unwrap().to_le_bytes());
    bytes.extend(value.as_bytes());
}

fn read(fid: u32) -> Vec<u8> {
    let mut body = fid.to_le_bytes().to_vec();
    body.extend(0_u64.to_le_bytes());
    body.extend(3_u32.to_le_bytes());
    body
}

pub fn regression() {
    let mut filesystem = Filesystem::new(Limits::default(), 100);
    filesystem.write_file("resident", b"now").unwrap();
    filesystem.add_lazy_file("first", 3, SourceId(1)).unwrap();
    filesystem.add_lazy_file("second", 3, SourceId(2)).unwrap();
    let tree = RustFilesystem::new(filesystem);
    let mut machine = new_machine();
    let mut ring = Ring::new(&mut machine, &tree, 0);
    ring.negotiate(&mut machine);
    ring.open(&mut machine, "resident", 2);
    ring.enqueue(&mut machine, 0, 116, 4, &read(2));
    assert_eq!(ring.cpu_kick(&mut machine), CpuRunExitReason::WfiSleep);
    assert_eq!(&ring.reply(&mut machine, 0)[11..], b"now");
    assert_eq!(ring.count(&mut machine), 5);
    assert!(tree.next_load().is_none());

    // Boot loading preserves an existing CPU's sleep state. Use a fresh CPU
    // and independent namespace for the lazy-service exit probe.
    let lazy_tree = RustFilesystem::new(Filesystem::new(Limits::default(), 100));
    lazy_tree
        .with_filesystem(|fs| fs.add_lazy_file("first", 3, SourceId(99)))
        .unwrap();
    let mut lazy_machine = new_machine();
    let mut lazy_ring = Ring::new(&mut lazy_machine, &lazy_tree, 0);
    lazy_ring.negotiate(&mut lazy_machine);
    lazy_ring.open(&mut lazy_machine, "first", 3);
    lazy_ring.enqueue(&mut lazy_machine, 1, 116, 10, &read(3));
    assert_eq!(
        lazy_ring.cpu_kick(&mut lazy_machine),
        CpuRunExitReason::HostServiceRequested
    );
    drop(lazy_machine);

    // A flush publishes the zero-length retirement before its own reply.
    ring.open(&mut machine, "first", 3);
    ring.submit(&mut machine, 1, 116, 10, &read(3));
    let first = tree.next_load().unwrap();
    let before = ring.count(&mut machine);
    ring.submit(&mut machine, 2, 108, 11, &10_u16.to_le_bytes());
    assert_eq!(ring.count(&mut machine), before + 2);
    assert_eq!(ring.used(&mut machine, before), (2, 0));
    assert_eq!(ring.used(&mut machine, before + 1), (4, 7));
    ring.submit(&mut machine, 1, 116, 10, &read(3));
    assert!(tree.next_load().is_none());

    // Independent source completions publish in completion order, not request
    // order; another endpoint can join the same retained source operation.
    ring.open(&mut machine, "second", 4);
    ring.submit(&mut machine, 3, 116, 12, &read(4));
    let second = tree.next_load().unwrap();
    let mut other = Ring::new(&mut machine, &tree, 1);
    other.negotiate(&mut machine);
    other.open(&mut machine, "first", 3);
    other.submit(&mut machine, 1, 116, 10, &read(3));
    assert!(tree.next_load().is_none());
    let before = ring.count(&mut machine);
    tree.with_filesystem(|fs| fs.complete_load(second, b"two".to_vec()))
        .unwrap();
    machine.poll_ninep().unwrap();
    assert_eq!(ring.used(&mut machine, before), (6, 14));
    assert_eq!(&ring.reply(&mut machine, 3)[11..], b"two");
    tree.with_filesystem(|fs| fs.complete_load(first, b"one".to_vec()))
        .unwrap();
    machine.poll_ninep().unwrap();
    assert_eq!(&ring.reply(&mut machine, 1)[11..], b"one");
    assert_eq!(&other.reply(&mut machine, 1)[11..], b"one");

    // A guest truncation on another endpoint satisfies the pending read in
    // the same notification, and superseded source work is never dispatched.
    tree.with_filesystem(|fs| fs.add_lazy_file("truncate", 3, SourceId(5)))
        .unwrap();
    ring.open(&mut machine, "truncate", 7);
    ring.submit(&mut machine, 1, 116, 20, &read(7));
    let before = ring.count(&mut machine);
    other.open_flags(&mut machine, "truncate", 7, 0x202);
    assert_eq!(ring.count(&mut machine), before + 1);
    assert_eq!(ring.used(&mut machine, before), (2, 11));
    assert_eq!(ring.reply(&mut machine, 1)[4], 117);
    assert!(tree.next_load().is_none());

    // Source failure publishes an ordinary tagged EIO response; it does not
    // fail the endpoint or require a new notification from the guest.
    tree.with_filesystem(|fs| fs.add_lazy_file("failure", 3, SourceId(6)))
        .unwrap();
    ring.open(&mut machine, "failure", 8);
    ring.submit(&mut machine, 1, 116, 21, &read(8));
    let failure = tree.next_load().unwrap();
    tree.with_filesystem(|fs| fs.fail_load(failure)).unwrap();
    machine.poll_ninep().unwrap();
    assert_eq!(ring.reply(&mut machine, 1)[4], 7);
    assert_eq!(word(&mut machine, ring.base + 0x3c07), 5);

    // Namespace reset completes retained descriptors with ESTALE, whereas
    // device reset never writes old guest rings even if a source finishes.
    tree.with_filesystem(|fs| fs.add_lazy_file("late", 3, SourceId(3)))
        .unwrap();
    ring.open(&mut machine, "late", 5);
    ring.submit(&mut machine, 1, 116, 13, &read(5));
    let late = tree.next_load().unwrap();
    tree.with_filesystem(Filesystem::reset).unwrap();
    machine.poll_ninep().unwrap();
    assert_eq!(ring.reply(&mut machine, 1)[4], 7);
    assert_eq!(word(&mut machine, ring.base + 0x3c07), 116);
    assert!(
        tree.with_filesystem(|fs| fs.complete_load(late, b"old".to_vec()))
            .is_err()
    );
    tree.with_filesystem(|fs| fs.add_lazy_file("after", 3, SourceId(4)))
        .unwrap();
    ring.submit(&mut machine, 0, 104, 14, &{
        let mut attach = 1_u32.to_le_bytes().to_vec();
        attach.extend(u32::MAX.to_le_bytes());
        string(&mut attach, "host");
        string(&mut attach, "");
        attach.extend(1000_u32.to_le_bytes());
        attach
    });
    ring.open(&mut machine, "after", 6);
    ring.submit(&mut machine, 1, 116, 15, &read(6));
    let load = tree.next_load().unwrap();
    let old_ring = machine.read_ram(ring.base + 0x2000, 132).unwrap().to_vec();
    write(&mut machine, ring.mmio() + 0x70, AccessWidth::Word, 0);
    tree.with_filesystem(|fs| fs.complete_load(load, b"new".to_vec()))
        .unwrap();
    machine.poll_ninep().unwrap();
    assert_eq!(machine.read_ram(ring.base + 0x2000, 132).unwrap(), old_ring);
    // Whole-VM reset also retires every endpoint while preserving undispatched
    // namespace loads. Their later completions cannot touch either old ring.
    tree.with_filesystem(|fs| fs.add_lazy_file("vm-reset", 3, SourceId(8)))
        .unwrap();
    other.negotiate(&mut machine);
    other.open(&mut machine, "vm-reset", 9);
    other.submit(&mut machine, 1, 116, 22, &read(9));
    let old_other_ring = machine.read_ram(other.base + 0x2000, 132).unwrap().to_vec();
    machine.reset().unwrap();
    let retained_load = tree.next_load().unwrap();
    tree.with_filesystem(|fs| fs.complete_load(retained_load, b"yes".to_vec()))
        .unwrap();
    machine.poll_ninep().unwrap();
    assert_eq!(
        machine.read_ram(other.base + 0x2000, 132).unwrap(),
        old_other_ring
    );
    assert_eq!(machine.read_ram(ring.base + 0x2000, 132).unwrap(), old_ring);
    drop(machine);
    assert_eq!(
        tree.with_filesystem(|fs| fs.read_file("after")).unwrap(),
        FileRead::Resident(b"new".to_vec())
    );
}
