#ifndef TINYEMU_BRIDGE_H
#define TINYEMU_BRIDGE_H

#include <stddef.h>
#include <stdint.h>

typedef struct TinyemuCore TinyemuCore;

enum {
    TINYEMU_CPU_LIMIT_REACHED = 0,
    TINYEMU_CPU_WFI_SLEEP = 1,
    TINYEMU_CPU_HOST_SERVICE_REQUESTED = 2,
    TINYEMU_CPU_TIMER_REPROGRAMMED = 3,
};

typedef struct {
    uint32_t consumed_cycles;
    uint32_t reason;
} TinyemuCpuRunResult;

TinyemuCore *tinyemu_core_create(void);
void tinyemu_core_destroy(TinyemuCore *core);
int tinyemu_core_register_ram(TinyemuCore *core, uint64_t base, uint64_t len,
                              int flags);
int tinyemu_core_register_device(TinyemuCore *core, uint64_t base, uint64_t len,
                                 int widths);
uint8_t *tinyemu_core_ram_range(TinyemuCore *core, uint64_t address,
                                size_t len, int write);
int tinyemu_core_read(TinyemuCore *core, uint64_t address, unsigned width,
                      uint64_t *value);
int tinyemu_core_write(TinyemuCore *core, uint64_t address, unsigned width,
                       uint64_t value);
int tinyemu_core_take_dirty(TinyemuCore *core, int region,
                            uint32_t *words, size_t count);
int tinyemu_core_clear_dirty(TinyemuCore *core, int region, uint64_t offset);
void tinyemu_core_set_guest_timer_ticks(TinyemuCore *core, uint64_t ticks);
uint64_t tinyemu_core_stimecmp(const TinyemuCore *core);
uint32_t tinyemu_core_is_wfi_sleeping(const TinyemuCore *core);
void tinyemu_core_set_interrupts(TinyemuCore *core, uint32_t mask);
TinyemuCpuRunResult tinyemu_core_run_cpu(TinyemuCore *core, uint32_t cycle_limit,
                                   void *platform);
uint64_t tinyemu_core_pc(const TinyemuCore *core);
uint64_t tinyemu_core_register(const TinyemuCore *core, unsigned index);
uint64_t tinyemu_core_mcause(const TinyemuCore *core);
uint64_t tinyemu_core_mtval(const TinyemuCore *core);

#endif
