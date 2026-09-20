use riscbox::browser_storage::HttpBlockStore;
use riscbox::entropy::{EntropyError, EntropySource};
use riscbox::memory::{AccessWidth, GuestAddress, PhysicalMemory, RamFlags};
use riscbox::virtio::VirtioTransport;
use riscbox::virtio_devices::{
    BlockBackend, BlockDevice, ConsoleDevice, DeviceError, EntropyDevice, InputDevice, InputEvent,
    InputKind, MAX_NETWORK_FRAME_SIZE, MAX_PENDING_NETWORK_FRAMES, NetworkBackend, NetworkDevice,
    NetworkIngress, NinePBackend, NinePDevice, NinePGeneration, NinePOutcome, NinePRequestId,
    VirtioMmioDevice,
};
use std::cell::RefCell;
use std::rc::Rc;

const RAM: u64 = 0x8000_0000;
const DESC: u64 = RAM + 0x1000;
const AVAIL: u64 = RAM + 0x2000;
const USED: u64 = RAM + 0x3000;
const DATA: u64 = RAM + 0x4000;

fn test_memory() -> PhysicalMemory {
    let mut memory = PhysicalMemory::new();
    memory
        .register_ram(GuestAddress(RAM), 0x10_000, RamFlags::default())
        .unwrap();
    memory
}

fn write(memory: &mut PhysicalMemory, address: u64, width: AccessWidth, value: u64) {
    memory.write(GuestAddress(address), width, value).unwrap();
}

fn bytes(memory: &mut PhysicalMemory, address: u64, data: &[u8]) {
    for (offset, byte) in data.iter().enumerate() {
        write(
            memory,
            address + offset as u64,
            AccessWidth::Byte,
            u64::from(*byte),
        );
    }
}

fn descriptor(
    memory: &mut PhysicalMemory,
    index: u16,
    address: u64,
    len: u32,
    flags: u16,
    next: u16,
) {
    let base = DESC + u64::from(index) * 16;
    write(memory, base, AccessWidth::DoubleWord, address);
    write(memory, base + 8, AccessWidth::Word, u64::from(len));
    write(memory, base + 12, AccessWidth::HalfWord, u64::from(flags));
    write(memory, base + 14, AccessWidth::HalfWord, u64::from(next));
}

fn configure<D>(device: &mut VirtioMmioDevice<D>, memory: &mut PhysicalMemory, queue: u16)
where
    D: riscbox::virtio_devices::VirtioDevice,
{
    device
        .write(memory, 0x30, u32::from(queue), AccessWidth::Word)
        .unwrap();
    for (register, address) in [(0x80, DESC), (0x90, AVAIL), (0xa0, USED)] {
        device
            .write(
                memory,
                register,
                u32::try_from(address).expect("test addresses fit in u32"),
                AccessWidth::Word,
            )
            .unwrap();
        device
            .write(
                memory,
                register + 4,
                (address >> 32) as u32,
                AccessWidth::Word,
            )
            .unwrap();
    }
    device.write(memory, 0x44, 1, AccessWidth::Word).unwrap();
    device.write(memory, 0x70, 4, AccessWidth::Word).unwrap();
}

fn available(memory: &mut PhysicalMemory, head: u16) {
    write(memory, AVAIL + 2, AccessWidth::HalfWord, 1);
    write(memory, AVAIL + 4, AccessWidth::HalfWord, u64::from(head));
}

#[derive(Default)]
struct Disk {
    data: Vec<u8>,
}
impl BlockBackend for Disk {
    fn capacity_sectors(&self) -> u64 {
        8
    }
    fn read(&mut self, sector: u64, data: &mut [u8]) -> Result<(), DeviceError> {
        let start = usize::try_from(sector).expect("test sector fits usize") * 512;
        data.copy_from_slice(&self.data[start..start + data.len()]);
        Ok(())
    }
    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), DeviceError> {
        let start = usize::try_from(sector).expect("test sector fits usize") * 512;
        self.data[start..start + data.len()].copy_from_slice(data);
        Ok(())
    }
}

struct CountingEntropy(u8);

impl EntropySource for CountingEntropy {
    fn fill(&mut self, destination: &mut [u8]) -> Result<(), EntropyError> {
        for byte in destination {
            *byte = self.0;
            self.0 = self.0.wrapping_add(1);
        }
        Ok(())
    }
}

#[test]
fn entropy_fills_writable_chains_and_rejects_readable_buffers() {
    let source = Rc::new(RefCell::new(CountingEntropy(0x40)));
    let mut device =
        VirtioMmioDevice::new(VirtioTransport::new(4, 0, &[8]), EntropyDevice::new(source));
    let mut memory = test_memory();
    configure(&mut device, &mut memory, 0);
    descriptor(&mut memory, 0, DATA, 5, 2, 0);
    available(&mut memory, 0);
    device
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .expect("entropy request");
    assert_eq!(
        &memory.arena()[0x4000..0x4005],
        &[0x40, 0x41, 0x42, 0x43, 0x44]
    );
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .expect("used index"),
        1
    );

    device.transport.reset();
    configure(&mut device, &mut memory, 0);
    descriptor(&mut memory, 0, DATA, 1, 0, 0);
    available(&mut memory, 0);
    assert_eq!(
        device.write(&mut memory, 0x50, 0, AccessWidth::Word),
        Err(DeviceError::InvalidRequest)
    );
}

#[test]
fn block_reads_writes_identification_and_unsupported_status() {
    let mut memory = test_memory();
    let disk = Disk {
        data: vec![0x5a; 4096],
    };
    let mut device = VirtioMmioDevice::new(
        VirtioTransport::new(2, 0, &[8]),
        BlockDevice::new(disk, *b"riscbox-disk-0000000"),
    );
    configure(&mut device, &mut memory, 0);
    let mut header = [0; 16];
    header[8..].copy_from_slice(&1_u64.to_le_bytes());
    bytes(&mut memory, DATA, &header);
    descriptor(&mut memory, 0, DATA, 16, 1, 1);
    descriptor(&mut memory, 1, DATA + 0x100, 513, 2, 0);
    available(&mut memory, 0);
    device
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(memory.arena()[0x4100], 0x5a);
    assert_eq!(memory.arena()[0x4300], 0);

    device.transport.reset();
    configure(&mut device, &mut memory, 0);
    header[..4].copy_from_slice(&99_u32.to_le_bytes());
    bytes(&mut memory, DATA, &header);
    descriptor(&mut memory, 0, DATA, 16, 1, 1);
    bytes(&mut memory, DATA + 0x100, &[0xff; 21]);
    descriptor(&mut memory, 1, DATA + 0x100, 21, 2, 0);
    available(&mut memory, 0);
    device
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(memory.arena()[0x4100], 0xff);
    assert_eq!(memory.arena()[0x4114], 2);

    device.transport.reset();
    configure(&mut device, &mut memory, 0);
    header[..4].copy_from_slice(&1_u32.to_le_bytes());
    bytes(&mut memory, DATA, &header);
    bytes(&mut memory, DATA + 0x100, b"write");
    descriptor(&mut memory, 0, DATA, 16, 1, 1);
    descriptor(&mut memory, 1, DATA + 0x100, 512, 1, 2);
    descriptor(&mut memory, 2, DATA + 0x200, 1, 2, 0);
    available(&mut memory, 0);
    device
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(&device.device.backend().data[512..517], b"write");
    assert_eq!(memory.arena()[0x4200], 0);

    device.transport.reset();
    configure(&mut device, &mut memory, 0);
    header[..4].copy_from_slice(&8_u32.to_le_bytes());
    bytes(&mut memory, DATA, &header);
    descriptor(&mut memory, 0, DATA, 16, 1, 1);
    descriptor(&mut memory, 1, DATA + 0x100, 21, 2, 0);
    available(&mut memory, 0);
    device
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(&memory.arena()[0x4100..0x4114], b"riscbox-disk-0000000");
    assert_eq!(memory.arena()[0x4114], 0);
    assert_eq!(device.read(0x100, AccessWidth::DoubleWord), 0); // MMIO only exposes up to words
}

#[test]
fn http_block_read_waits_for_completion_before_updating_the_used_ring() {
    let mut memory = test_memory();
    let store = HttpBlockStore::from_manifest("images/disk.json", "{block_size:1,n_block:1}", 1024)
        .expect("valid manifest");
    let mut device = VirtioMmioDevice::new(
        VirtioTransport::new(2, 0, &[8]),
        BlockDevice::new(store, *b"riscbox-http-0000000"),
    );
    configure(&mut device, &mut memory, 0);
    bytes(&mut memory, DATA, &[0; 16]);
    descriptor(&mut memory, 0, DATA, 16, 1, 1);
    descriptor(&mut memory, 1, DATA + 0x100, 513, 2, 0);
    available(&mut memory, 0);

    device
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .expect("queue cache-missing request");
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .unwrap(),
        0
    );
    let request = device
        .device
        .backend_mut()
        .next_request()
        .expect("HTTP request");
    assert_eq!(request.url, "images/blk000000000.bin");

    device
        .write(&mut memory, 0x70, 0, AccessWidth::Word)
        .expect("device reset");

    device
        .device
        .backend_mut()
        .complete(request.id, vec![0x6d; 1024])
        .expect("HTTP completion");
    device
        .device
        .resume(&mut device.transport, &mut memory)
        .expect("discarded request stays idle");
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .unwrap(),
        0
    );
    configure(&mut device, &mut memory, 0);
    device
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .expect("retry cached request");
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .unwrap(),
        1
    );
    assert_eq!(&memory.arena()[0x4100..0x4300], &[0x6d; 512]);
    assert_eq!(memory.arena()[0x4300], 0);
}

#[test]
fn console_moves_bytes_and_resize_raises_config_interrupt() {
    let mut memory = test_memory();
    let mut device = VirtioMmioDevice::new(
        VirtioTransport::new(3, 1, &[8, 8]),
        ConsoleDevice::new(80, 25),
    );
    assert_eq!(
        device.write(&mut memory, 0x50, 0, AccessWidth::Word),
        Err(DeviceError::InvalidRequest)
    );
    configure(&mut device, &mut memory, 1);
    bytes(&mut memory, DATA, b"ok");
    descriptor(&mut memory, 0, DATA, 2, 0, 0);
    available(&mut memory, 0);
    device
        .write(&mut memory, 0x50, 1, AccessWidth::Word)
        .unwrap();
    assert_eq!(device.device.output, b"ok");
    device.device.resize(&mut device.transport, 120, 40);
    assert_eq!(device.read(0x100, AccessWidth::HalfWord), 120);
    assert_eq!(device.read(0x102, AccessWidth::HalfWord), 40);
    assert!(device.transport.irq());

    let mut memory = test_memory();
    let mut receive = VirtioMmioDevice::new(
        VirtioTransport::new(3, 1, &[8, 8]),
        ConsoleDevice::new(80, 25),
    );
    configure(&mut receive, &mut memory, 0);
    descriptor(&mut memory, 0, DATA, 3, 2, 0);
    available(&mut memory, 0);
    receive
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .unwrap(),
        0
    );
    receive
        .device
        .receive(&mut receive.transport, &mut memory, b"abc")
        .unwrap();
    assert_eq!(&memory.arena()[0x4000..0x4003], b"abc");
}

#[derive(Default)]
struct Net {
    packets: Vec<Vec<u8>>,
}
impl NetworkBackend for Net {
    fn transmit(&mut self, packet: &[u8]) -> Result<(), DeviceError> {
        self.packets.push(packet.to_vec());
        Ok(())
    }
}

#[test]
fn network_strips_and_adds_the_ten_byte_header() {
    let mut memory = test_memory();
    let mut device = VirtioMmioDevice::new(
        VirtioTransport::new(1, 0, &[8, 8]),
        NetworkDevice::new(Net::default(), [1, 2, 3, 4, 5, 6]),
    );
    configure(&mut device, &mut memory, 1);
    bytes(&mut memory, DATA, &[0; 10]);
    bytes(&mut memory, DATA + 10, b"frame");
    descriptor(&mut memory, 0, DATA, 15, 0, 0);
    available(&mut memory, 0);
    device
        .write(&mut memory, 0x50, 1, AccessWidth::Word)
        .unwrap();
    assert_eq!(device.device.backend().packets, [b"frame".to_vec()]);
    assert_eq!(device.read(0x100, AccessWidth::Word), 0x0403_0201);

    let mut memory = test_memory();
    let mut receive = VirtioMmioDevice::new(
        VirtioTransport::new(1, 0, &[8, 8]),
        NetworkDevice::new(Net::default(), [0; 6]),
    );
    configure(&mut receive, &mut memory, 0);
    descriptor(&mut memory, 0, DATA, 15, 2, 0);
    available(&mut memory, 0);
    receive
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .unwrap(),
        0
    );
    receive
        .device
        .receive_packet(&mut receive.transport, &mut memory, b"frame".to_vec())
        .unwrap();
    assert_eq!(&memory.arena()[0x400a..0x400f], b"frame");
}

#[test]
fn network_carrier_updates_status_and_interrupts_only_on_transitions() {
    let mut memory = test_memory();
    let mut device = VirtioMmioDevice::new(
        VirtioTransport::new(1, (1 << 5) | (1 << 16), &[8, 8]),
        NetworkDevice::new(Net::default(), [2, 3, 4, 5, 6, 7]),
    );
    assert_eq!(device.read(0x104, AccessWidth::Word), 0x0000_0706);
    assert_eq!(device.read(0x0fc, AccessWidth::Word), 0);

    device.device.set_carrier(&mut device.transport, true);
    assert!(device.device.carrier_is_up());
    assert_eq!(device.read(0x106, AccessWidth::HalfWord), 1);
    assert_eq!(device.read(0x060, AccessWidth::Word), 2);
    assert_eq!(device.read(0x0fc, AccessWidth::Word), 1);
    device.device.set_carrier(&mut device.transport, true);
    assert_eq!(device.read(0x0fc, AccessWidth::Word), 1);

    device
        .write(&mut memory, 0x064, 2, AccessWidth::Word)
        .expect("interrupt acknowledgement");
    device.device.set_carrier(&mut device.transport, false);
    assert_eq!(device.read(0x106, AccessWidth::HalfWord), 0);
    assert_eq!(device.read(0x0fc, AccessWidth::Word), 2);
}

#[test]
fn network_bounds_ingress_and_reset_discards_pending_frames() {
    let mut memory = test_memory();
    let mut device = VirtioMmioDevice::new(
        VirtioTransport::new(1, 0, &[8, 8]),
        NetworkDevice::new(Net::default(), [0; 6]),
    );
    assert_eq!(
        device
            .device
            .receive_packet(&mut device.transport, &mut memory, Vec::new()),
        Ok(NetworkIngress::Dropped)
    );
    assert_eq!(
        device.device.receive_packet(
            &mut device.transport,
            &mut memory,
            vec![0; MAX_NETWORK_FRAME_SIZE + 1],
        ),
        Ok(NetworkIngress::Dropped)
    );
    for value in 0..MAX_PENDING_NETWORK_FRAMES {
        assert_eq!(
            device.device.receive_packet(
                &mut device.transport,
                &mut memory,
                vec![u8::try_from(value).expect("test byte"); 1],
            ),
            Ok(NetworkIngress::Accepted)
        );
    }
    assert_eq!(device.device.pending_frames(), MAX_PENDING_NETWORK_FRAMES);
    assert_eq!(
        device
            .device
            .receive_packet(&mut device.transport, &mut memory, vec![0; 1]),
        Ok(NetworkIngress::Dropped)
    );

    device
        .write(&mut memory, 0x070, 0, AccessWidth::Word)
        .expect("device reset");
    assert_eq!(device.device.pending_frames(), 0);
}

#[test]
fn network_drops_frames_that_do_not_fit_and_rejects_offload_headers() {
    let mut memory = test_memory();
    let mut receive = VirtioMmioDevice::new(
        VirtioTransport::new(1, 0, &[8, 8]),
        NetworkDevice::new(Net::default(), [0; 6]),
    );
    configure(&mut receive, &mut memory, 0);
    descriptor(&mut memory, 0, DATA, 12, 2, 0);
    available(&mut memory, 0);
    assert_eq!(
        receive
            .device
            .receive_packet(&mut receive.transport, &mut memory, b"frame".to_vec(),),
        Ok(NetworkIngress::Accepted)
    );
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .expect("used index"),
        1
    );
    assert_eq!(
        memory
            .read(GuestAddress(USED + 8), AccessWidth::Word)
            .expect("used length"),
        0
    );

    let mut transmit = VirtioMmioDevice::new(
        VirtioTransport::new(1, 0, &[8, 8]),
        NetworkDevice::new(Net::default(), [0; 6]),
    );
    configure(&mut transmit, &mut memory, 1);
    bytes(&mut memory, DATA, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    descriptor(&mut memory, 0, DATA, 10, 0, 0);
    available(&mut memory, 0);
    assert_eq!(
        transmit.write(&mut memory, 0x50, 1, AccessWidth::Word),
        Err(DeviceError::InvalidRequest)
    );
    assert!(transmit.device.backend().packets.is_empty());
}

#[derive(Default)]
struct Pending9p {
    requests: Vec<(NinePRequestId, Vec<u8>, u32)>,
}

impl NinePBackend for Pending9p {
    fn submit(&mut self, request_id: NinePRequestId, request: Vec<u8>, reply_capacity: u32) {
        self.requests.push((request_id, request, reply_capacity));
    }
}

fn p9_message(kind: u8, tag: u16) -> [u8; 7] {
    let [tag_low, tag_high] = tag.to_le_bytes();
    [7, 0, 0, 0, kind, tag_low, tag_high]
}

fn pending_p9() -> (
    PhysicalMemory,
    VirtioMmioDevice<NinePDevice<Pending9p>>,
    [u8; 7],
) {
    let mut memory = test_memory();
    let mut p9 = VirtioMmioDevice::new(
        VirtioTransport::new(9, 1, &[8]),
        NinePDevice::new(Pending9p::default(), b"root"),
    );
    configure(&mut p9, &mut memory, 0);
    let request = p9_message(100, 0x1111);
    bytes(&mut memory, DATA, &request);
    descriptor(&mut memory, 0, DATA, 7, 1, 1);
    descriptor(&mut memory, 1, DATA + 0x100, 7, 2, 0);
    available(&mut memory, 0);
    p9.write(&mut memory, 0x50, 0, AccessWidth::Word).unwrap();
    (memory, p9, request)
}

#[test]
fn ninep_pending_completion_rejects_malformed_mismatched_and_oversized_replies() {
    for reply in [
        vec![6, 0, 0, 0, 101, 0x11, 0x11],
        p9_message(101, 0x2222).to_vec(),
        [p9_message(101, 0x1111).as_slice(), &[0]].concat(),
    ] {
        let (mut memory, mut p9, _) = pending_p9();
        let generation = p9.device.generation();
        assert_eq!(
            p9.device.complete(
                &mut p9.transport,
                &mut memory,
                generation,
                NinePRequestId(1),
                NinePOutcome::Reply(reply),
            ),
            Err(DeviceError::InvalidRequest)
        );
    }

    let (mut memory, mut p9, _) = pending_p9();
    let generation = p9.device.generation();
    assert_eq!(
        p9.device.complete(
            &mut p9.transport,
            &mut memory,
            generation,
            NinePRequestId(1),
            NinePOutcome::EndpointFailure,
        ),
        Err(DeviceError::Backend)
    );
    assert_eq!(
        p9.device.complete(
            &mut p9.transport,
            &mut memory,
            generation,
            NinePRequestId(1),
            NinePOutcome::Suppressed,
        ),
        Err(DeviceError::Backend)
    );
}

#[test]
fn ninep_pending_requests_complete_out_of_order_and_reset_retires_generation() {
    let mut memory = test_memory();
    let mut p9 = VirtioMmioDevice::new(
        VirtioTransport::new(9, 1, &[8]),
        NinePDevice::new(Pending9p::default(), b"root"),
    );
    configure(&mut p9, &mut memory, 0);

    let first = p9_message(100, 0x1111);
    let second = p9_message(108, 0x2222);
    bytes(&mut memory, DATA, &first);
    bytes(&mut memory, DATA + 0x200, &second);
    descriptor(&mut memory, 0, DATA, 7, 1, 1);
    descriptor(&mut memory, 1, DATA + 0x100, 7, 2, 0);
    descriptor(&mut memory, 2, DATA + 0x200, 7, 1, 3);
    descriptor(&mut memory, 3, DATA + 0x300, 7, 2, 0);
    write(&mut memory, AVAIL + 2, AccessWidth::HalfWord, 2);
    write(&mut memory, AVAIL + 4, AccessWidth::HalfWord, 0);
    write(&mut memory, AVAIL + 6, AccessWidth::HalfWord, 2);
    p9.write(&mut memory, 0x50, 0, AccessWidth::Word).unwrap();

    assert_eq!(
        p9.device.backend_mut().requests,
        [
            (NinePRequestId(1), first.to_vec(), 7),
            (NinePRequestId(2), second.to_vec(), 7),
        ]
    );
    let generation = p9.device.generation();
    let mut second_reply = second;
    second_reply[4] += 1;
    p9.device
        .complete(
            &mut p9.transport,
            &mut memory,
            generation,
            NinePRequestId(2),
            NinePOutcome::Reply(second_reply.to_vec()),
        )
        .unwrap();
    p9.device
        .complete(
            &mut p9.transport,
            &mut memory,
            generation,
            NinePRequestId(1),
            NinePOutcome::Suppressed,
        )
        .unwrap();
    assert_eq!(
        memory.read(GuestAddress(USED + 2), AccessWidth::HalfWord),
        Ok(2)
    );
    assert_eq!(
        memory.read(GuestAddress(USED + 4), AccessWidth::Word),
        Ok(2)
    );
    assert_eq!(
        memory.read(GuestAddress(USED + 8), AccessWidth::Word),
        Ok(7)
    );
    assert_eq!(
        memory.read(GuestAddress(USED + 12), AccessWidth::Word),
        Ok(0)
    );
    assert_eq!(
        memory.read(GuestAddress(USED + 16), AccessWidth::Word),
        Ok(0)
    );

    assert_eq!(
        p9.device.complete(
            &mut p9.transport,
            &mut memory,
            generation,
            NinePRequestId(1),
            NinePOutcome::Suppressed,
        ),
        Err(DeviceError::Backend)
    );
    p9.write(&mut memory, 0x70, 0, AccessWidth::Word).unwrap();
    assert_eq!(p9.device.generation(), NinePGeneration(2));
    assert_eq!(
        p9.device.complete(
            &mut p9.transport,
            &mut memory,
            generation,
            NinePRequestId(2),
            NinePOutcome::Suppressed,
        ),
        Ok(())
    );
}

#[test]
fn receive_notifications_drain_buffered_console_network_and_input_data() {
    let mut memory = test_memory();
    let mut console = VirtioMmioDevice::new(
        VirtioTransport::new(3, 0, &[8, 8]),
        ConsoleDevice::new(80, 25),
    );
    configure(&mut console, &mut memory, 0);
    console
        .device
        .receive(&mut console.transport, &mut memory, b"x")
        .unwrap();
    descriptor(&mut memory, 0, DATA, 1, 2, 0);
    available(&mut memory, 0);
    console
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(memory.arena()[0x4000], b'x');

    let mut memory = test_memory();
    let mut network = VirtioMmioDevice::new(
        VirtioTransport::new(1, 0, &[8, 8]),
        NetworkDevice::new(Net::default(), [0; 6]),
    );
    configure(&mut network, &mut memory, 0);
    network
        .device
        .receive_packet(&mut network.transport, &mut memory, b"frame".to_vec())
        .unwrap();
    descriptor(&mut memory, 0, DATA, 15, 2, 0);
    available(&mut memory, 0);
    network
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(&memory.arena()[0x400a..0x400f], b"frame");

    let mut memory = test_memory();
    let mut input = VirtioMmioDevice::new(
        VirtioTransport::new(18, 0, &[8, 8]),
        InputDevice::new(InputKind::Keyboard),
    );
    configure(&mut input, &mut memory, 0);
    input
        .device
        .send_key(&mut input.transport, &mut memory, 30, true)
        .unwrap();
    descriptor(&mut memory, 0, DATA, 8, 2, 0);
    available(&mut memory, 0);
    input
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(memory.arena()[0x4002], 30);
}

#[test]
fn ninep_validates_messages_and_input_emits_events() {
    let mut memory = test_memory();
    let mut p9 = VirtioMmioDevice::new(
        VirtioTransport::new(9, 1, &[8]),
        NinePDevice::new(Pending9p::default(), b"root"),
    );
    configure(&mut p9, &mut memory, 0);
    let request = [7, 0, 0, 0, 100, 0x34, 0x12];
    bytes(&mut memory, DATA, &request);
    descriptor(&mut memory, 0, DATA, 7, 1, 1);
    descriptor(&mut memory, 1, DATA + 0x100, 7, 2, 0);
    available(&mut memory, 0);
    p9.write(&mut memory, 0x50, 0, AccessWidth::Word).unwrap();
    let mut reply = request;
    reply[4] += 1;
    p9.device
        .complete(
            &mut p9.transport,
            &mut memory,
            p9.device.generation(),
            NinePRequestId(1),
            NinePOutcome::Reply(reply.to_vec()),
        )
        .unwrap();
    assert_eq!(
        &memory.arena()[0x4100..0x4107],
        &[7, 0, 0, 0, 101, 0x34, 0x12]
    );
    assert_eq!(
        p9.read(0x102, AccessWidth::Word),
        u32::from_le_bytes(*b"root")
    );

    let mut memory = test_memory();
    let mut input = VirtioMmioDevice::new(
        VirtioTransport::new(18, 0, &[8, 8]),
        InputDevice::new(InputKind::Keyboard),
    );
    configure(&mut input, &mut memory, 0);
    input.device.push_event(InputEvent {
        kind: 1,
        code: 30,
        value: 1,
    });
    descriptor(&mut memory, 0, DATA, 8, 2, 0);
    available(&mut memory, 0);
    input
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(&memory.arena()[0x4000..0x4008], &[1, 0, 30, 0, 1, 0, 0, 0]);
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .unwrap(),
        1
    );
    input
        .write(&mut memory, 0x100, 1, AccessWidth::Byte)
        .unwrap();
    assert_eq!(input.read(0x102, AccessWidth::Byte), 15);
    input
        .write(&mut memory, 0x100, 0x11, AccessWidth::Byte)
        .unwrap();
    input
        .write(&mut memory, 0x101, 1, AccessWidth::Byte)
        .unwrap();
    assert_eq!(input.read(0x102, AccessWidth::Byte), 16);
    assert_eq!(input.read(0x108, AccessWidth::Byte), 0xff);

    let mut memory = test_memory();
    let mut keyboard = VirtioMmioDevice::new(
        VirtioTransport::new(18, 0, &[8, 8]),
        InputDevice::new(riscbox::virtio_devices::InputKind::Keyboard),
    );
    configure(&mut keyboard, &mut memory, 0);
    descriptor(&mut memory, 0, DATA, 8, 2, 0);
    descriptor(&mut memory, 1, DATA + 8, 8, 2, 0);
    write(&mut memory, AVAIL + 2, AccessWidth::HalfWord, 2);
    write(&mut memory, AVAIL + 4, AccessWidth::HalfWord, 0);
    write(&mut memory, AVAIL + 6, AccessWidth::HalfWord, 1);
    keyboard
        .write(&mut memory, 0x50, 0, AccessWidth::Word)
        .unwrap();
    assert_eq!(
        memory
            .read(GuestAddress(USED + 2), AccessWidth::HalfWord)
            .unwrap(),
        0
    );
    keyboard
        .device
        .send_key(&mut keyboard.transport, &mut memory, 30, true)
        .unwrap();
    assert_eq!(&memory.arena()[0x4000..0x4008], &[1, 0, 30, 0, 1, 0, 0, 0]);
    assert_eq!(&memory.arena()[0x4008..0x4010], &[0; 8]);
}

#[test]
fn io_bounds_and_pointer_profiles_are_enforced() {
    let mut memory = test_memory();
    let disk = Disk {
        data: vec![0; 4096],
    };
    let mut block = VirtioMmioDevice::new(
        VirtioTransport::new(2, 0, &[8]),
        BlockDevice::new(disk, [0; 20]),
    );
    configure(&mut block, &mut memory, 0);
    let mut header = [0; 16];
    header[8..].copy_from_slice(&8_u64.to_le_bytes());
    bytes(&mut memory, DATA, &header);
    descriptor(&mut memory, 0, DATA, 16, 1, 1);
    descriptor(&mut memory, 1, DATA + 0x100, 513, 2, 0);
    available(&mut memory, 0);
    assert_eq!(
        block.write(&mut memory, 0x50, 0, AccessWidth::Word),
        Err(DeviceError::InvalidRequest)
    );

    for (kind, event, size, bits) in [(InputKind::Mouse, 2, 2, 3), (InputKind::Tablet, 3, 1, 3)] {
        let mut input =
            VirtioMmioDevice::new(VirtioTransport::new(18, 0, &[8, 8]), InputDevice::new(kind));
        input
            .write(&mut memory, 0x100, 0x11, AccessWidth::Byte)
            .unwrap();
        input
            .write(&mut memory, 0x101, event, AccessWidth::Byte)
            .unwrap();
        assert_eq!(input.read(0x102, AccessWidth::Byte), size);
        assert_eq!(input.read(0x108, AccessWidth::Byte), bits);
    }
}
