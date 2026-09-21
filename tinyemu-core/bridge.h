#ifndef TINYEMU_BRIDGE_H
#define TINYEMU_BRIDGE_H

#include <stddef.h>
#include <stdint.h>

typedef struct TinyemuCore TinyemuCore;

typedef struct {
    uint32_t cycles;
    uint32_t waiting;
} TinyemuRunResult;

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
void tinyemu_core_set_time(TinyemuCore *core, uint64_t ticks);
void tinyemu_core_set_interrupts(TinyemuCore *core, uint32_t mask);
TinyemuRunResult tinyemu_core_run(TinyemuCore *core, uint32_t budget,
                                   void *host);
uint64_t tinyemu_core_pc(const TinyemuCore *core);
uint64_t tinyemu_core_register(const TinyemuCore *core, unsigned index);
uint64_t tinyemu_core_mcause(const TinyemuCore *core);
uint64_t tinyemu_core_mtval(const TinyemuCore *core);

#endif
