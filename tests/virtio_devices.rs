use riscbox::browser_storage::HttpBlockStore;
use riscbox::entropy::{EntropyError, EntropySource};
use riscbox::machine::{Machine, MachineConfig, VIRTIO_BASE};
use riscbox::memory::{AccessWidth, GuestAddress};
use riscbox::virtio_devices::{
    BlockBackend, DeviceError, InputKind, MAX_NETWORK_FRAME_SIZE, MAX_PENDING_NETWORK_FRAMES,
    NetworkBackend, NetworkIngress, NinePBackend, NinePGeneration, NinePOutcome, NinePRequestId,
};
use std::cell::RefCell;
use std::rc::Rc;

const RAM: u64 = 0x8000_0000;
const DESC: u64 = RAM + 0x1000;
const AVAIL: u64 = RAM + 0x2000;
const USED: u64 = RAM + 0x3000;
const DATA: u64 = RAM + 0x4000;

fn test_machine() -> Machine {
    Machine::new(MachineConfig {
        ram_size: 64 << 20,
        framebuffer: None,
    })
    .expect("valid machine")
}

fn machine_write(machine: &mut Machine, address: u64, width: AccessWidth, value: u64) {
    machine
        .bus_mut()
        .write(GuestAddress(address), width, value)
        .expect("guest bus write");
}

fn machine_bytes(machine: &mut Machine, address: u64, data: &[u8]) {
    machine.write_ram(address, data).expect("guest RAM write");
}

fn machine_descriptor(
    machine: &mut Machine,
    index: u16,
    address: u64,
    len: u32,
    flags: u16,
    next: u16,
) {
    let base = DESC + u64::from(index) * 16;
    machine_write(machine, base, AccessWidth::DoubleWord, address);
    machine_write(machine, base + 8, AccessWidth::Word, u64::from(len));
    machine_write(machine, base + 12, AccessWidth::HalfWord, u64::from(flags));
    machine_write(machine, base + 14, AccessWidth::HalfWord, u64::from(next));
}

fn machine_configure(machine: &mut Machine, slot: usize, queue: u16) {
    let base = VIRTIO_BASE
        + u64::try_from(slot)
            .expect("slot fits address")
            .checked_mul(0x1000)
            .expect("slot address fits");
    machine_write(machine, base + 0x30, AccessWidth::Word, u64::from(queue));
    for (register, address) in [(0x80, DESC), (0x90, AVAIL), (0xa0, USED)] {
        machine_write(
            machine,
            base + register,
            AccessWidth::Word,
            address as u32 as u64,
        );
        machine_write(
            machine,
            base + register + 4,
            AccessWidth::Word,
            address >> 32,
        );
    }
    machine_write(machine, base + 0x44, AccessWidth::Word, 1);
    machine_write(machine, base + 0x70, AccessWidth::Word, 4);
}

fn machine_available(machine: &mut Machine, head: u16) {
    machine_write(machine, AVAIL + 2, AccessWidth::HalfWord, 1);
    machine_write(machine, AVAIL + 4, AccessWidth::HalfWord, u64::from(head));
}

fn machine_kick(machine: &mut Machine, slot: usize, queue: u16) {
    let base = VIRTIO_BASE
        + u64::try_from(slot)
            .expect("slot fits address")
            .checked_mul(0x1000)
            .expect("slot address fits");
    machine_write(machine, base + 0x50, AccessWidth::Word, u64::from(queue));
}

fn machine_read(machine: &mut Machine, address: u64, width: AccessWidth) -> u64 {
    machine
        .bus_mut()
        .read(GuestAddress(address), width)
        .expect("guest bus read")
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

#[derive(Clone)]
struct SharedDisk(Rc<RefCell<Vec<u8>>>);

impl BlockBackend for SharedDisk {
    fn capacity_sectors(&self) -> u64 {
        8
    }

    fn read(&mut self, sector: u64, data: &mut [u8]) -> Result<(), DeviceError> {
        let start = usize::try_from(sector).expect("test sector fits usize") * 512;
        data.copy_from_slice(&self.0.borrow()[start..start + data.len()]);
        Ok(())
    }

    fn write(&mut self, sector: u64, data: &[u8]) -> Result<(), DeviceError> {
        let start = usize::try_from(sector).expect("test sector fits usize") * 512;
        self.0.borrow_mut()[start..start + data.len()].copy_from_slice(data);
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
    let mut machine = Machine::new_with_entropy(
        MachineConfig {
            ram_size: 64 << 20,
            framebuffer: None,
        },
        source,
    )
    .expect("valid machine");
    let slot = machine.add_entropy_device().expect("entropy slot");
    machine_configure(&mut machine, slot, 0);
    machine_descriptor(&mut machine, 0, DATA, 5, 2, 0);
    machine_available(&mut machine, 0);
    machine_kick(&mut machine, slot, 0);
    assert_eq!(
        machine.read_ram(DATA, 5).expect("entropy result"),
        &[0x40, 0x41, 0x42, 0x43, 0x44]
    );
    assert_eq!(
        u16::from_le_bytes(
            machine
                .read_ram(USED + 2, 2)
                .expect("used index")
                .try_into()
                .expect("two bytes")
        ),
        1
    );

    machine_write(&mut machine, VIRTIO_BASE + 0x70, AccessWidth::Word, 0);
    machine_configure(&mut machine, slot, 0);
    machine_descriptor(&mut machine, 0, DATA, 1, 0, 0);
    machine_available(&mut machine, 0);
    assert_eq!(
        machine
            .bus_mut()
            .write(GuestAddress(VIRTIO_BASE + 0x50), AccessWidth::Word, 0),
        Err(riscbox::cpu::BusError::AccessFault)
    );
}

#[test]
fn block_reads_writes_identification_and_unsupported_status() {
    let backend = Rc::new(RefCell::new(vec![0x5a; 4096]));
    let mut machine = test_machine();
    let slot = machine
        .add_block_device(
            Box::new(SharedDisk(Rc::clone(&backend))),
            *b"riscbox-disk-0000000",
        )
        .expect("block slot");
    machine_configure(&mut machine, slot, 0);
    let mut header = [0; 16];
    header[8..].copy_from_slice(&1_u64.to_le_bytes());
    machine_bytes(&mut machine, DATA, &header);
    machine_descriptor(&mut machine, 0, DATA, 16, 1, 1);
    machine_descriptor(&mut machine, 1, DATA + 0x100, 513, 2, 0);
    machine_available(&mut machine, 0);
    machine_kick(&mut machine, slot, 0);
    assert_eq!(machine.read_ram(DATA + 0x100, 1).unwrap(), &[0x5a]);
    assert_eq!(machine.read_ram(DATA + 0x300, 1).unwrap(), &[0]);

    machine_write(&mut machine, VIRTIO_BASE + 0x70, AccessWidth::Word, 0);
    machine_configure(&mut machine, slot, 0);
    header[..4].copy_from_slice(&99_u32.to_le_bytes());
    machine_bytes(&mut machine, DATA, &header);
    machine_bytes(&mut machine, DATA + 0x100, &[0xff; 21]);
    machine_descriptor(&mut machine, 0, DATA, 16, 1, 1);
    machine_descriptor(&mut machine, 1, DATA + 0x100, 21, 2, 0);
    machine_available(&mut machine, 0);
    machine_kick(&mut machine, slot, 0);
    assert_eq!(machine.read_ram(DATA + 0x100, 1).unwrap(), &[0xff]);
    assert_eq!(machine.read_ram(DATA + 0x114, 1).unwrap(), &[2]);

    machine_write(&mut machine, VIRTIO_BASE + 0x70, AccessWidth::Word, 0);
    machine_configure(&mut machine, slot, 0);
    header[..4].copy_from_slice(&1_u32.to_le_bytes());
    machine_bytes(&mut machine, DATA, &header);
    machine_bytes(&mut machine, DATA + 0x100, b"write");
    machine_descriptor(&mut machine, 0, DATA, 16, 1, 1);
    machine_descriptor(&mut machine, 1, DATA + 0x100, 512, 1, 2);
    machine_descriptor(&mut machine, 2, DATA + 0x200, 1, 2, 0);
    machine_available(&mut machine, 0);
    machine_kick(&mut machine, slot, 0);
    assert_eq!(&backend.borrow()[512..517], b"write");
    assert_eq!(machine.read_ram(DATA + 0x200, 1).unwrap(), &[0]);

    machine_write(&mut machine, VIRTIO_BASE + 0x70, AccessWidth::Word, 0);
    machine_configure(&mut machine, slot, 0);
    header[..4].copy_from_slice(&8_u32.to_le_bytes());
    machine_bytes(&mut machine, DATA, &header);
    machine_descriptor(&mut machine, 0, DATA, 16, 1, 1);
    machine_descriptor(&mut machine, 1, DATA + 0x100, 21, 2, 0);
    machine_available(&mut machine, 0);
    machine_kick(&mut machine, slot, 0);
    assert_eq!(
        machine.read_ram(DATA + 0x100, 20).unwrap(),
        b"riscbox-disk-0000000"
    );
    assert_eq!(machine.read_ram(DATA + 0x114, 1).unwrap(), &[0]);
}

#[test]
fn http_block_read_waits_for_completion_before_updating_the_used_ring() {
    let store = HttpBlockStore::from_manifest("images/disk.json", "{block_size:1,n_block:1}", 1024)
        .expect("valid manifest");
    let mut machine = test_machine();
    let slot = machine
        .add_http_block_device(store, *b"riscbox-http-0000000")
        .expect("HTTP block slot");
    machine_configure(&mut machine, slot, 0);
    machine_bytes(&mut machine, DATA, &[0; 16]);
    machine_descriptor(&mut machine, 0, DATA, 16, 1, 1);
    machine_descriptor(&mut machine, 1, DATA + 0x100, 513, 2, 0);
    machine_available(&mut machine, 0);
    machine_kick(&mut machine, slot, 0);
    assert_eq!(
        u16::from_le_bytes(machine.read_ram(USED + 2, 2).unwrap().try_into().unwrap()),
        0
    );
    let request = machine
        .next_http_block_request(slot)
        .expect("slot exists")
        .expect("HTTP request");
    assert_eq!(request.url, "images/blk000000000.bin");

    machine_write(&mut machine, VIRTIO_BASE + 0x70, AccessWidth::Word, 0);
    machine
        .complete_http_block_request(slot, request.id, vec![0x6d; 1024])
        .expect("discarded request stays idle");
    assert_eq!(
        u16::from_le_bytes(machine.read_ram(USED + 2, 2).unwrap().try_into().unwrap()),
        0
    );
    machine_configure(&mut machine, slot, 0);
    machine_kick(&mut machine, slot, 0);
    assert_eq!(
        u16::from_le_bytes(machine.read_ram(USED + 2, 2).unwrap().try_into().unwrap()),
        1
    );
    assert_eq!(machine.read_ram(DATA + 0x100, 512).unwrap(), &[0x6d; 512]);
    assert_eq!(machine.read_ram(DATA + 0x300, 1).unwrap(), &[0]);
}

#[test]
fn console_moves_bytes_and_resize_raises_config_interrupt() {
    let mut machine = test_machine();
    let slot = machine.add_console_device(80, 25).expect("console slot");
    machine_configure(&mut machine, slot, 1);
    machine_bytes(&mut machine, DATA, b"ok");
    machine_descriptor(&mut machine, 0, DATA, 2, 0, 0);
    machine_available(&mut machine, 0);
    machine_kick(&mut machine, slot, 1);
    assert_eq!(machine.take_virtio_console_output(slot).unwrap(), b"ok");
    machine.resize_virtio_console(slot, 120, 40).unwrap();
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x100, AccessWidth::HalfWord),
        120
    );
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x102, AccessWidth::HalfWord),
        40
    );
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x60, AccessWidth::Word),
        3
    );

    let mut receive = test_machine();
    let slot = receive.add_console_device(80, 25).expect("console slot");
    machine_configure(&mut receive, slot, 0);
    receive.virtio_console_receive(slot, b"abc").unwrap();
    machine_descriptor(&mut receive, 0, DATA, 3, 2, 0);
    machine_available(&mut receive, 0);
    machine_kick(&mut receive, slot, 0);
    assert_eq!(receive.read_ram(DATA, 3).unwrap(), b"abc");
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

#[derive(Clone, Default)]
struct SharedNet(Rc<RefCell<Vec<Vec<u8>>>>);

impl NetworkBackend for SharedNet {
    fn transmit(&mut self, packet: &[u8]) -> Result<(), DeviceError> {
        self.0.borrow_mut().push(packet.to_vec());
        Ok(())
    }
}

#[test]
fn network_strips_and_adds_the_ten_byte_header() {
    let packets = Rc::new(RefCell::new(Vec::new()));
    let mut machine = test_machine();
    let slot = machine
        .add_network_device(Box::new(SharedNet(Rc::clone(&packets))), [1, 2, 3, 4, 5, 6])
        .expect("network slot");
    machine_configure(&mut machine, slot, 1);
    machine_bytes(&mut machine, DATA, &[0; 10]);
    machine_bytes(&mut machine, DATA + 10, b"frame");
    machine_descriptor(&mut machine, 0, DATA, 15, 0, 0);
    machine_available(&mut machine, 0);
    machine_kick(&mut machine, slot, 1);
    assert_eq!(*packets.borrow(), [b"frame".to_vec()]);
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x100, AccessWidth::Word),
        0x0403_0201
    );

    let mut receive = test_machine();
    let slot = receive
        .add_network_device(Box::new(SharedNet::default()), [0; 6])
        .unwrap();
    machine_configure(&mut receive, slot, 0);
    machine_descriptor(&mut receive, 0, DATA, 15, 2, 0);
    machine_available(&mut receive, 0);
    assert_eq!(
        receive
            .virtio_network_receive(slot, b"frame".to_vec())
            .unwrap(),
        NetworkIngress::Accepted
    );
    assert_eq!(receive.read_ram(DATA + 10, 5).unwrap(), b"frame");
}

#[test]
fn network_carrier_updates_status_and_interrupts_only_on_transitions() {
    let mut machine = test_machine();
    let slot = machine
        .add_network_device(Box::new(Net::default()), [2, 3, 4, 5, 6, 7])
        .unwrap();
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x104, AccessWidth::Word),
        0x0000_0706
    );
    machine.virtio_network_set_carrier(slot, true).unwrap();
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x106, AccessWidth::HalfWord),
        1
    );
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x60, AccessWidth::Word),
        2
    );
    machine.virtio_network_set_carrier(slot, true).unwrap();
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x60, AccessWidth::Word),
        2
    );
    machine_write(&mut machine, VIRTIO_BASE + 0x64, AccessWidth::Word, 2);
    machine.virtio_network_set_carrier(slot, false).unwrap();
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x106, AccessWidth::HalfWord),
        0
    );
    assert_eq!(
        machine_read(&mut machine, VIRTIO_BASE + 0x60, AccessWidth::Word),
        2
    );
}

#[test]
fn network_bounds_ingress_and_reset_discards_pending_frames() {
    let mut machine = test_machine();
    let slot = machine
        .add_network_device(Box::new(Net::default()), [0; 6])
        .unwrap();
    assert_eq!(
        machine.virtio_network_receive(slot, Vec::new()).unwrap(),
        NetworkIngress::Dropped
    );
    assert_eq!(
        machine
            .virtio_network_receive(slot, vec![0; MAX_NETWORK_FRAME_SIZE + 1])
            .unwrap(),
        NetworkIngress::Dropped
    );
    for value in 0..MAX_PENDING_NETWORK_FRAMES {
        assert_eq!(
            machine
                .virtio_network_receive(slot, vec![u8::try_from(value).expect("test byte"); 1])
                .unwrap(),
            NetworkIngress::Accepted
        );
    }
    assert_eq!(
        machine.virtio_network_receive(slot, vec![0; 1]).unwrap(),
        NetworkIngress::Dropped
    );

    machine_write(&mut machine, VIRTIO_BASE + 0x70, AccessWidth::Word, 0);
    assert_eq!(
        machine.virtio_network_receive(slot, vec![0; 1]).unwrap(),
        NetworkIngress::Accepted
    );
}

#[test]
fn network_drops_frames_that_do_not_fit_and_rejects_offload_headers() {
    let mut receive = test_machine();
    let slot = receive
        .add_network_device(Box::new(Net::default()), [0; 6])
        .unwrap();
    machine_configure(&mut receive, slot, 0);
    machine_descriptor(&mut receive, 0, DATA, 12, 2, 0);
    machine_available(&mut receive, 0);
    assert_eq!(
        receive
            .virtio_network_receive(slot, b"frame".to_vec())
            .unwrap(),
        NetworkIngress::Accepted
    );
    assert_eq!(
        u16::from_le_bytes(receive.read_ram(USED + 2, 2).unwrap().try_into().unwrap()),
        1
    );
    assert_eq!(
        u32::from_le_bytes(receive.read_ram(USED + 8, 4).unwrap().try_into().unwrap()),
        0
    );

    let packets = Rc::new(RefCell::new(Vec::new()));
    let mut transmit = test_machine();
    let slot = transmit
        .add_network_device(Box::new(SharedNet(Rc::clone(&packets))), [0; 6])
        .unwrap();
    machine_configure(&mut transmit, slot, 1);
    machine_bytes(&mut transmit, DATA, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    machine_descriptor(&mut transmit, 0, DATA, 10, 0, 0);
    machine_available(&mut transmit, 0);
    assert_eq!(
        transmit
            .bus_mut()
            .write(GuestAddress(VIRTIO_BASE + 0x50), AccessWidth::Word, 1,),
        Err(riscbox::cpu::BusError::AccessFault)
    );
    assert!(packets.borrow().is_empty());
}

#[derive(Clone)]
struct Pending9p {
    requests: Rc<RefCell<Vec<(NinePRequestId, Vec<u8>, u32)>>>,
    generation: Rc<RefCell<NinePGeneration>>,
}

impl Default for Pending9p {
    fn default() -> Self {
        Self {
            requests: Rc::new(RefCell::new(Vec::new())),
            generation: Rc::new(RefCell::new(NinePGeneration(1))),
        }
    }
}

impl NinePBackend for Pending9p {
    fn submit(&mut self, request_id: NinePRequestId, request: Vec<u8>, reply_capacity: u32) {
        self.requests
            .borrow_mut()
            .push((request_id, request, reply_capacity));
    }

    fn reset(&mut self, generation: NinePGeneration) {
        *self.generation.borrow_mut() = generation;
    }
}

fn p9_message(kind: u8, tag: u16) -> [u8; 7] {
    let [tag_low, tag_high] = tag.to_le_bytes();
    [7, 0, 0, 0, kind, tag_low, tag_high]
}

fn pending_p9() -> (Machine, usize, [u8; 7], Pending9p) {
    let backend = Pending9p::default();
    let mut machine = test_machine();
    let slot = machine
        .add_ninep_device(Box::new(backend.clone()), b"root")
        .expect("9p slot");
    machine_configure(&mut machine, slot, 0);
    let request = p9_message(100, 0x1111);
    machine_bytes(&mut machine, DATA, &request);
    machine_descriptor(&mut machine, 0, DATA, 7, 1, 1);
    machine_descriptor(&mut machine, 1, DATA + 0x100, 7, 2, 0);
    machine_available(&mut machine, 0);
    machine_kick(&mut machine, slot, 0);
    (machine, slot, request, backend)
}

#[test]
fn ninep_pending_completion_rejects_malformed_mismatched_and_oversized_replies() {
    for reply in [
        vec![6, 0, 0, 0, 101, 0x11, 0x11],
        p9_message(101, 0x2222).to_vec(),
        [p9_message(101, 0x1111).as_slice(), &[0]].concat(),
    ] {
        let (mut machine, slot, _, backend) = pending_p9();
        let generation = *backend.generation.borrow();
        assert_eq!(
            machine.complete_ninep_transport_request(
                slot,
                generation,
                NinePRequestId(1),
                NinePOutcome::Reply(reply),
            ),
            Err(riscbox::machine::MachineError::Virtio(
                DeviceError::InvalidRequest
            ))
        );
    }

    let (mut machine, slot, _, backend) = pending_p9();
    let generation = *backend.generation.borrow();
    assert_eq!(
        machine.complete_ninep_transport_request(
            slot,
            generation,
            NinePRequestId(1),
            NinePOutcome::EndpointFailure,
        ),
        Err(riscbox::machine::MachineError::Virtio(DeviceError::Backend))
    );
    assert_eq!(
        machine.complete_ninep_transport_request(
            slot,
            generation,
            NinePRequestId(1),
            NinePOutcome::Suppressed,
        ),
        Err(riscbox::machine::MachineError::Virtio(DeviceError::Backend))
    );
}

#[test]
fn ninep_pending_requests_complete_out_of_order_and_reset_retires_generation() {
    let backend = Pending9p::default();
    let mut machine = test_machine();
    let slot = machine
        .add_ninep_device(Box::new(backend.clone()), b"root")
        .unwrap();
    machine_configure(&mut machine, slot, 0);

    let first = p9_message(100, 0x1111);
    let second = p9_message(108, 0x2222);
    machine_bytes(&mut machine, DATA, &first);
    machine_bytes(&mut machine, DATA + 0x200, &second);
    machine_descriptor(&mut machine, 0, DATA, 7, 1, 1);
    machine_descriptor(&mut machine, 1, DATA + 0x100, 7, 2, 0);
    machine_descriptor(&mut machine, 2, DATA + 0x200, 7, 1, 3);
    machine_descriptor(&mut machine, 3, DATA + 0x300, 7, 2, 0);
    machine_write(&mut machine, AVAIL + 2, AccessWidth::HalfWord, 2);
    machine_write(&mut machine, AVAIL + 4, AccessWidth::HalfWord, 0);
    machine_write(&mut machine, AVAIL + 6, AccessWidth::HalfWord, 2);
    machine_kick(&mut machine, slot, 0);

    assert_eq!(
        *backend.requests.borrow(),
        [
            (NinePRequestId(1), first.to_vec(), 7),
            (NinePRequestId(2), second.to_vec(), 7),
        ]
    );
    let generation = *backend.generation.borrow();
    let mut second_reply = second;
    second_reply[4] += 1;
    machine
        .complete_ninep_transport_request(
            slot,
            generation,
            NinePRequestId(2),
            NinePOutcome::Reply(second_reply.to_vec()),
        )
        .unwrap();
    machine
        .complete_ninep_transport_request(
            slot,
            generation,
            NinePRequestId(1),
            NinePOutcome::Suppressed,
        )
        .unwrap();
    assert_eq!(
        u16::from_le_bytes(machine.read_ram(USED + 2, 2).unwrap().try_into().unwrap()),
        2
    );
    assert_eq!(
        u32::from_le_bytes(machine.read_ram(USED + 4, 4).unwrap().try_into().unwrap()),
        2
    );
    assert_eq!(
        u32::from_le_bytes(machine.read_ram(USED + 8, 4).unwrap().try_into().unwrap()),
        7
    );
    assert_eq!(
        u32::from_le_bytes(machine.read_ram(USED + 12, 4).unwrap().try_into().unwrap()),
        0
    );
    assert_eq!(
        u32::from_le_bytes(machine.read_ram(USED + 16, 4).unwrap().try_into().unwrap()),
        0
    );

    assert_eq!(
        machine.complete_ninep_transport_request(
            slot,
            generation,
            NinePRequestId(1),
            NinePOutcome::Suppressed,
        ),
        Err(riscbox::machine::MachineError::Virtio(DeviceError::Backend))
    );
    machine_write(&mut machine, VIRTIO_BASE + 0x70, AccessWidth::Word, 0);
    assert_eq!(*backend.generation.borrow(), NinePGeneration(2));
    assert_eq!(
        machine.complete_ninep_transport_request(
            slot,
            generation,
            NinePRequestId(2),
            NinePOutcome::Suppressed,
        ),
        Ok(())
    );
}

#[test]
fn receive_notifications_drain_buffered_console_network_and_input_data() {
    let mut console = test_machine();
    let slot = console.add_console_device(80, 25).unwrap();
    console.virtio_console_receive(slot, b"x").unwrap();
    machine_configure(&mut console, slot, 0);
    machine_descriptor(&mut console, 0, DATA, 1, 2, 0);
    machine_available(&mut console, 0);
    machine_kick(&mut console, slot, 0);
    assert_eq!(console.read_ram(DATA, 1).unwrap(), b"x");

    let mut network = test_machine();
    let slot = network
        .add_network_device(Box::new(Net::default()), [0; 6])
        .unwrap();
    assert_eq!(
        network
            .virtio_network_receive(slot, b"frame".to_vec())
            .unwrap(),
        NetworkIngress::Accepted
    );
    machine_configure(&mut network, slot, 0);
    machine_descriptor(&mut network, 0, DATA, 15, 2, 0);
    machine_available(&mut network, 0);
    machine_kick(&mut network, slot, 0);
    assert_eq!(network.read_ram(DATA + 10, 5).unwrap(), b"frame");

    let mut input = test_machine();
    let slot = input.add_input_device(InputKind::Keyboard).unwrap();
    machine_configure(&mut input, slot, 0);
    machine_descriptor(&mut input, 0, DATA, 8, 2, 0);
    machine_available(&mut input, 0);
    input.virtio_key_event(slot, 30, true).unwrap();
    assert_eq!(input.read_ram(DATA, 8).unwrap(), &[1, 0, 30, 0, 1, 0, 0, 0]);
}

#[test]
fn ninep_validates_messages_and_input_emits_events() {
    let backend = Pending9p::default();
    let mut p9 = test_machine();
    let p9_slot = p9
        .add_ninep_device(Box::new(backend.clone()), b"root")
        .unwrap();
    machine_configure(&mut p9, p9_slot, 0);
    let request = [7, 0, 0, 0, 100, 0x34, 0x12];
    machine_bytes(&mut p9, DATA, &request);
    machine_descriptor(&mut p9, 0, DATA, 7, 1, 1);
    machine_descriptor(&mut p9, 1, DATA + 0x100, 7, 2, 0);
    machine_available(&mut p9, 0);
    machine_kick(&mut p9, p9_slot, 0);
    let mut reply = request;
    reply[4] += 1;
    p9.complete_ninep_transport_request(
        p9_slot,
        *backend.generation.borrow(),
        NinePRequestId(1),
        NinePOutcome::Reply(reply.to_vec()),
    )
    .unwrap();
    assert_eq!(
        p9.read_ram(DATA + 0x100, 7).unwrap(),
        &[7, 0, 0, 0, 101, 0x34, 0x12]
    );
    assert_eq!(
        machine_read(&mut p9, VIRTIO_BASE + 0x102, AccessWidth::Word),
        u64::from(u32::from_le_bytes(*b"root"))
    );

    let mut input = test_machine();
    let input_slot = input.add_input_device(InputKind::Keyboard).unwrap();
    machine_configure(&mut input, input_slot, 0);
    input.virtio_key_event(input_slot, 30, true).unwrap();
    machine_descriptor(&mut input, 0, DATA, 8, 2, 0);
    machine_available(&mut input, 0);
    machine_kick(&mut input, input_slot, 0);
    assert_eq!(input.read_ram(DATA, 8).unwrap(), &[1, 0, 30, 0, 1, 0, 0, 0]);
    assert_eq!(
        u16::from_le_bytes(input.read_ram(USED + 2, 2).unwrap().try_into().unwrap()),
        1
    );
    machine_write(&mut input, VIRTIO_BASE + 0x100, AccessWidth::Byte, 1);
    assert_eq!(
        machine_read(&mut input, VIRTIO_BASE + 0x102, AccessWidth::Byte),
        15
    );
    machine_write(&mut input, VIRTIO_BASE + 0x100, AccessWidth::Byte, 0x11);
    machine_write(&mut input, VIRTIO_BASE + 0x101, AccessWidth::Byte, 1);
    assert_eq!(
        machine_read(&mut input, VIRTIO_BASE + 0x102, AccessWidth::Byte),
        16
    );
    assert_eq!(
        machine_read(&mut input, VIRTIO_BASE + 0x108, AccessWidth::Byte),
        0xff
    );

    let mut keyboard = test_machine();
    let keyboard_slot = keyboard.add_input_device(InputKind::Keyboard).unwrap();
    machine_configure(&mut keyboard, keyboard_slot, 0);
    machine_descriptor(&mut keyboard, 0, DATA, 8, 2, 0);
    machine_descriptor(&mut keyboard, 1, DATA + 8, 8, 2, 0);
    machine_write(&mut keyboard, AVAIL + 2, AccessWidth::HalfWord, 2);
    machine_write(&mut keyboard, AVAIL + 4, AccessWidth::HalfWord, 0);
    machine_write(&mut keyboard, AVAIL + 6, AccessWidth::HalfWord, 1);
    machine_kick(&mut keyboard, keyboard_slot, 0);
    assert_eq!(
        u16::from_le_bytes(keyboard.read_ram(USED + 2, 2).unwrap().try_into().unwrap()),
        0
    );
    keyboard.virtio_key_event(keyboard_slot, 30, true).unwrap();
    assert_eq!(
        keyboard.read_ram(DATA, 16).unwrap(),
        &[1, 0, 30, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn io_bounds_and_pointer_profiles_are_enforced() {
    let mut block = test_machine();
    let slot = block
        .add_block_device(
            Box::new(Disk {
                data: vec![0; 4096],
            }),
            [0; 20],
        )
        .unwrap();
    machine_configure(&mut block, slot, 0);
    let mut header = [0; 16];
    header[8..].copy_from_slice(&8_u64.to_le_bytes());
    machine_bytes(&mut block, DATA, &header);
    machine_descriptor(&mut block, 0, DATA, 16, 1, 1);
    machine_descriptor(&mut block, 1, DATA + 0x100, 513, 2, 0);
    machine_available(&mut block, 0);
    assert_eq!(
        block
            .bus_mut()
            .write(GuestAddress(VIRTIO_BASE + 0x50), AccessWidth::Word, 0,),
        Err(riscbox::cpu::BusError::AccessFault)
    );

    for (kind, event, size, bits) in [
        (InputKind::Mouse, 2_u64, 2_u64, 3_u64),
        (InputKind::Tablet, 3_u64, 1_u64, 3_u64),
    ] {
        let mut input = test_machine();
        input.add_input_device(kind).unwrap();
        machine_write(&mut input, VIRTIO_BASE + 0x100, AccessWidth::Byte, 0x11);
        machine_write(&mut input, VIRTIO_BASE + 0x101, AccessWidth::Byte, event);
        assert_eq!(
            machine_read(&mut input, VIRTIO_BASE + 0x102, AccessWidth::Byte),
            size
        );
        assert_eq!(
            machine_read(&mut input, VIRTIO_BASE + 0x108, AccessWidth::Byte),
            bits
        );
    }
}
