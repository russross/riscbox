#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#include "cutils.h"
#include "goldfish_rtc.h"
#include "iomem.h"
#include "virtio.h"
#include "machine.h"
#include "riscv_cpu.h"
#include "riscv_machine_test.h"
#include "uart16550.h"

#define CHECK(condition) do {                                                \
        if (!(condition)) {                                                  \
            fprintf(stderr, "%s:%d: check failed: %s\n",                    \
                    __FILE__, __LINE__, #condition);                         \
            exit(1);                                                         \
        }                                                                    \
    } while (0)

typedef struct {
    int level;
    int changes;
} IRQLog;

typedef struct {
    uint8_t data[16];
    int length;
} TxLog;

typedef struct {
    int count;
    int y;
    int height;
} DrawLog;

static void record_irq(void *opaque, int irq_num, int level)
{
    IRQLog *log = opaque;

    CHECK(irq_num == 7);
    log->level = level;
    log->changes++;
}

static void record_tx(void *opaque, const uint8_t *buf, int len)
{
    TxLog *log = opaque;

    CHECK(log->length + len <= (int)sizeof(log->data));
    memcpy(log->data + log->length, buf, len);
    log->length += len;
}

static uint32_t mmio_read(PhysMemoryMap *map, uint64_t address, int size_log2)
{
    PhysMemoryRange *range = get_phys_mem_range(map, address);

    CHECK(range != NULL);
    CHECK(!range->is_ram);
    return range->read_func(range->opaque, address - range->addr, size_log2);
}

static void mmio_write(PhysMemoryMap *map, uint64_t address, uint32_t value,
                       int size_log2)
{
    PhysMemoryRange *range = get_phys_mem_range(map, address);

    CHECK(range != NULL);
    CHECK(!range->is_ram);
    range->write_func(range->opaque, address - range->addr, value, size_log2);
}

static void test_uart(void)
{
    const uint64_t base = UINT64_C(0x10000000);
    PhysMemoryMap *map = phys_mem_map_init();
    IRQLog irq_log = { 0 };
    TxLog tx_log = { { 0 }, 0 };
    IRQSignal irq;
    UART16550State *uart;
    uint8_t input[] = { 'a', 'b', 'c', 'd' };

    irq_init(&irq, record_irq, &irq_log, 7);
    uart = uart16550_init(map, base, 0x100, &irq, record_tx, &tx_log);
    CHECK(mmio_read(map, base + 5, 0) == 0x60);
    CHECK(mmio_read(map, base + 6, 0) == 0xb0);
    mmio_write(map, base + 7, 0x5a, 0);
    CHECK(mmio_read(map, base + 7, 0) == 0x5a);

    mmio_write(map, base + 3, 0x80, 0);
    mmio_write(map, base, 0x34, 0);
    mmio_write(map, base + 1, 0x12, 0);
    CHECK(mmio_read(map, base, 0) == 0x34);
    CHECK(mmio_read(map, base + 1, 0) == 0x12);
    mmio_write(map, base + 3, 0x03, 0);

    CHECK(uart16550_receive(uart, input, 4) == 1);
    CHECK(mmio_read(map, base + 5, 0) == 0x61);
    mmio_write(map, base + 1, 1, 0);
    CHECK(irq_log.level == 1);
    CHECK(mmio_read(map, base + 2, 0) == 0x04);
    CHECK(mmio_read(map, base, 0) == 'a');
    CHECK(irq_log.level == 0);

    mmio_write(map, base + 2, 0x41, 0);
    CHECK(uart16550_receive(uart, input, 4) == 4);
    CHECK(mmio_read(map, base + 2, 0) == 0xc4);
    CHECK(mmio_read(map, base, 0) == 'a');
    CHECK(mmio_read(map, base + 2, 0) == 0xcc);
    mmio_write(map, base + 2, 0x03, 0);
    CHECK(mmio_read(map, base + 5, 0) == 0x60);

    mmio_write(map, base, 'x', 0);
    CHECK(tx_log.length == 1 && tx_log.data[0] == 'x');
    mmio_write(map, base + 1, 2, 0);
    CHECK(mmio_read(map, base + 2, 0) == 0xc2);
    CHECK(irq_log.level == 0);

    mmio_write(map, base + 4, 0x13, 0);
    CHECK(mmio_read(map, base + 6, 0) == 0x30);
    mmio_write(map, base, 'z', 0);
    CHECK(tx_log.length == 1);
    CHECK(mmio_read(map, base, 0) == 'z');

    uart16550_end(uart);
    phys_mem_map_end(map);
}

static void test_goldfish_rtc(void)
{
    const uint64_t base = UINT64_C(0x101000);
    PhysMemoryMap *map = phys_mem_map_init();
    IRQLog irq_log = { 0 };
    IRQSignal irq;
    GoldfishRTCState *rtc;
    uint64_t now;

    irq_init(&irq, record_irq, &irq_log, 7);
    rtc = goldfish_rtc_init(map, base, 0x1000, &irq);
    now = mmio_read(map, base, 2);
    now |= (uint64_t)mmio_read(map, base + 4, 2) << 32;
    CHECK(now != 0);
    mmio_write(map, base + 0x10, 1, 2);
    mmio_write(map, base + 0x0c, now >> 32, 2);
    mmio_write(map, base + 0x08, (uint32_t)now, 2);
    CHECK(irq_log.level == 1);
    CHECK(mmio_read(map, base + 0x18, 2) == 0);
    mmio_write(map, base + 0x1c, 0, 2);
    CHECK(irq_log.level == 0);
    CHECK(goldfish_rtc_get_sleep_duration(rtc, 25) == 25);
    goldfish_rtc_end(rtc);
    phys_mem_map_end(map);
}

static void test_clint_and_plic(void)
{
    RISCVMachineTest *test = riscv_machine_test_init();
    const uint32_t meip = UINT32_C(1) << 11;
    const uint32_t seip = UINT32_C(1) << 9;

    riscv_machine_test_clint_write(test, 0, 3);
    CHECK(riscv_machine_test_clint_read(test, 0) == 1);
    CHECK(riscv_machine_test_mip(test) & (UINT32_C(1) << 3));
    riscv_machine_test_clint_write(test, 0, 0);
    CHECK(!(riscv_machine_test_mip(test) & (UINT32_C(1) << 3)));
    riscv_machine_test_clint_write(test, 0x4000, 0x89abcdef);
    riscv_machine_test_clint_write(test, 0x4004, 0x01234567);
    CHECK(riscv_machine_test_clint_read(test, 0x4000) == 0x89abcdef);
    CHECK(riscv_machine_test_clint_read(test, 0x4004) == 0x01234567);
    CHECK(riscv_machine_test_clint_read(test, 0x1234) == 0);

    riscv_machine_test_plic_write(test, 4 * 5, 3);
    riscv_machine_test_plic_write(test, 4 * 7, 5);
    riscv_machine_test_plic_write(test, 0x2000, (1U << 5) | (1U << 7));
    riscv_machine_test_plic_set_irq(test, 5, 1);
    riscv_machine_test_plic_set_irq(test, 7, 1);
    CHECK(riscv_machine_test_mip(test) & meip);
    CHECK(riscv_machine_test_plic_read(test, 0x200004) == 7);
    CHECK(riscv_machine_test_plic_read(test, 0x200004) == 5);
    CHECK(!(riscv_machine_test_mip(test) & meip));
    riscv_machine_test_plic_write(test, 0x200004, 7);
    CHECK(riscv_machine_test_mip(test) & meip);
    riscv_machine_test_plic_set_irq(test, 7, 0);
    CHECK(riscv_machine_test_plic_read(test, 0x200004) == 7);
    riscv_machine_test_plic_write(test, 0x200004, 7);
    riscv_machine_test_plic_set_irq(test, 5, 0);
    riscv_machine_test_plic_write(test, 0x200004, 5);
    CHECK(!(riscv_machine_test_mip(test) & meip));

    riscv_machine_test_plic_write(test, 4 * 9, 4);
    riscv_machine_test_plic_write(test, 0x2080, 1U << 9);
    riscv_machine_test_plic_write(test, 0x201000, 4);
    riscv_machine_test_plic_set_irq(test, 9, 1);
    CHECK(!(riscv_machine_test_mip(test) & seip));
    CHECK(riscv_machine_test_plic_read(test, 0x201004) == 0);
    riscv_machine_test_plic_write(test, 0x201000, 3);
    CHECK(riscv_machine_test_mip(test) & seip);
    CHECK(riscv_machine_test_plic_read(test, 0x201004) == 9);

    riscv_machine_test_end(test);
}

static int contains_bytes(const uint8_t *data, size_t size, const char *text)
{
    size_t length = strlen(text);
    size_t i;

    for (i = 0; i + length <= size; i++) {
        if (memcmp(data + i, text, length) == 0)
            return 1;
    }
    return 0;
}

static void test_fdt_and_finisher(void)
{
    RISCVMachineTest *test = riscv_machine_test_init();
    uint8_t *fdt;
    size_t size;
    pid_t child;
    int status;

    fdt = riscv_machine_test_fdt(test, &size);
    CHECK(size > 40);
    CHECK(fdt[0] == 0xd0 && fdt[1] == 0x0d && fdt[2] == 0xfe &&
          fdt[3] == 0xed);
    CHECK(contains_bytes(fdt, size, "sifive,clint0"));
    CHECK(contains_bytes(fdt, size, "sifive,plic-1.0.0"));
    CHECK(contains_bytes(fdt, size, "google,goldfish-rtc"));
    CHECK(contains_bytes(fdt, size, "ns16550a"));
    CHECK(contains_bytes(fdt, size, "simple-framebuffer"));
    CHECK(contains_bytes(fdt, size, "syscon-poweroff"));
    CHECK(contains_bytes(fdt, size, "console=ttyS0"));
    free(fdt);
    riscv_machine_test_end(test);

    child = fork();
    CHECK(child >= 0);
    if (child == 0) {
        freopen("/dev/null", "w", stdout);
        riscv_machine_test_finisher_write(0, 0x5555);
        _exit(2);
    }
    CHECK(waitpid(child, &status, 0) == child);
    CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0);
    child = fork();
    CHECK(child >= 0);
    if (child == 0) {
        freopen("/dev/null", "w", stdout);
        riscv_machine_test_finisher_write(0, 0x00023333);
        _exit(2);
    }
    CHECK(waitpid(child, &status, 0) == child);
    CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 1);
}

static void record_draw(FBDevice *fb, void *opaque, int x, int y,
                        int width, int height)
{
    DrawLog *log = opaque;

    CHECK(x == 0 && width == fb->width);
    log->count++;
    log->y = y;
    log->height = height;
}

static void ignore_invalidation(void *opaque, uint8_t *address, size_t length)
{
    (void)opaque;
    (void)address;
    (void)length;
}

static void test_framebuffer(void)
{
    PhysMemoryMap *map = phys_mem_map_init();
    FBDevice fb = { 0 };
    DrawLog log = { 0 };

    map->flush_tlb_write_range = ignore_invalidation;
    simplefb_init(map, UINT64_C(0x4100000), &fb, 640, 480);
    CHECK(fb.width == 640 && fb.height == 480 && fb.stride == 2560);
    CHECK(fb.fb_size == 0x130000);
    CHECK(fb.fb_data != NULL);
    fb.refresh(&fb, record_draw, &log);
    CHECK(log.count == 0);
    phys_mem_get_ram_ptr(map, UINT64_C(0x4100000) + 10 * 4096, TRUE);
    fb.refresh(&fb, record_draw, &log);
    CHECK(log.count == 1 && log.y == 16 && log.height == 2);
    fb.refresh(&fb, record_draw, &log);
    CHECK(log.count == 1);
    free(fb.device_opaque);
    phys_mem_map_end(map);
}

int main(void)
{
    test_uart();
    test_goldfish_rtc();
    test_clint_and_plic();
    test_fdt_and_finisher();
    test_framebuffer();
    puts("platform foundation C tests passed");
    return 0;
}
