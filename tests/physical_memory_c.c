#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#include "cutils.h"
#include "iomem.h"

#define CHECK(condition) do {                                                \
        if (!(condition)) {                                                  \
            fprintf(stderr, "%s:%d: check failed: %s\n",                    \
                    __FILE__, __LINE__, #condition);                         \
            exit(1);                                                         \
        }                                                                    \
    } while (0)

typedef struct {
    uint8_t *address[8];
    size_t length[8];
    int count;
} InvalidationLog;

static void record_invalidation(void *opaque, uint8_t *address, size_t length)
{
    InvalidationLog *log = opaque;

    CHECK(log->count < (int)(sizeof(log->address) / sizeof(log->address[0])));
    log->address[log->count] = address;
    log->length[log->count] = length;
    log->count++;
}

static uint32_t device_read(void *opaque, uint32_t offset, int size_log2)
{
    (void)opaque;
    (void)offset;
    (void)size_log2;
    return 0;
}

static void device_write(void *opaque, uint32_t offset, uint32_t value,
                         int size_log2)
{
    (void)opaque;
    (void)offset;
    (void)value;
    (void)size_log2;
}

static void test_maps_ram_and_devices(void)
{
    PhysMemoryMap *map = phys_mem_map_init();
    PhysMemoryRange *ram;
    PhysMemoryRange *device;
    uint8_t *ptr;
    size_t i;

    ram = cpu_register_ram(map, UINT64_C(0x80000000), 0x2000, 0);
    device = cpu_register_device(map, UINT64_C(0x10000000), 0x100, NULL,
                                 device_read, device_write,
                                 DEVIO_SIZE8 | DEVIO_SIZE32);
    CHECK(get_phys_mem_range(map, UINT64_C(0x80000000)) == ram);
    CHECK(get_phys_mem_range(map, UINT64_C(0x80001fff)) == ram);
    CHECK(get_phys_mem_range(map, UINT64_C(0x80002000)) == NULL);
    CHECK(get_phys_mem_range(map, UINT64_C(0x10000000)) == device);
    CHECK(phys_mem_get_ram_ptr(map, UINT64_C(0x10000000), FALSE) == NULL);
    ptr = phys_mem_get_ram_ptr(map, UINT64_C(0x80000000), FALSE);
    CHECK(ptr == ram->phys_mem);
    for (i = 0; i < 0x2000; i++)
        CHECK(ptr[i] == 0);
    phys_mem_map_end(map);
}

static void test_disabled_and_moved_mappings(void)
{
    InvalidationLog log = { 0 };
    PhysMemoryMap *map = phys_mem_map_init();
    PhysMemoryRange *ram;

    map->opaque = &log;
    map->flush_tlb_write_range = record_invalidation;
    ram = cpu_register_ram(map, UINT64_C(0x80000000), 0x1000,
                           DEVRAM_FLAG_DISABLED);
    CHECK(get_phys_mem_range(map, UINT64_C(0x80000000)) == NULL);
    phys_mem_set_addr(ram, UINT64_C(0x81000000), TRUE);
    CHECK(log.count == 1);
    CHECK(log.address[0] == ram->phys_mem);
    CHECK(log.length[0] == 0x1000);
    CHECK(phys_mem_get_ram_ptr(map, UINT64_C(0x81000000), FALSE) ==
          ram->phys_mem);
    phys_mem_set_addr(ram, UINT64_C(0x81000000), TRUE);
    CHECK(log.count == 1);
    phys_mem_set_addr(ram, 0, FALSE);
    CHECK(log.count == 2);
    CHECK(get_phys_mem_range(map, UINT64_C(0x81000000)) == NULL);
    phys_mem_map_end(map);
}

static void test_dirty_pages(void)
{
    InvalidationLog log = { 0 };
    PhysMemoryMap *map = phys_mem_map_init();
    PhysMemoryRange *plain;
    PhysMemoryRange *tracked;
    const uint32_t *snapshot;

    map->opaque = &log;
    map->flush_tlb_write_range = record_invalidation;
    plain = cpu_register_ram(map, UINT64_C(0x80000000), 0x1000, 0);
    tracked = cpu_register_ram(map, UINT64_C(0x90000000), 33 * 0x1000,
                               DEVRAM_FLAG_DIRTY_BITS);
    CHECK(phys_mem_get_ram_ptr(map, UINT64_C(0x90000000), TRUE) ==
          tracked->phys_mem);
    CHECK(phys_mem_get_ram_ptr(map, UINT64_C(0x90020000), TRUE) ==
          tracked->phys_mem + 0x20000);
    snapshot = phys_mem_get_dirty_bits(tracked);
    CHECK(snapshot[0] == 1);
    CHECK(snapshot[1] == 1);
    CHECK(log.count == 1);
    CHECK(log.address[0] == tracked->phys_mem);
    CHECK(log.length[0] == 33 * 0x1000);
    snapshot = phys_mem_get_dirty_bits(tracked);
    CHECK(snapshot[0] == 0);
    CHECK(snapshot[1] == 0);
    CHECK(phys_mem_get_dirty_bits(plain) == NULL);

    phys_mem_get_ram_ptr(map, UINT64_C(0x90001000), TRUE);
    phys_mem_reset_dirty_bit(tracked, 0x1001);
    CHECK(log.count == 2);
    CHECK(log.address[1] == tracked->phys_mem + 0x1000);
    CHECK(log.length[1] == 0x1000);
    phys_mem_reset_dirty_bit(tracked, 0x1001);
    CHECK(log.count == 2);
    snapshot = phys_mem_get_dirty_bits(tracked);
    CHECK(snapshot[0] == 0);
    CHECK(snapshot[1] == 0);
    phys_mem_map_end(map);
}

int main(void)
{
    test_maps_ram_and_devices();
    test_disabled_and_moved_mappings();
    test_dirty_pages();
    puts("physical memory C tests passed");
    return 0;
}
