#include "bridge.h"
#include "riscv_cpu_priv.h"

#include <string.h>

typedef struct {
    struct TinyemuCore *core;
    uint64_t base;
} BridgeDevice;

struct TinyemuCore {
    PhysMemoryMap *map;
    RISCVCPUState *cpu;
    uint64_t time_ticks;
    void *host;
    BridgeDevice devices[PHYS_MEM_RANGE_MAX];
    unsigned device_count;
};

extern int tinyemu_host_read(void *host, uint64_t address, unsigned width,
                             uint32_t *value);
extern int tinyemu_host_write(void *host, uint64_t address, unsigned width,
                              uint32_t value);
extern uint32_t tinyemu_host_interrupts(void *host);

static uint64_t get_time(void *opaque)
{
    return ((TinyemuCore *)opaque)->time_ticks;
}

static void flush_write_tlb(void *opaque, uint8_t *address, size_t len)
{
    TinyemuCore *core = opaque;
    riscv_cpu_flush_tlb_write_range_ram(core->cpu, address, len);
}

static int read_device(void *opaque, uint32_t offset, int size_log2,
                       uint32_t *value)
{
    BridgeDevice *device = opaque;
    int status = tinyemu_host_read(device->core->host, device->base + offset,
                                   1u << size_log2, value);
    tinyemu_core_set_interrupts(device->core,
                                tinyemu_host_interrupts(device->core->host));
    return status;
}

static int write_device(void *opaque, uint32_t offset, uint32_t value,
                        int size_log2)
{
    BridgeDevice *device = opaque;
    int status = tinyemu_host_write(device->core->host, device->base + offset,
                                    1u << size_log2, value);
    tinyemu_core_set_interrupts(device->core,
                                tinyemu_host_interrupts(device->core->host));
    return status;
}

TinyemuCore *tinyemu_core_create(void)
{
    TinyemuCore *core = tinyemu_alloc_zeroed(sizeof(*core));
    if (!core)
        return NULL;
    core->map = phys_mem_map_init();
    if (!core->map) {
        tinyemu_free(core);
        return NULL;
    }
    core->cpu = riscv_cpu_init(core->map);
    if (!core->cpu) {
        phys_mem_map_end(core->map);
        tinyemu_free(core);
        return NULL;
    }
    core->map->opaque = core;
    core->map->flush_tlb_write_range = flush_write_tlb;
    riscv_cpu_set_time_source(core->cpu, get_time, core);
    return core;
}

void tinyemu_core_destroy(TinyemuCore *core)
{
    if (!core)
        return;
    riscv_cpu_end(core->cpu);
    phys_mem_map_end(core->map);
    tinyemu_free(core);
}

int tinyemu_core_register_ram(TinyemuCore *core, uint64_t base, uint64_t len,
                              int flags)
{
    if (core->map->n_phys_mem_range == PHYS_MEM_RANGE_MAX ||
        len == 0 || (len & (DEVRAM_PAGE_SIZE - 1)) != 0 ||
        len > SIZE_MAX || base > UINT64_MAX - len)
        return -1;
    int region = core->map->n_phys_mem_range;
    return cpu_register_ram(core->map, base, len, flags) ? region : -1;
}

int tinyemu_core_register_device(TinyemuCore *core, uint64_t base, uint64_t len,
                                 int widths)
{
    if (core->map->n_phys_mem_range == PHYS_MEM_RANGE_MAX ||
        core->device_count == PHYS_MEM_RANGE_MAX || len == 0 ||
        len > UINT32_MAX || base > UINT64_MAX - len)
        return -1;
    int region = core->map->n_phys_mem_range;
    BridgeDevice *device = &core->devices[core->device_count++];
    device->core = core;
    device->base = base;
    if (!cpu_register_device(core->map, base, len, device,
                             read_device, write_device, widths))
        return -1;
    return region;
}

uint8_t *tinyemu_core_ram_range(TinyemuCore *core, uint64_t address,
                                size_t len, int write)
{
    PhysMemoryRange *region = get_phys_mem_range(core->map, address);
    if (!region || !region->is_ram ||
        (write && (region->devram_flags & DEVRAM_FLAG_ROM)) ||
        len > region->size - (address - region->addr))
        return NULL;
    size_t first = (size_t)(address - region->addr);
    if (write && len != 0) {
        for (size_t page = first >> DEVRAM_PAGE_SIZE_LOG2;
             page <= (first + len - 1) >> DEVRAM_PAGE_SIZE_LOG2; page++)
            phys_mem_set_dirty_bit(region, page << DEVRAM_PAGE_SIZE_LOG2);
    }
    return region->phys_mem + first;
}

int tinyemu_core_read(TinyemuCore *core, uint64_t address, unsigned width,
                      uint64_t *value)
{
    if ((width != 1 && width != 2 && width != 4 && width != 8) || !value)
        return -1;
    uint8_t *ptr = tinyemu_core_ram_range(core, address, width, 0);
    if (!ptr)
        return -1;
    uint64_t result = 0;
    for (unsigned i = 0; i < width; i++)
        result |= (uint64_t)ptr[i] << (8 * i);
    *value = result;
    return 0;
}

int tinyemu_core_write(TinyemuCore *core, uint64_t address, unsigned width,
                       uint64_t value)
{
    if (width != 1 && width != 2 && width != 4 && width != 8)
        return -1;
    uint8_t *ptr = tinyemu_core_ram_range(core, address, width, 1);
    if (!ptr)
        return -1;
    for (unsigned i = 0; i < width; i++)
        ptr[i] = (uint8_t)(value >> (8 * i));
    return 0;
}

int tinyemu_core_take_dirty(TinyemuCore *core, int region,
                            uint32_t *words, size_t count)
{
    if (region < 0 || region >= core->map->n_phys_mem_range)
        return -1;
    PhysMemoryRange *range = &core->map->phys_mem_range[region];
    if (!range->is_ram || !range->dirty_bits ||
        count != (size_t)range->dirty_bits_size / sizeof(uint32_t))
        return -1;
    const uint32_t *snapshot = phys_mem_get_dirty_bits(range);
    for (size_t i = 0; i < count; i++)
        words[i] = snapshot[i];
    return 0;
}

int tinyemu_core_clear_dirty(TinyemuCore *core, int region, uint64_t offset)
{
    if (region < 0 || region >= core->map->n_phys_mem_range)
        return -1;
    PhysMemoryRange *range = &core->map->phys_mem_range[region];
    if (!range->is_ram || offset >= range->org_size)
        return -1;
    phys_mem_reset_dirty_bit(range, (size_t)offset);
    return 0;
}

void tinyemu_core_set_time(TinyemuCore *core, uint64_t ticks)
{
    core->time_ticks = ticks;
    riscv_cpu_update_time(core->cpu, ticks);
}

uint64_t tinyemu_core_stimecmp(const TinyemuCore *core)
{
    if (riscv_cpu_get_mip(core->cpu) & MIP_STIP)
        return UINT64_MAX;
    return riscv_cpu_get_stimecmp(core->cpu);
}

void tinyemu_core_set_interrupts(TinyemuCore *core, uint32_t mask)
{
    const uint32_t external = MIP_MSIP | MIP_MTIP | MIP_MEIP | MIP_SEIP;
    riscv_cpu_reset_mip(core->cpu, external);
    riscv_cpu_set_mip(core->cpu, mask & external);
}

TinyemuRunResult tinyemu_core_run(TinyemuCore *core, uint32_t budget,
                                   void *host)
{
    core->host = host;
    uint64_t before = riscv_cpu_get_cycles(core->cpu);
    riscv_cpu_interp(core->cpu, (int)budget);
    core->host = NULL;
    TinyemuRunResult result = {
        (uint32_t)(riscv_cpu_get_cycles(core->cpu) - before),
        (uint32_t)riscv_cpu_get_power_down(core->cpu)
    };
    return result;
}

uint64_t tinyemu_core_pc(const TinyemuCore *core)
{
    return core->cpu->pc;
}

uint64_t tinyemu_core_register(const TinyemuCore *core, unsigned index)
{
    return index < 32 ? core->cpu->reg[index] : 0;
}

uint64_t tinyemu_core_mcause(const TinyemuCore *core)
{
    return core->cpu->mcause;
}

uint64_t tinyemu_core_mtval(const TinyemuCore *core)
{
    return core->cpu->mtval;
}
