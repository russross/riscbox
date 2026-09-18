#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "cutils.h"

#include "../virtio.c"

#define CHECK(condition) do {                                                \
        if (!(condition)) {                                                  \
            fprintf(stderr, "%s:%d: check failed: %s\n",                    \
                    __FILE__, __LINE__, #condition);                         \
            exit(1);                                                         \
        }                                                                    \
    } while (0)

#define RAM_SIZE 0x10000

typedef struct {
    uint8_t ram[RAM_SIZE];
    int irq_level;
    int irq_changes;
    int recv_count;
    int read_size;
    int write_size;
} TestState;

static TestState *test_state;

static uint8_t *test_ram_ptr(VIRTIODevice *s, virtio_phys_addr_t addr,
                             BOOL is_rw)
{
    (void)s;
    (void)is_rw;
    if (addr >= RAM_SIZE)
        return NULL;
    return test_state->ram + addr;
}

static void test_set_irq(void *opaque, int irq_num, int level)
{
    TestState *state = opaque;

    CHECK(irq_num == 1);
    state->irq_level = level;
    state->irq_changes++;
}

static int test_recv(VIRTIODevice *s, int queue_idx, int desc_idx,
                     int read_size, int write_size)
{
    (void)s;
    CHECK(queue_idx == 0);
    CHECK(desc_idx == 0);
    test_state->recv_count++;
    test_state->read_size = read_size;
    test_state->write_size = write_size;
    return 0;
}

static void init_device(VIRTIODevice *dev, TestState *state, IRQSignal *irq,
                        uint32_t device_id, VIRTIODeviceRecvFunc *recv)
{
    int i;

    memset(state, 0, sizeof(*state));
    memset(dev, 0, sizeof(*dev));
    test_state = state;
    irq_init(irq, test_set_irq, state, 1);
    dev->irq = irq;
    dev->get_ram_ptr = test_ram_ptr;
    dev->device_id = device_id;
    dev->vendor_id = UINT32_C(0x554d4551);
    dev->device_recv = recv;
    for (i = 0; i < MAX_QUEUE; i++)
        dev->queue[i].num_max = MAX_QUEUE_NUM;
    virtio_reset(dev);
}

static void put_desc(TestState *state, int index, uint64_t addr, uint32_t len,
                     uint16_t flags, uint16_t next)
{
    uint8_t *desc = state->ram + 0x1000 + index * 16;

    put_le64(desc, addr);
    put_le32(desc + 8, len);
    put_le16(desc + 12, flags);
    put_le16(desc + 14, next);
}

static void configure_queue(VIRTIODevice *dev, int queue, int count)
{
    QueueState *qs = &dev->queue[queue];

    qs->num = count;
    qs->desc_addr = 0x1000;
    qs->avail_addr = 0x2000;
    qs->used_addr = 0x3000;
    qs->ready = 1;
}

static void make_available(TestState *state, int slot, uint16_t desc_index)
{
    put_le16(state->ram + 0x2000 + 4 + slot * 2, desc_index);
    put_le16(state->ram + 0x2002, slot + 1);
}

static void test_transport_and_queue(void)
{
    TestState state;
    IRQSignal irq;
    VIRTIODevice dev;

    init_device(&dev, &state, &irq, 3, test_recv);
    CHECK(virtio_mmio_read(&dev, VIRTIO_MMIO_MAGIC_VALUE, 2) == 0x74726976);
    CHECK(virtio_mmio_read(&dev, VIRTIO_MMIO_VERSION, 2) == 2);
    CHECK(virtio_mmio_read(&dev, VIRTIO_MMIO_DEVICE_ID, 2) == 3);
    CHECK(virtio_mmio_read(&dev, VIRTIO_MMIO_VENDOR_ID, 2) == 0x554d4551);
    configure_queue(&dev, 0, 8);
    put_desc(&state, 0, 0x4000, 5, VRING_DESC_F_NEXT, 1);
    put_desc(&state, 1, 0x5000, 7, VRING_DESC_F_WRITE, 0);
    make_available(&state, 0, 0);

    dev.status = 4;
    dev.queue[0].ready = 0;
    queue_notify(&dev, 0);
    CHECK(state.recv_count == 0);
    dev.queue[0].ready = 1;
    dev.status = 0;
    queue_notify(&dev, 0);
    CHECK(state.recv_count == 0);
    dev.status = 4;
    queue_notify(&dev, 0);
    CHECK(state.recv_count == 1);
    CHECK(state.read_size == 5 && state.write_size == 7);

    virtio_consume_desc(&dev, 0, 0, 7);
    CHECK(get_le16(state.ram + 0x3002) == 1);
    CHECK(get_le32(state.ram + 0x3004) == 0);
    CHECK(get_le32(state.ram + 0x3008) == 7);
    CHECK(state.irq_level == 1 && dev.int_status == 1);
    virtio_mmio_write(&dev, VIRTIO_MMIO_INTERRUPT_ACK, 1, 2);
    CHECK(state.irq_level == 0 && dev.int_status == 0);

    put_desc(&state, 0, 0x4000, 1, VRING_DESC_F_NEXT, 0);
    CHECK(get_desc_rw_size(&dev, &state.read_size, &state.write_size, 0, 0) < 0);
    CHECK(get_desc_rw_size(&dev, &state.read_size, &state.write_size, 0, 8) < 0);

    virtio_mmio_write(&dev, VIRTIO_MMIO_STATUS, 0, 2);
    CHECK(dev.queue[0].ready == 0 && dev.queue[0].desc_addr == 0);
}

typedef struct {
    uint8_t data[32];
    int len;
} ConsoleLog;

static void console_write(void *opaque, const uint8_t *buf, int len)
{
    ConsoleLog *log = opaque;

    CHECK(len <= (int)sizeof(log->data));
    memcpy(log->data, buf, len);
    log->len = len;
}

static void test_console(void)
{
    TestState state;
    IRQSignal irq;
    VIRTIOConsoleDevice console;
    ConsoleLog log = { { 0 }, 0 };
    CharacterDevice chr = { &log, console_write, NULL };
    static const uint8_t output[] = { 'h', 'e', 'l', 'l', 'o' };
    static const uint8_t input[] = { 'a', 'b', 'c', 'd', 'e' };

    init_device(&console.common, &state, &irq, 3,
                virtio_console_recv_request);
    console.cs = &chr;
    configure_queue(&console.common, 1, 8);
    memcpy(state.ram + 0x4000, output, sizeof(output));
    put_desc(&state, 0, 0x4000, sizeof(output), 0, 0);
    make_available(&state, 0, 0);
    console.common.status = 4;
    queue_notify(&console.common, 1);
    CHECK(log.len == 5 && memcmp(log.data, output, sizeof(output)) == 0);

    memset(state.ram + 0x2000, 0, 16);
    memset(state.ram + 0x3000, 0, 16);
    configure_queue(&console.common, 0, 8);
    console.common.queue[0].manual_recv = TRUE;
    put_desc(&state, 0, 0x5000, 3, VRING_DESC_F_WRITE, 0);
    make_available(&state, 0, 0);
    CHECK(virtio_console_get_write_len(&console.common) == 3);
    CHECK(virtio_console_write_data(&console.common, input, sizeof(input)) == 3);
    CHECK(memcmp(state.ram + 0x5000, input, 3) == 0);
    CHECK(get_le32(state.ram + 0x3008) == 3);

    virtio_console_resize_event(&console.common, 80, 25);
    CHECK(get_le16(console.common.config_space) == 80);
    CHECK(get_le16(console.common.config_space + 2) == 25);
    CHECK(console.common.int_status & 2);
}

static void test_block_status(void)
{
    TestState state;
    IRQSignal irq;
    VIRTIOBlockDevice block;
    BlockRequestHeader header = { UINT32_C(0xdeadbeef), 0, 0 };

    init_device(&block.common, &state, &irq, 2, virtio_block_recv_request);
    configure_queue(&block.common, 0, 8);
    memcpy(state.ram + 0x4000, &header, sizeof(header));
    put_desc(&state, 0, 0x4000, sizeof(header), VRING_DESC_F_NEXT, 1);
    put_desc(&state, 1, 0x5000, 1, VRING_DESC_F_WRITE, 0);
    make_available(&state, 0, 0);
    block.common.status = 4;
    queue_notify(&block.common, 0);
    CHECK(state.ram[0x5000] == VIRTIO_BLK_S_UNSUPP);
    CHECK(get_le32(state.ram + 0x3008) == 1);
}

typedef struct {
    EthernetDevice net;
    uint8_t packet[32];
    int packet_len;
} NetLog;

static void net_write(EthernetDevice *net, const uint8_t *buf, int len)
{
    NetLog *log = (NetLog *)net;

    memcpy(log->packet, buf, len);
    log->packet_len = len;
}

static void test_network(void)
{
    TestState state;
    IRQSignal irq;
    VIRTIONetDevice device;
    NetLog log = { 0 };
    static const uint8_t packet[] = { 1, 2, 3, 4 };

    init_device(&device.common, &state, &irq, 1, virtio_net_recv_request);
    log.net.write_packet = net_write;
    device.es = &log.net;
    device.header_size = sizeof(VIRTIONetHeader);
    configure_queue(&device.common, 1, 8);
    CHECK(device.header_size == 10);
    memset(state.ram + 0x4000, 0, device.header_size);
    memcpy(state.ram + 0x4000 + device.header_size, packet, sizeof(packet));
    put_desc(&state, 0, 0x4000, device.header_size + sizeof(packet), 0, 0);
    make_available(&state, 0, 0);
    device.common.status = 4;
    queue_notify(&device.common, 1);
    CHECK(log.packet_len == 4);
    CHECK(memcmp(log.packet, packet, sizeof(packet)) == 0);
}

static void test_input(void)
{
    TestState state;
    IRQSignal irq;
    VIRTIOInputDevice input;

    init_device(&input.common, &state, &irq, 18, virtio_input_recv_request);
    input.type = VIRTIO_INPUT_TYPE_KEYBOARD;
    configure_queue(&input.common, 0, 8);
    input.common.queue[0].manual_recv = TRUE;
    put_desc(&state, 0, 0x4000, 8, VRING_DESC_F_WRITE, 0);
    put_desc(&state, 1, 0x4010, 8, VRING_DESC_F_WRITE, 0);
    put_le16(state.ram + 0x2004, 0);
    put_le16(state.ram + 0x2006, 1);
    put_le16(state.ram + 0x2002, 2);
    CHECK(virtio_input_send_key_event(&input.common, TRUE, 30) == 0);
    CHECK(get_le16(state.ram + 0x4000) == VIRTIO_INPUT_EV_KEY);
    CHECK(get_le16(state.ram + 0x4002) == 30);
    CHECK(get_le32(state.ram + 0x4004) == 1);
    CHECK(get_le16(state.ram + 0x4010) == VIRTIO_INPUT_EV_SYN);
    CHECK(get_le16(state.ram + 0x3002) == 2);
}

typedef struct {
    P9Server server;
    int requests;
} TestP9Server;

static int p9_request(P9Server *server, const uint8_t *request,
                      size_t request_size, uint8_t *reply,
                      size_t reply_capacity)
{
    TestP9Server *test = (TestP9Server *)server;

    CHECK(request_size == 7);
    CHECK(reply_capacity >= 7);
    test->requests++;
    put_le32(reply, 7);
    reply[4] = request[4] + 1;
    put_le16(reply + 5, get_le16(request + 5));
    return 7;
}

static void test_9p_protocol(void)
{
    TestState state;
    IRQSignal irq;
    VIRTIO9PProtocolDevice device;
    TestP9Server server = { { p9_request, NULL }, 0 };
    uint8_t request[7];

    init_device(&device.common, &state, &irq, 9,
                virtio_9p_protocol_recv_request);
    device.server = &server.server;
    configure_queue(&device.common, 0, 8);
    put_le32(request, sizeof(request));
    request[4] = 100;
    put_le16(request + 5, 42);
    memcpy(state.ram + 0x4000, request, sizeof(request));
    put_desc(&state, 0, 0x4000, sizeof(request), VRING_DESC_F_NEXT, 1);
    put_desc(&state, 1, 0x5000, 32, VRING_DESC_F_WRITE, 0);
    make_available(&state, 0, 0);
    device.common.status = 4;
    queue_notify(&device.common, 0);
    CHECK(server.requests == 1);
    CHECK(get_le32(state.ram + 0x5000) == 7);
    CHECK(state.ram[0x5004] == 101);
    CHECK(get_le16(state.ram + 0x5005) == 42);
}

int main(void)
{
    test_transport_and_queue();
    test_console();
    test_block_status();
    test_network();
    test_input();
    test_9p_protocol();
    puts("VirtIO C tests passed");
    return 0;
}
