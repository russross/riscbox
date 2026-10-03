//! Exercises actual guest rings and Rust endpoint state on native and WASM.

use riscbox::entropy::{EntropyError, EntropySource};
use riscbox::guest_memory::{AccessWidth, GuestAddress};
use riscbox::machine::{Machine, MachineConfig, VIRTIO_BASE};
use riscbox::ninep::{Filesystem, Limits, SharedFilesystem};
use riscbox::ninep_protocol::NinePEndpoint;
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
    fn new(machine: &mut Machine, tree: &SharedFilesystem, index: u32) -> Self {
        let slot = machine
            .add_ninep_device(NinePEndpoint::new(tree.clone()).unwrap(), b"share")
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

// Resident requests complete inside queue notification, with independent sessions.
pub fn regression() {
    let tree = SharedFilesystem::new(Filesystem::new(Limits::default(), 100));
    tree.with_filesystem(|fs| fs.write_file("file", b"abc"))
        .unwrap();
    let mut machine = new_machine();
    let mut first = Ring::new(&mut machine, &tree, 0);
    let mut second = Ring::new(&mut machine, &tree, 1);
    first.negotiate(&mut machine);
    second.negotiate(&mut machine);
    first.open(&mut machine, "file", 2);
    second.open(&mut machine, "file", 2);
    first.submit(&mut machine, 0, 116, 4, &read(2));
    assert_eq!(&first.reply(&mut machine, 0)[11..], b"abc");
    tree.with_filesystem(|fs| fs.write_file("file", b"new"))
        .unwrap();
    second.submit(&mut machine, 0, 116, 4, &read(2));
    assert_eq!(&second.reply(&mut machine, 0)[11..], b"new");
    assert!(first.count(&mut machine) > 0);
    assert!(first.used(&mut machine, 0).1 > 0);
    machine.reset().unwrap();
    assert_eq!(
        tree.with_filesystem(|fs| fs.read_file("file")).unwrap(),
        b"new"
    );
    first.configure(&mut machine);
    first.negotiate(&mut machine);
    first.open(&mut machine, "file", 2);
    first.submit(&mut machine, 0, 116, 4, &read(2));
    assert_eq!(&first.reply(&mut machine, 0)[11..], b"new");
}
