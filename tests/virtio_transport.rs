use riscbox::guest_memory::{AccessWidth, GuestAddress, MemoryAccess, MemoryError};
use riscbox::tinyemu_core::Core;
use riscbox::virtio::{DescriptorIndex, QueueError, QueueIndex, VERSION_1, VirtioTransport};

const RAM: u64 = 0x8000_0000;
const DESC: u64 = RAM + 0x1000;
const AVAIL: u64 = RAM + 0x2000;
const USED: u64 = RAM + 0x3000;

struct TestRam(Core);

impl TestRam {
    fn new() -> Self {
        let mut core = Core::new().expect("TinyEMU core allocation");
        core.register_ram(RAM, 0x8000, 0)
            .expect("guest RAM registration");
        Self(core)
    }

    fn range(
        &mut self,
        address: GuestAddress,
        len: usize,
        write: bool,
    ) -> Result<&mut [u8], MemoryError> {
        self.0
            .ram_range(address.0, len, write)
            .ok_or(MemoryError::Unmapped(address))
    }
}

impl MemoryAccess for TestRam {
    fn read(&mut self, address: GuestAddress, width: AccessWidth) -> Result<u64, MemoryError> {
        let mut bytes = [0; 8];
        let len = width.bytes();
        bytes[..len].copy_from_slice(self.range(address, len, false)?);
        Ok(u64::from_le_bytes(bytes))
    }

    fn write(
        &mut self,
        address: GuestAddress,
        width: AccessWidth,
        value: u64,
    ) -> Result<(), MemoryError> {
        let len = width.bytes();
        self.range(address, len, true)?
            .copy_from_slice(&value.to_le_bytes()[..len]);
        Ok(())
    }

    fn validate_ram(
        &mut self,
        address: GuestAddress,
        len: usize,
        write: bool,
    ) -> Result<(), MemoryError> {
        self.range(address, len, write).map(|_| ())
    }

    fn read_bytes(&mut self, address: GuestAddress, bytes: &mut [u8]) -> Result<(), MemoryError> {
        bytes.copy_from_slice(self.range(address, bytes.len(), false)?);
        Ok(())
    }

    fn write_bytes(&mut self, address: GuestAddress, bytes: &[u8]) -> Result<(), MemoryError> {
        self.range(address, bytes.len(), true)?
            .copy_from_slice(bytes);
        Ok(())
    }
}

fn memory() -> TestRam {
    TestRam::new()
}

fn transport() -> VirtioTransport {
    VirtioTransport::new(2, 1 << 9, &[8])
}

fn write(memory: &mut TestRam, address: u64, width: AccessWidth, value: u64) {
    MemoryAccess::write(memory, GuestAddress(address), width, value).unwrap();
}

fn descriptor(memory: &mut TestRam, index: u16, address: u64, len: u32, flags: u16, next: u16) {
    let base = DESC + u64::from(index) * 16;
    write(memory, base, AccessWidth::DoubleWord, address);
    write(memory, base + 8, AccessWidth::Word, u64::from(len));
    write(memory, base + 12, AccessWidth::HalfWord, u64::from(flags));
    write(memory, base + 14, AccessWidth::HalfWord, u64::from(next));
}

fn configure(transport: &mut VirtioTransport) {
    transport.write_mmio(0x038, 8, AccessWidth::Word);
    transport.write_mmio(0x080, low(DESC), AccessWidth::Word);
    transport.write_mmio(0x084, (DESC >> 32) as u32, AccessWidth::Word);
    transport.write_mmio(0x090, low(AVAIL), AccessWidth::Word);
    transport.write_mmio(0x094, (AVAIL >> 32) as u32, AccessWidth::Word);
    transport.write_mmio(0x0a0, low(USED), AccessWidth::Word);
    transport.write_mmio(0x0a4, (USED >> 32) as u32, AccessWidth::Word);
    transport.write_mmio(0x044, 1, AccessWidth::Word);
}

fn low(value: u64) -> u32 {
    u32::from_le_bytes(value.to_le_bytes()[..4].try_into().unwrap())
}

#[test]
fn register_negotiation_queue_configuration_and_reset() {
    let mut device = transport();
    assert_eq!(device.read_mmio(0, AccessWidth::Word), 0x7472_6976);
    assert_eq!(device.read_mmio(4, AccessWidth::Word), 2);
    assert_eq!(device.read_mmio(8, AccessWidth::Word), 2);
    assert_eq!(device.read_mmio(0x0c, AccessWidth::Word), 0x554d_4551);
    assert_eq!(device.read_mmio(0x10, AccessWidth::Word), 1 << 9);
    device.write_mmio(0x14, 1, AccessWidth::Word);
    assert_eq!(device.read_mmio(0x10, AccessWidth::Word), 1);
    device.write_mmio(0x24, 1, AccessWidth::Word);
    device.write_mmio(0x20, 1, AccessWidth::Word);
    device.write_mmio(0x24, 0, AccessWidth::Word);
    device.write_mmio(0x20, 1 << 9, AccessWidth::Word);
    assert_eq!(device.driver_features(), VERSION_1 | 1 << 9);

    device.write_mmio(0x20, (1 << 9) | 1, AccessWidth::Word);
    device.write_mmio(0x70, 0x0f, AccessWidth::Word);
    assert_eq!(device.read_mmio(0x70, AccessWidth::Word), 7);

    device.write_mmio(0x30, 7, AccessWidth::Word);
    assert_eq!(device.read_mmio(0x34, AccessWidth::Word), 0);
    device.write_mmio(0x30, 8, AccessWidth::Word);
    assert_eq!(device.read_mmio(0x30, AccessWidth::Word), 7);
    device.write_mmio(0x30, 0, AccessWidth::Word);
    device.write_mmio(0x38, 3, AccessWidth::Word);
    assert_eq!(device.read_mmio(0x38, AccessWidth::Word), 8);
    configure(&mut device);
    assert_eq!(
        device.read_mmio(0x84, AccessWidth::Word),
        (DESC >> 32) as u32
    );
    assert_eq!(device.read_mmio(0x44, AccessWidth::Word), 1);

    device.raise_config_interrupt();
    assert!(device.irq());
    assert_eq!(device.read_mmio(0x60, AccessWidth::Word), 2);
    assert_eq!(device.read_mmio(0xfc, AccessWidth::Word), 1);
    device.write_mmio(0x64, 2, AccessWidth::Word);
    assert!(!device.irq());
    device.write_mmio(0x70, 0, AccessWidth::Word);
    assert_eq!(device.read_mmio(0x38, AccessWidth::Word), 8);
    assert_eq!(device.read_mmio(0x44, AccessWidth::Word), 0);
    assert_eq!(device.driver_features(), 0);
}

#[test]
fn fragmented_chain_copies_and_publishes_used_entry() {
    let mut memory = memory();
    let mut device = transport();
    configure(&mut device);
    descriptor(&mut memory, 2, RAM + 0x4000, 3, 1, 3);
    descriptor(&mut memory, 3, RAM + 0x4fff, 3, 1, 4);
    descriptor(&mut memory, 4, RAM + 0x6000, 5, 2, 0);
    write(&mut memory, RAM + 0x4000, AccessWidth::Word, 0x0063_6261);
    for (offset, byte) in [b'd', b'e', b'f'].into_iter().enumerate() {
        write(
            &mut memory,
            RAM + 0x4fff + offset as u64,
            AccessWidth::Byte,
            u64::from(byte),
        );
    }
    write(&mut memory, AVAIL + 2, AccessWidth::HalfWord, 1);
    write(&mut memory, AVAIL + 4, AccessWidth::HalfWord, 2);

    let chain = device
        .next_chain(&mut memory, QueueIndex(0))
        .unwrap()
        .unwrap();
    assert_eq!(chain.head, DescriptorIndex(2));
    assert_eq!((chain.readable, chain.writable), (6, 5));
    let mut input = [0; 6];
    device
        .read_chain(&mut memory, &chain, 0, &mut input)
        .unwrap();
    assert_eq!(&input, b"abcdef");
    device.write_chain(&mut memory, &chain, 1, b"WXYZ").unwrap();
    assert_eq!(
        memory
            .read(GuestAddress(RAM + 0x6000), AccessWidth::Byte)
            .unwrap(),
        0
    );
    assert_eq!(
        memory.range(GuestAddress(RAM + 0x6001), 4, false).unwrap(),
        b"WXYZ"
    );
    device
        .complete_chain(&mut memory, QueueIndex(0), &chain, 4)
        .unwrap();
    assert_eq!(
        memory
            .read(GuestAddress(USED + 4), AccessWidth::Word)
            .unwrap(),
        2
    );
    assert_eq!(
        memory
            .read(GuestAddress(USED + 8), AccessWidth::Word)
            .unwrap(),
        4
    );
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .unwrap(),
        1
    );
    assert_eq!(device.read_mmio(0x60, AccessWidth::Word), 1);
    assert!(
        device
            .next_chain(&mut memory, QueueIndex(0))
            .unwrap()
            .is_none()
    );
}

#[test]
fn queue_wraparound_and_interrupt_ack() {
    let mut memory = memory();
    let mut device = transport();
    configure(&mut device);
    descriptor(&mut memory, 0, RAM + 0x4000, 1, 2, 0);
    for index in 0..10_u16 {
        write(
            &mut memory,
            AVAIL + 4 + u64::from(index & 7) * 2,
            AccessWidth::HalfWord,
            0,
        );
        write(
            &mut memory,
            AVAIL + 2,
            AccessWidth::HalfWord,
            u64::from(index.wrapping_add(1)),
        );
        let chain = device
            .next_chain(&mut memory, QueueIndex(0))
            .unwrap()
            .unwrap();
        device
            .complete_chain(&mut memory, QueueIndex(0), &chain, 1)
            .unwrap();
    }
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .unwrap(),
        10
    );
    device.raise_config_interrupt();
    assert_eq!(device.read_mmio(0x60, AccessWidth::Word), 3);
    device.write_mmio(0x64, 1, AccessWidth::Word);
    assert!(device.irq());
    device.write_mmio(0x64, 2, AccessWidth::Word);
    assert!(!device.irq());
}

#[test]
fn malformed_queues_and_chains_are_rejected() {
    let mut memory = memory();
    let mut device = transport();
    assert_eq!(
        device.next_chain(&mut memory, QueueIndex(0)),
        Err(QueueError::NotReady)
    );
    assert_eq!(
        device.next_chain(&mut memory, QueueIndex(8)),
        Err(QueueError::InvalidQueue)
    );
    configure(&mut device);

    write(&mut memory, AVAIL + 2, AccessWidth::HalfWord, 1);
    write(&mut memory, AVAIL + 4, AccessWidth::HalfWord, 8);
    assert_eq!(
        device.next_chain(&mut memory, QueueIndex(0)),
        Err(QueueError::InvalidDescriptor)
    );

    device.reset();
    configure(&mut device);
    descriptor(&mut memory, 0, RAM + 0x4000, 1, 1, 0);
    write(&mut memory, AVAIL + 2, AccessWidth::HalfWord, 1);
    write(&mut memory, AVAIL + 4, AccessWidth::HalfWord, 0);
    assert_eq!(
        device.next_chain(&mut memory, QueueIndex(0)),
        Err(QueueError::DescriptorLoop)
    );

    device.reset();
    configure(&mut device);
    descriptor(&mut memory, 0, RAM + 0x4000, 1, 4, 0);
    assert_eq!(
        device.next_chain(&mut memory, QueueIndex(0)),
        Err(QueueError::UnsupportedIndirect)
    );

    device.reset();
    configure(&mut device);
    descriptor(&mut memory, 0, RAM + 0x4000, 1, 3, 1);
    descriptor(&mut memory, 1, RAM + 0x4001, 1, 0, 0);
    assert_eq!(
        device.next_chain(&mut memory, QueueIndex(0)),
        Err(QueueError::DirectionChange)
    );

    device.reset();
    configure(&mut device);
    descriptor(&mut memory, 0, u64::MAX, 2, 0, 0);
    assert_eq!(
        device.next_chain(&mut memory, QueueIndex(0)),
        Err(QueueError::Overflow)
    );

    device.reset();
    configure(&mut device);
    device.write_mmio(0x080, low(DESC + 1), AccessWidth::Word);
    assert_eq!(
        device.next_chain(&mut memory, QueueIndex(0)),
        Err(QueueError::Misaligned)
    );
}
