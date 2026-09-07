/*
 * RISCV machine
 * 
 * Copyright (c) 2016-2017 Fabrice Bellard
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
 * THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 */
#include <stdlib.h>
#include <stdio.h>
#include <stdarg.h>
#include <string.h>
#include <inttypes.h>
#include <assert.h>
#include <fcntl.h>
#include <errno.h>
#include <unistd.h>
#include <time.h>

#include "cutils.h"
#include "iomem.h"
#include "riscv_cpu.h"
#include "uart16550.h"
#include "virtio.h"
#include "machine.h"

/* RISCV machine */

typedef struct RISCVMachine {
    VirtMachine common;
    PhysMemoryMap *mem_map;
    RISCVCPUState *cpu_state;
    uint64_t ram_size;
    /* RTC */
    BOOL rtc_real_time;
    uint64_t rtc_start_time;
    uint64_t timecmp;
    uint32_t msip;
    /* PLIC */
    uint32_t plic_level_irq;
    uint32_t plic_pending_irq;
    uint32_t plic_claimed_irq[2];
    /* contexts: 0 is M-mode, 1 is S-mode */
    uint32_t plic_priority[32];
    uint32_t plic_enable[2];
    uint32_t plic_threshold[2];
    IRQSignal plic_irq[32]; /* IRQ 0 is not used */

    VIRTIODevice *keyboard_dev;
    VIRTIODevice *mouse_dev;

    UART16550State *uart_dev;
    VIRTIODevice *virtio_console_dev;
    VMConsoleType console_type;
    BOOL uart_output;

    int virtio_count;
} RISCVMachine;

/* Memory map following the QEMU 'virt' platform (hw/riscv/virt.c).
   The framebuffer is not part of the virt spec; it lives in
   the VIRT_PLATFORM_BUS expansion window (0x4000000-0x5ffffff). */
#define LOW_RAM_SIZE   0x00010000 /* 64KB */
#define RAM_BASE_ADDR  0x80000000 /* VIRT_DRAM */
#define KERNEL_LOAD_OFFSET 0x00200000
#define FDT_ALIGN 0x00200000
#define FDT_MAX_OFFSET 0x40000000
#define TEST_BASE_ADDR 0x00100000 /* VIRT_TEST */
#define TEST_SIZE      0x00001000
#define CLINT_BASE_ADDR 0x02000000 /* VIRT_CLINT */
#define CLINT_SIZE      0x00010000
#define VIRTIO_BASE_ADDR 0x10001000 /* VIRT_VIRTIO */
#define VIRTIO_SIZE      0x1000
#define VIRTIO_IRQ       1
#define PLIC_BASE_ADDR 0x0c000000 /* VIRT_PLIC */
#define PLIC_SIZE      0x04000000
#define UART_BASE_ADDR 0x10000000 /* VIRT_UART0 */
#define UART_SIZE      0x100
#define UART_IRQ       10
#define UART_CLOCK     3686400
#define FRAMEBUFFER_BASE_ADDR 0x04100000

/* UART_IRQ is reserved: map the n-th virtio device to its PLIC IRQ. */
static int virtio_irq_num(int index)
{
    int irq_num = VIRTIO_IRQ + index;
    if (irq_num >= UART_IRQ)
        irq_num++;
    return irq_num;
}

#define RTC_FREQ 10000000
#define RTC_FREQ_DIV 16 /* arbitrary, relative to CPU freq to have a
                           10 MHz frequency */

static uint64_t rtc_get_real_time(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * RTC_FREQ +
        (ts.tv_nsec / (1000000000 / RTC_FREQ));
}

static uint64_t rtc_get_time(RISCVMachine *m)
{
    uint64_t val;
    if (m->rtc_real_time) {
        val = rtc_get_real_time() - m->rtc_start_time;
    } else {
        val = riscv_cpu_get_cycles(m->cpu_state) / RTC_FREQ_DIV;
    }
    //    printf("rtc_time=%" PRId64 "\n", val);
    return val;
}

static uint64_t rtc_get_time_for_cpu(void *opaque)
{
    RISCVMachine *m = opaque;

    return rtc_get_time(m);
}

/* SiFive test finisher, as on QEMU virt: a 32-bit register at offset
   0 whose low half selects pass or fail. */
#define TEST_FINISHER_FAIL 0x3333
#define TEST_FINISHER_PASS 0x5555

static uint32_t test_read(void *opaque, uint32_t offset,
                          int size_log2)
{
    (void)opaque;
    (void)offset;
    assert(size_log2 == 2);
    return 0;
}

static void test_write(void *opaque, uint32_t offset, uint32_t val,
                       int size_log2)
{
    (void)opaque;
    assert(size_log2 == 2);
    if (offset != 0)
        return;
    switch (val & 0xffff) {
    case TEST_FINISHER_FAIL:
        printf("\nTest failed with code %d.\n", (val >> 16) & 0xffff);
        exit(1);
    case TEST_FINISHER_PASS:
        printf("\nPower off.\n");
        exit(0);
    default:
        break;
    }
}

static uint32_t clint_read(void *opaque, uint32_t offset, int size_log2)
{
    RISCVMachine *m = opaque;
    uint32_t val;

    assert(size_log2 == 2);
    switch(offset) {
    case 0x0:
        val = m->msip;
        break;
    case 0xbff8:
        val = rtc_get_time(m);
        break;
    case 0xbffc:
        val = rtc_get_time(m) >> 32;
        break;
    case 0x4000:
        val = m->timecmp;
        break;
    case 0x4004:
        val = m->timecmp >> 32;
        break;
    default:
        val = 0;
        break;
    }
    return val;
}
 
static void clint_write(void *opaque, uint32_t offset, uint32_t val,
                      int size_log2)
{
    RISCVMachine *m = opaque;

    assert(size_log2 == 2);
    switch(offset) {
    case 0x0:
        m->msip = val & 1;
        if (m->msip)
            riscv_cpu_set_mip(m->cpu_state, MIP_MSIP);
        else
            riscv_cpu_reset_mip(m->cpu_state, MIP_MSIP);
        break;
    case 0x4000:
        m->timecmp = (m->timecmp & ~0xffffffff) | val;
        riscv_cpu_reset_mip(m->cpu_state, MIP_MTIP);
        break;
    case 0x4004:
        m->timecmp = (m->timecmp & 0xffffffff) | ((uint64_t)val << 32);
        riscv_cpu_reset_mip(m->cpu_state, MIP_MTIP);
        break;
    default:
        break;
    }
}

/* Standard SiFive PLIC layout, two hart-0 contexts: 0 is M-mode,
   1 is S-mode. */
#define PLIC_PRIORITY_BASE 0x0
#define PLIC_PENDING_BASE 0x1000
#define PLIC_ENABLE_BASE 0x2000
#define PLIC_ENABLE_SIZE 0x80
#define PLIC_CONTEXT_BASE 0x200000
#define PLIC_CONTEXT_SIZE 0x1000
#define PLIC_CONTEXT_THRESHOLD 0x0
#define PLIC_CONTEXT_CLAIM 0x4

static int plic_find_irq(RISCVMachine *s, int context, BOOL check_threshold)
{
    uint32_t mask;
    uint32_t best_priority;
    int best_irq, i;

    mask = s->plic_pending_irq & s->plic_enable[context];
    best_irq = 0;
    best_priority = 0;
    for(i = 1; i < 32; i++) {
        uint32_t priority;

        if (!(mask & (UINT32_C(1) << i)))
            continue;
        priority = s->plic_priority[i];
        if (check_threshold && priority <= s->plic_threshold[context])
            continue;
        if (priority > best_priority) {
            best_irq = i;
            best_priority = priority;
        }
    }
    return best_irq;
}

static void plic_update_mip(RISCVMachine *s)
{
    RISCVCPUState *cpu = s->cpu_state;
    if (plic_find_irq(s, 0, TRUE))
        riscv_cpu_set_mip(cpu, MIP_MEIP);
    else
        riscv_cpu_reset_mip(cpu, MIP_MEIP);
    if (plic_find_irq(s, 1, TRUE))
        riscv_cpu_set_mip(cpu, MIP_SEIP);
    else
        riscv_cpu_reset_mip(cpu, MIP_SEIP);
}

static uint32_t plic_claim(RISCVMachine *s, int context)
{
    uint32_t mask;
    int irq;

    irq = plic_find_irq(s, context, FALSE);
    if (irq != 0) {
        mask = UINT32_C(1) << irq;
        s->plic_pending_irq &= ~mask;
        s->plic_claimed_irq[context] |= mask;
        plic_update_mip(s);
    }
    return irq;
}

static void plic_complete(RISCVMachine *s, int context, uint32_t irq)
{
    uint32_t mask;

    if (irq < 1 || irq >= 32)
        return;
    mask = UINT32_C(1) << irq;
    if (!(s->plic_claimed_irq[context] & mask))
        return;
    s->plic_claimed_irq[context] &= ~mask;
    if (s->plic_level_irq & mask)
        s->plic_pending_irq |= mask;
    plic_update_mip(s);
}

static uint32_t plic_read(void *opaque, uint32_t offset, int size_log2)
{
    RISCVMachine *s = opaque;
    int context;

    assert(size_log2 == 2);
    if (offset < PLIC_PRIORITY_BASE + 4 * 32 &&
        (offset - PLIC_PRIORITY_BASE) % 4 == 0)
        return s->plic_priority[(offset - PLIC_PRIORITY_BASE) / 4];
    if (offset == PLIC_PENDING_BASE)
        return s->plic_pending_irq;
    if (offset == PLIC_ENABLE_BASE)
        return s->plic_enable[0];
    if (offset == PLIC_ENABLE_BASE + PLIC_ENABLE_SIZE)
        return s->plic_enable[1];
    if (offset >= PLIC_CONTEXT_BASE &&
        offset < PLIC_CONTEXT_BASE + 2 * PLIC_CONTEXT_SIZE) {
        context = (offset - PLIC_CONTEXT_BASE) / PLIC_CONTEXT_SIZE;
        offset = (offset - PLIC_CONTEXT_BASE) % PLIC_CONTEXT_SIZE;
        if (offset == PLIC_CONTEXT_THRESHOLD)
            return s->plic_threshold[context];
        if (offset == PLIC_CONTEXT_CLAIM)
            return plic_claim(s, context);
    }
    return 0;
}

static void plic_write(void *opaque, uint32_t offset, uint32_t val,
                       int size_log2)
{
    RISCVMachine *s = opaque;
    int context;

    assert(size_log2 == 2);
    if (offset >= PLIC_PRIORITY_BASE + 4 &&
        offset < PLIC_PRIORITY_BASE + 4 * 32 &&
        (offset - PLIC_PRIORITY_BASE) % 4 == 0) {
        s->plic_priority[(offset - PLIC_PRIORITY_BASE) / 4] = val & 7;
        plic_update_mip(s);
        return;
    }
    if (offset == PLIC_ENABLE_BASE) {
        s->plic_enable[0] = val & ~UINT32_C(1);
        plic_update_mip(s);
        return;
    }
    if (offset == PLIC_ENABLE_BASE + PLIC_ENABLE_SIZE) {
        s->plic_enable[1] = val & ~UINT32_C(1);
        plic_update_mip(s);
        return;
    }
    if (offset >= PLIC_CONTEXT_BASE &&
        offset < PLIC_CONTEXT_BASE + 2 * PLIC_CONTEXT_SIZE) {
        context = (offset - PLIC_CONTEXT_BASE) / PLIC_CONTEXT_SIZE;
        offset = (offset - PLIC_CONTEXT_BASE) % PLIC_CONTEXT_SIZE;
        if (offset == PLIC_CONTEXT_THRESHOLD) {
            s->plic_threshold[context] = val & 7;
            plic_update_mip(s);
        } else if (offset == PLIC_CONTEXT_CLAIM) {
            plic_complete(s, context, val);
        }
    }
}

static void plic_set_irq(void *opaque, int irq_num, int state)
{
    RISCVMachine *s = opaque;
    uint32_t mask;

    mask = UINT32_C(1) << irq_num;
    if (state) {
        s->plic_level_irq |= mask;
        if (!(s->plic_claimed_irq[0] & mask) &&
            !(s->plic_claimed_irq[1] & mask))
            s->plic_pending_irq |= mask;
    } else {
        s->plic_level_irq &= ~mask;
    }
    plic_update_mip(s);
}

static void uart_tx_func(void *opaque, const uint8_t *buf, int len)
{
    RISCVMachine *s = opaque;

    if (!s->common.console)
        return;
    if (s->console_type != VM_CONSOLE_UART && !s->uart_output)
        return;
    s->common.console->write_data(s->common.console->opaque, buf, len);
}

static uint8_t *get_ram_ptr(RISCVMachine *s, uint64_t paddr, BOOL is_rw)
{
    return phys_mem_get_ram_ptr(s->mem_map, paddr, is_rw);
}

/* FDT machine description */

#define FDT_MAGIC	0xd00dfeed
#define FDT_VERSION	17

struct fdt_header {
    uint32_t magic;
    uint32_t totalsize;
    uint32_t off_dt_struct;
    uint32_t off_dt_strings;
    uint32_t off_mem_rsvmap;
    uint32_t version;
    uint32_t last_comp_version; /* <= 17 */
    uint32_t boot_cpuid_phys;
    uint32_t size_dt_strings;
    uint32_t size_dt_struct;
};

struct fdt_reserve_entry {
       uint64_t address;
       uint64_t size;
};

#define FDT_BEGIN_NODE	1
#define FDT_END_NODE	2
#define FDT_PROP	3
#define FDT_NOP		4
#define FDT_END		9

typedef struct {
    uint32_t *tab;
    int tab_len;
    int tab_size;
    int open_node_count;
    
    char *string_table;
    int string_table_len;
    int string_table_size;
} FDTState;

static FDTState *fdt_init(void)
{
    FDTState *s;
    s = mallocz(sizeof(*s));
    return s;
}

static void fdt_alloc_len(FDTState *s, int len)
{
    int new_size;
    if (unlikely(len > s->tab_size)) {
        new_size = max_int(len, s->tab_size * 3 / 2);
        s->tab = realloc(s->tab, new_size * sizeof(uint32_t));
        s->tab_size = new_size;
    }
}

static void fdt_put32(FDTState *s, int v)
{
    fdt_alloc_len(s, s->tab_len + 1);
    s->tab[s->tab_len++] = cpu_to_be32(v);
}

/* the data is zero padded */
static void fdt_put_data(FDTState *s, const uint8_t *data, int len)
{
    int len1;
    
    len1 = (len + 3) / 4;
    fdt_alloc_len(s, s->tab_len + len1);
    if (len != 0)
        memcpy(s->tab + s->tab_len, data, len);
    memset((uint8_t *)(s->tab + s->tab_len) + len, 0, -len & 3);
    s->tab_len += len1;
}

static void fdt_begin_node(FDTState *s, const char *name)
{
    fdt_put32(s, FDT_BEGIN_NODE);
    fdt_put_data(s, (uint8_t *)name, strlen(name) + 1);
    s->open_node_count++;
}

static void fdt_begin_node_num(FDTState *s, const char *name, uint64_t n)
{
    char buf[256];
    snprintf(buf, sizeof(buf), "%s@%" PRIx64, name, n);
    fdt_begin_node(s, buf);
}

static void fdt_end_node(FDTState *s)
{
    fdt_put32(s, FDT_END_NODE);
    s->open_node_count--;
}

static int fdt_get_string_offset(FDTState *s, const char *name)
{
    int pos, new_size, name_size, new_len;

    pos = 0;
    while (pos < s->string_table_len) {
        if (!strcmp(s->string_table + pos, name))
            return pos;
        pos += strlen(s->string_table + pos) + 1;
    }
    /* add a new string */
    name_size = strlen(name) + 1;
    new_len = s->string_table_len + name_size;
    if (new_len > s->string_table_size) {
        new_size = max_int(new_len, s->string_table_size * 3 / 2);
        s->string_table = realloc(s->string_table, new_size);
        s->string_table_size = new_size;
    }
    pos = s->string_table_len;
    memcpy(s->string_table + pos, name, name_size);
    s->string_table_len = new_len;
    return pos;
}

static void fdt_prop(FDTState *s, const char *prop_name,
                     const void *data, int data_len)
{
    fdt_put32(s, FDT_PROP);
    fdt_put32(s, data_len);
    fdt_put32(s, fdt_get_string_offset(s, prop_name));
    fdt_put_data(s, data, data_len);
}

static void fdt_prop_tab_u32(FDTState *s, const char *prop_name,
                             uint32_t *tab, int tab_len)
{
    int i;
    fdt_put32(s, FDT_PROP);
    fdt_put32(s, tab_len * sizeof(uint32_t));
    fdt_put32(s, fdt_get_string_offset(s, prop_name));
    for(i = 0; i < tab_len; i++)
        fdt_put32(s, tab[i]);
}

static void fdt_prop_u32(FDTState *s, const char *prop_name, uint32_t val)
{
    fdt_prop_tab_u32(s, prop_name, &val, 1);
}

static void fdt_prop_tab_u64(FDTState *s, const char *prop_name,
                             uint64_t v0)
{
    uint32_t tab[2];
    tab[0] = v0 >> 32;
    tab[1] = v0;
    fdt_prop_tab_u32(s, prop_name, tab, 2);
}

static void fdt_prop_tab_u64_2(FDTState *s, const char *prop_name,
                               uint64_t v0, uint64_t v1)
{
    uint32_t tab[4];
    tab[0] = v0 >> 32;
    tab[1] = v0;
    tab[2] = v1 >> 32;
    tab[3] = v1;
    fdt_prop_tab_u32(s, prop_name, tab, 4);
}

static void fdt_prop_str(FDTState *s, const char *prop_name,
                         const char *str)
{
    fdt_prop(s, prop_name, str, strlen(str) + 1);
}

/* NULL terminated string list */
static void fdt_prop_tab_str(FDTState *s, const char *prop_name,
                             ...)
{
    va_list ap;
    int size, str_size;
    char *ptr, *tab;

    va_start(ap, prop_name);
    size = 0;
    for(;;) {
        ptr = va_arg(ap, char *);
        if (!ptr)
            break;
        str_size = strlen(ptr) + 1;
        size += str_size;
    }
    va_end(ap);
    
    tab = malloc(size);
    va_start(ap, prop_name);
    size = 0;
    for(;;) {
        ptr = va_arg(ap, char *);
        if (!ptr)
            break;
        str_size = strlen(ptr) + 1;
        memcpy(tab + size, ptr, str_size);
        size += str_size;
    }
    va_end(ap);
    
    fdt_prop(s, prop_name, tab, size);
    free(tab);
}

static int fdt_output_size(FDTState *s)
{
    int pos;

    pos = sizeof(struct fdt_header) + sizeof(struct fdt_reserve_entry);
    pos += s->tab_len * sizeof(uint32_t) + s->string_table_len;
    return (pos + 7) & ~7;
}

/* write the FDT to 'dst'. return the FDT size in bytes */
static int fdt_output(FDTState *s, uint8_t *dst)
{
    struct fdt_header *h;
    struct fdt_reserve_entry *re;
    int dt_struct_size;
    int dt_strings_size;
    int pos;

    assert(s->open_node_count == 0);
    
    dt_struct_size = s->tab_len * sizeof(uint32_t);
    dt_strings_size = s->string_table_len;

    h = (struct fdt_header *)dst;
    h->magic = cpu_to_be32(FDT_MAGIC);
    h->version = cpu_to_be32(FDT_VERSION);
    h->last_comp_version = cpu_to_be32(16);
    h->boot_cpuid_phys = cpu_to_be32(0);
    h->size_dt_strings = cpu_to_be32(dt_strings_size);
    h->size_dt_struct = cpu_to_be32(dt_struct_size);

    pos = sizeof(struct fdt_header);

    h->off_mem_rsvmap = cpu_to_be32(pos);
    re = (struct fdt_reserve_entry *)(dst + pos);
    re->address = 0; /* no reserved entry */
    re->size = 0;
    pos += sizeof(struct fdt_reserve_entry);

    h->off_dt_struct = cpu_to_be32(pos);
    memcpy(dst + pos, s->tab, dt_struct_size);
    pos += dt_struct_size;

    h->off_dt_strings = cpu_to_be32(pos);
    memcpy(dst + pos, s->string_table, dt_strings_size);
    pos += dt_strings_size;

    /* align to 8, just in case */
    while ((pos & 7) != 0) {
        dst[pos++] = 0;
    }

    h->totalsize = cpu_to_be32(pos);
    return pos;
}

void fdt_end(FDTState *s)
{
    free(s->tab);
    free(s->string_table);
    free(s);
}

/* Canonical single-letter ISA order from the RISC-V DT bindings. */
static const char single_letter_order[] = "iemafdqcbkjpvh";

static uint8_t *riscv_build_fdt(RISCVMachine *m, int *pfdt_size,
                                uint64_t initrd_start, uint64_t initrd_size,
                                const char *cmd_line)
{
    FDTState *s;
    uint8_t *dst;
    int size, i, cur_phandle, intc_phandle, plic_phandle, cpu_phandle;
    int syscon_phandle;
    char isa_string[192], *q;
    uint32_t misa;
    uint32_t tab[4];
    FBDevice *fb_dev;
    
    s = fdt_init();

    cur_phandle = 1;
    
    fdt_begin_node(s, "");
    fdt_prop_u32(s, "#address-cells", 2);
    fdt_prop_u32(s, "#size-cells", 2);
    fdt_prop_str(s, "compatible", "riscv-virtio");
    fdt_prop_str(s, "model", "riscv-virtio,qemu");

    /* CPU list */
    fdt_begin_node(s, "cpus");
    fdt_prop_u32(s, "#address-cells", 1);
    fdt_prop_u32(s, "#size-cells", 0);
    fdt_prop_u32(s, "timebase-frequency", RTC_FREQ);

    /* cpu */
    fdt_begin_node_num(s, "cpu", 0);
    fdt_prop_str(s, "device_type", "cpu");
    fdt_prop_u32(s, "reg", 0);
    fdt_prop_str(s, "status", "okay");
    fdt_prop_str(s, "compatible", "riscv");

    misa = riscv_cpu_get_misa(m->cpu_state);
    strcpy(isa_string, "rv64");
    q = isa_string + 4;
    for(i = 0; single_letter_order[i] != '\0'; i++) {
        int bit = single_letter_order[i] - 'a';
        if (bit == 'S' - 'A' || bit == 'U' - 'A')
            continue; /* privilege modes are not ISA extensions */
        if (misa & (1 << bit))
            *q++ = single_letter_order[i];
    }
    strcpy(q, "_zicbom_zicbop_zicboz_ziccamoa_ziccif_zicclsm_ziccrse_zicntr"
              "_zicond_zicsr_zifencei_zihintntl_zihintpause_zihpm_zimop"
              "_za64rs_zawrs"
              "_zcb_zcmop_zba_zbb_zbs_sstc_svadu");
    fdt_prop_str(s, "riscv,isa", isa_string);
    fdt_prop_str(s, "riscv,isa-base", "rv64i");

    /* Modern kernels enumerate extensions from riscv,isa-extensions. */
    {
        static const char ext_letters[] = "imafdcb";
        static const char *const ext_names[] = {
            "zicbom", "zicbop", "zicboz", "ziccamoa", "ziccif",
            "zicclsm", "ziccrse", "zicntr", "zicond", "zicsr", "zifencei",
            "zihintntl", "zihintpause", "zihpm", "zimop", "za64rs", "zawrs",
            "zcb", "zcmop", "zba", "zbb", "zbs", "sstc", "svadu",
        };
        char ext_list[192];
        char *r = ext_list;
        size_t j;
        for(j = 0; j < sizeof(ext_letters) - 1; j++) {
            if (misa & (1 << (ext_letters[j] - 'a'))) {
                *r++ = ext_letters[j];
                *r++ = '\0';
            }
        }
        for(j = 0; j < sizeof(ext_names) / sizeof(ext_names[0]); j++) {
            size_t len = strlen(ext_names[j]) + 1;
            memcpy(r, ext_names[j], len);
            r += len;
        }
        fdt_prop(s, "riscv,isa-extensions", ext_list, r - ext_list);
    }
    
    fdt_prop_u32(s, "riscv,cbom-block-size", 64);
    fdt_prop_u32(s, "riscv,cbop-block-size", 64);
    fdt_prop_u32(s, "riscv,cboz-block-size", 64);
    fdt_prop_str(s, "mmu-type", "riscv,sv39");
    fdt_prop_u32(s, "clock-frequency", 2000000000);
    cpu_phandle = cur_phandle++;
    fdt_prop_u32(s, "phandle", cpu_phandle);

    fdt_begin_node(s, "interrupt-controller");
    fdt_prop_u32(s, "#interrupt-cells", 1);
    fdt_prop(s, "interrupt-controller", NULL, 0);
    fdt_prop_str(s, "compatible", "riscv,cpu-intc");
    intc_phandle = cur_phandle++;
    fdt_prop_u32(s, "phandle", intc_phandle);
    fdt_end_node(s); /* interrupt-controller */
    
    fdt_end_node(s); /* cpu */

    fdt_begin_node(s, "cpu-map");
    fdt_begin_node(s, "cluster0");
    fdt_begin_node(s, "core0");
    fdt_prop_u32(s, "cpu", cpu_phandle);
    fdt_end_node(s); /* core0 */
    fdt_end_node(s); /* cluster0 */
    fdt_end_node(s); /* cpu-map */

    fdt_end_node(s); /* cpus */

    fdt_begin_node_num(s, "memory", RAM_BASE_ADDR);
    fdt_prop_str(s, "device_type", "memory");
    tab[0] = (uint64_t)RAM_BASE_ADDR >> 32;
    tab[1] = RAM_BASE_ADDR;
    tab[2] = m->ram_size >> 32;
    tab[3] = m->ram_size;
    fdt_prop_tab_u32(s, "reg", tab, 4);
    
    fdt_end_node(s); /* memory */

    syscon_phandle = cur_phandle++;

    fdt_begin_node(s, "soc");
    fdt_prop_u32(s, "#address-cells", 2);
    fdt_prop_u32(s, "#size-cells", 2);
    fdt_prop_str(s, "compatible", "simple-bus");
    fdt_prop(s, "ranges", NULL, 0);

    fdt_begin_node_num(s, "syscon", TEST_BASE_ADDR);
    fdt_prop_tab_str(s, "compatible",
                     "sifive,test1", "sifive,test0", "syscon", NULL);
    fdt_prop_tab_u64_2(s, "reg", TEST_BASE_ADDR, TEST_SIZE);
    fdt_prop_u32(s, "phandle", syscon_phandle);
    fdt_end_node(s); /* syscon */

    fdt_begin_node_num(s, "clint", CLINT_BASE_ADDR);
    fdt_prop_tab_str(s, "compatible", "sifive,clint0", "riscv,clint0", NULL);

    tab[0] = intc_phandle;
    tab[1] = 3; /* M IPI irq */
    tab[2] = intc_phandle;
    tab[3] = 7; /* M timer irq */
    fdt_prop_tab_u32(s, "interrupts-extended", tab, 4);

    fdt_prop_tab_u64_2(s, "reg", CLINT_BASE_ADDR, CLINT_SIZE);
    
    fdt_end_node(s); /* clint */

    fdt_begin_node_num(s, "interrupt-controller", PLIC_BASE_ADDR);
    fdt_prop_u32(s, "#interrupt-cells", 1);
    fdt_prop(s, "interrupt-controller", NULL, 0);
    fdt_prop_tab_str(s, "compatible",
                     "sifive,plic-1.0.0", "riscv,plic0", NULL);
    fdt_prop_u32(s, "riscv,ndev", 31);
    fdt_prop_tab_u64_2(s, "reg", PLIC_BASE_ADDR, PLIC_SIZE);

    tab[0] = intc_phandle;
    tab[1] = 11; /* M ext irq */
    tab[2] = intc_phandle;
    tab[3] = 9; /* S ext irq */
    fdt_prop_tab_u32(s, "interrupts-extended", tab, 4);

    plic_phandle = cur_phandle++;
    fdt_prop_u32(s, "phandle", plic_phandle);

    fdt_end_node(s); /* plic */

    fdt_begin_node_num(s, "serial", UART_BASE_ADDR);
    fdt_prop_str(s, "compatible", "ns16550a");
    fdt_prop_tab_u64_2(s, "reg", UART_BASE_ADDR, UART_SIZE);
    tab[0] = plic_phandle;
    tab[1] = UART_IRQ;
    fdt_prop_tab_u32(s, "interrupts-extended", tab, 2);
    fdt_prop_u32(s, "clock-frequency", UART_CLOCK);
    fdt_end_node(s); /* serial */

    for(i = 0; i < m->virtio_count; i++) {
        fdt_begin_node_num(s, "virtio", VIRTIO_BASE_ADDR + i * VIRTIO_SIZE);
        fdt_prop_str(s, "compatible", "virtio,mmio");
        fdt_prop_tab_u64_2(s, "reg", VIRTIO_BASE_ADDR + i * VIRTIO_SIZE,
                           VIRTIO_SIZE);
        tab[0] = plic_phandle;
        tab[1] = virtio_irq_num(i);
        fdt_prop_tab_u32(s, "interrupts-extended", tab, 2);
        fdt_end_node(s); /* virtio */
    }

    fb_dev = m->common.fb_dev;
    if (fb_dev) {
        fdt_begin_node_num(s, "framebuffer", FRAMEBUFFER_BASE_ADDR);
        fdt_prop_str(s, "compatible", "simple-framebuffer");
        fdt_prop_tab_u64_2(s, "reg", FRAMEBUFFER_BASE_ADDR, fb_dev->fb_size);
        fdt_prop_u32(s, "width", fb_dev->width);
        fdt_prop_u32(s, "height", fb_dev->height);
        fdt_prop_u32(s, "stride", fb_dev->stride);
        fdt_prop_str(s, "format", "a8r8g8b8");
        fdt_end_node(s); /* framebuffer */
    }
    
    fdt_end_node(s); /* soc */

    fdt_begin_node(s, "poweroff");
    fdt_prop_str(s, "compatible", "syscon-poweroff");
    fdt_prop_u32(s, "regmap", syscon_phandle);
    fdt_prop_u32(s, "offset", 0);
    fdt_prop_u32(s, "value", TEST_FINISHER_PASS);
    fdt_end_node(s); /* poweroff */

    fdt_begin_node(s, "chosen");
    fdt_prop_str(s, "bootargs", cmd_line ? cmd_line : "");
    fdt_prop_str(s, "stdout-path", "/soc/serial@10000000");
    if (initrd_size > 0) {
        fdt_prop_tab_u64(s, "linux,initrd-start", initrd_start);
        fdt_prop_tab_u64(s, "linux,initrd-end", initrd_start + initrd_size);
    }

    fdt_end_node(s); /* chosen */
    
    fdt_end_node(s); /* / */

    fdt_put32(s, FDT_END);
    dst = malloc(fdt_output_size(s));
    size = fdt_output(s, dst);
#if 0
    {
        FILE *f;
        f = fopen("/tmp/riscvemu.dtb", "wb");
        fwrite(dst, 1, size, f);
        fclose(f);
    }
#endif
    fdt_end(s);
    *pfdt_size = size;
    return dst;
}

static void copy_bios(RISCVMachine *s, const uint8_t *buf, int buf_len,
                      const uint8_t *kernel_buf, int kernel_buf_len,
                      const uint8_t *initrd_buf, int initrd_buf_len,
                      const char *cmd_line)
{
    uint64_t bios_end, fdt_addr, fdt_limit, fdt_offset;
    uint64_t initrd_base, initrd_end, kernel_base, kernel_end;
    uint8_t *fdt_buf;
    uint8_t *ram_ptr, *low_ptr;
    uint32_t *q;
    uint64_t *qd;
    int fdt_size;

    bios_end = buf_len;
    if (bios_end > s->ram_size) {
        vm_error("BIOS too big\n");
        exit(1);
    }

    kernel_base = 0;
    kernel_end = bios_end;
    if (kernel_buf_len > 0) {
        kernel_base = KERNEL_LOAD_OFFSET;
        if (bios_end > kernel_base) {
            vm_error("BIOS overlaps the kernel load address\n");
            exit(1);
        }
        kernel_end = kernel_base + kernel_buf_len;
        if (kernel_end > s->ram_size) {
            vm_error("kernel too big\n");
            exit(1);
        }
    }

    initrd_base = 0;
    initrd_end = 0;
    if (initrd_buf_len > 0) {
        /* same allocation as QEMU */
        initrd_base = s->ram_size / 2;
        if (initrd_base > (128 << 20))
            initrd_base = 128 << 20;
        initrd_end = initrd_base + initrd_buf_len;
        if (kernel_end > initrd_base) {
            vm_error("kernel overlaps initrd\n");
            exit(1);
        }
        if (initrd_end > s->ram_size) {
            vm_error("initrd too big\n");
            exit(1);
        }
    }

    fdt_buf = riscv_build_fdt(s, &fdt_size,
                              RAM_BASE_ADDR + initrd_base, initrd_buf_len,
                              cmd_line);
    /* Keep the FDT high in RAM and below 3 GB, as on QEMU virt. */
    fdt_limit = s->ram_size;
    if (fdt_limit > FDT_MAX_OFFSET)
        fdt_limit = FDT_MAX_OFFSET;
    if ((uint64_t)fdt_size > fdt_limit) {
        free(fdt_buf);
        vm_error("not enough RAM for the device tree\n");
        exit(1);
    }
    fdt_offset = (fdt_limit - fdt_size) & ~(FDT_ALIGN - 1);
    if (fdt_offset < kernel_end ||
        (initrd_buf_len > 0 && initrd_base < fdt_offset + fdt_size &&
         fdt_offset < initrd_end)) {
        free(fdt_buf);
        vm_error("not enough RAM for the device tree\n");
        exit(1);
    }
    fdt_addr = RAM_BASE_ADDR + fdt_offset;

    ram_ptr = get_ram_ptr(s, RAM_BASE_ADDR, TRUE);
    memcpy(ram_ptr, buf, buf_len);
    if (kernel_buf_len > 0)
        memcpy(ram_ptr + kernel_base, kernel_buf, kernel_buf_len);
    if (initrd_buf_len > 0)
        memcpy(ram_ptr + initrd_base, initrd_buf, initrd_buf_len);
    memcpy(ram_ptr + fdt_offset, fdt_buf, fdt_size);
    free(fdt_buf);

    /* Reset vector at 0x1000, as on QEMU virt: enter the firmware at
       RAM_BASE_ADDR with a0 = mhartid and a1 = FDT address. The
       targets are loaded from literals so any 64-bit address works. */
    low_ptr = get_ram_ptr(s, 0, TRUE);
    q = (uint32_t *)(low_ptr + 0x1000);
    q[0] = 0x00000297; /* auipc t0, 0 */
    q[1] = 0x0182b283; /* ld t0, 24(t0) */
    q[2] = 0x00000597; /* auipc a1, 0 */
    q[3] = 0x0185b583; /* ld a1, 24(a1) */
    q[4] = 0xf1402573; /* csrr a0, mhartid */
    q[5] = 0x00028067; /* jalr zero, 0(t0) */
    qd = (uint64_t *)(low_ptr + 0x1018);
    qd[0] = RAM_BASE_ADDR;
    qd[1] = fdt_addr;
}

static void riscv_flush_tlb_write_range(void *opaque, uint8_t *ram_addr,
                                        size_t ram_size)
{
    RISCVMachine *s = opaque;
    riscv_cpu_flush_tlb_write_range_ram(s->cpu_state, ram_addr, ram_size);
}

static VirtMachine *riscv_machine_init(const VirtMachineParams *p)
{
    RISCVMachine *s;
    VIRTIODevice *blk_dev;
    int i, ram_flags;
    VIRTIOBusDef vbus_s, *vbus = &vbus_s;


    if (strcmp(p->machine_name, "riscv64") != 0) {
        vm_error("unsupported machine: %s\n", p->machine_name);
        return NULL;
    }
    
    s = mallocz(sizeof(*s));
    s->common.vmc = p->vmc;
    s->ram_size = p->ram_size;
    s->mem_map = phys_mem_map_init();
    /* needed to handle the RAM dirty bits */
    s->mem_map->opaque = s;
    s->mem_map->flush_tlb_write_range = riscv_flush_tlb_write_range;

    s->cpu_state = riscv_cpu_init(s->mem_map);
    if (!s->cpu_state) {
        vm_error("unable to initialize the RV64 CPU\n");
        /* XXX: should free resources */
        return NULL;
    }
    /* RAM */
    ram_flags = 0;
    cpu_register_ram(s->mem_map, RAM_BASE_ADDR, p->ram_size, ram_flags);
    cpu_register_ram(s->mem_map, 0x00000000, LOW_RAM_SIZE, 0);
    s->rtc_real_time = p->rtc_real_time;
    if (p->rtc_real_time) {
        s->rtc_start_time = rtc_get_real_time();
    }
    riscv_cpu_set_time_source(s->cpu_state, rtc_get_time_for_cpu, s);
    
    cpu_register_device(s->mem_map, CLINT_BASE_ADDR, CLINT_SIZE, s,
                        clint_read, clint_write, DEVIO_SIZE32);
    cpu_register_device(s->mem_map, PLIC_BASE_ADDR, PLIC_SIZE, s,
                        plic_read, plic_write, DEVIO_SIZE32);
    for(i = 1; i < 32; i++) {
        irq_init(&s->plic_irq[i], plic_set_irq, s, i);
    }

    cpu_register_device(s->mem_map, TEST_BASE_ADDR, TEST_SIZE,
                        s, test_read, test_write, DEVIO_SIZE32);
    s->common.console = p->console;
    s->console_type = p->console_type;
    s->uart_output = p->uart_output;

    s->uart_dev = uart16550_init(s->mem_map, UART_BASE_ADDR, UART_SIZE,
                                 &s->plic_irq[UART_IRQ],
                                 uart_tx_func, s);

    memset(vbus, 0, sizeof(*vbus));
    vbus->mem_map = s->mem_map;
    vbus->addr = VIRTIO_BASE_ADDR;

    /* virtio console */
    if (p->console && p->console_type == VM_CONSOLE_VIRTIO) {
        vbus->irq = &s->plic_irq[virtio_irq_num(s->virtio_count)];
        s->virtio_console_dev = virtio_console_init(vbus, p->console);
        vbus->addr += VIRTIO_SIZE;
        s->virtio_count++;
    }
    
    /* virtio net device */
    for(i = 0; i < p->eth_count; i++) {
        vbus->irq = &s->plic_irq[virtio_irq_num(s->virtio_count)];
        virtio_net_init(vbus, p->tab_eth[i].net);
        s->common.net = p->tab_eth[i].net;
        vbus->addr += VIRTIO_SIZE;
        s->virtio_count++;
    }

    /* virtio block device */
    for(i = 0; i < p->drive_count; i++) {
        vbus->irq = &s->plic_irq[virtio_irq_num(s->virtio_count)];
        blk_dev = virtio_block_init(vbus, p->tab_drive[i].block_dev);
        (void)blk_dev;
        vbus->addr += VIRTIO_SIZE;
        s->virtio_count++;
    }

    /* virtio filesystem */
    for(i = 0; i < p->fs_count; i++) {
        VIRTIODevice *fs_dev;
        vbus->irq = &s->plic_irq[virtio_irq_num(s->virtio_count)];
        fs_dev = virtio_9p_init(vbus, p->tab_fs[i].fs_dev,
                                p->tab_fs[i].tag);
        (void)fs_dev;
        //        virtio_set_debug(fs_dev, VIRTIO_DEBUG_9P);
        vbus->addr += VIRTIO_SIZE;
        s->virtio_count++;
    }

    if (p->display_device) {
        FBDevice *fb_dev;
        fb_dev = mallocz(sizeof(*fb_dev));
        s->common.fb_dev = fb_dev;
        if (!strcmp(p->display_device, "simplefb")) {
            simplefb_init(s->mem_map,
                          FRAMEBUFFER_BASE_ADDR,
                          fb_dev,
                          p->width, p->height);
            
        } else {
            vm_error("unsupported display device: %s\n", p->display_device);
            exit(1);
        }
    }

    if (p->input_device) {
        if (!strcmp(p->input_device, "virtio")) {
            vbus->irq = &s->plic_irq[virtio_irq_num(s->virtio_count)];
            s->keyboard_dev = virtio_input_init(vbus,
                                                VIRTIO_INPUT_TYPE_KEYBOARD);
            vbus->addr += VIRTIO_SIZE;
            s->virtio_count++;

            vbus->irq = &s->plic_irq[virtio_irq_num(s->virtio_count)];
            s->mouse_dev = virtio_input_init(vbus,
                                             VIRTIO_INPUT_TYPE_TABLET);
            vbus->addr += VIRTIO_SIZE;
            s->virtio_count++;
        } else {
            vm_error("unsupported input device: %s\n", p->input_device);
            exit(1);
        }
    }
    
    if (!p->files[VM_FILE_BIOS].buf) {
        vm_error("No bios found");
    }

    copy_bios(s, p->files[VM_FILE_BIOS].buf, p->files[VM_FILE_BIOS].len,
              p->files[VM_FILE_KERNEL].buf, p->files[VM_FILE_KERNEL].len,
              p->files[VM_FILE_INITRD].buf, p->files[VM_FILE_INITRD].len,
              p->cmdline);
    
    return (VirtMachine *)s;
}

static void riscv_machine_end(VirtMachine *s1)
{
    RISCVMachine *s = (RISCVMachine *)s1;
    /* XXX: stop all */
    riscv_cpu_end(s->cpu_state);
    phys_mem_map_end(s->mem_map);
    uart16550_end(s->uart_dev);
    free(s);
}

static int limit_timer_delay(int delay, uint64_t compare, uint64_t now)
{
    uint64_t delay_ms;

    if (compare <= now)
        return 0;
    delay_ms = (compare - now) / (RTC_FREQ / 1000);
    if (delay_ms < (uint64_t)delay)
        return delay_ms;
    return delay;
}

/* in ms */
static int riscv_machine_get_sleep_duration(VirtMachine *s1, int delay)
{
    RISCVMachine *m = (RISCVMachine *)s1;
    RISCVCPUState *s = m->cpu_state;
    uint64_t now;
    
    now = rtc_get_time(m);
    if (!(riscv_cpu_get_mip(s) & MIP_MTIP)) {
        if (m->timecmp <= now) {
            riscv_cpu_set_mip(s, MIP_MTIP);
            delay = 0;
        } else {
            delay = limit_timer_delay(delay, m->timecmp, now);
        }
    }
    riscv_cpu_update_time(s, now);
    if (!(riscv_cpu_get_mip(s) & MIP_STIP))
        delay = limit_timer_delay(delay, riscv_cpu_get_stimecmp(s), now);
    if (!riscv_cpu_get_power_down(s))
        delay = 0;
    return delay;
}

static void riscv_machine_interp(VirtMachine *s1, int max_exec_cycle)
{
    RISCVMachine *s = (RISCVMachine *)s1;
    riscv_cpu_interp(s->cpu_state, max_exec_cycle);
}

static void riscv_vm_send_key_event(VirtMachine *s1, BOOL is_down,
                                    uint16_t key_code)
{
    RISCVMachine *s = (RISCVMachine *)s1;
    if (s->keyboard_dev) {
        virtio_input_send_key_event(s->keyboard_dev, is_down, key_code);
    }
}

static int riscv_vm_console_receive_space(VirtMachine *s1)
{
    RISCVMachine *s = (RISCVMachine *)s1;

    if (!s->common.console)
        return 0;
    if (s->console_type == VM_CONSOLE_UART)
        return uart16550_receive_space(s->uart_dev);
    if (s->virtio_console_dev)
        return virtio_console_get_write_len(s->virtio_console_dev);
    return 0;
}

static int riscv_vm_console_receive(VirtMachine *s1,
                                    const uint8_t *buf, int len)
{
    RISCVMachine *s = (RISCVMachine *)s1;

    if (s->console_type == VM_CONSOLE_UART)
        return uart16550_receive(s->uart_dev, buf, len);
    if (s->virtio_console_dev)
        return virtio_console_write_data(s->virtio_console_dev, buf, len);
    return 0;
}

static void riscv_vm_console_resize(VirtMachine *s1, int width, int height)
{
    RISCVMachine *s = (RISCVMachine *)s1;

    if (s->console_type == VM_CONSOLE_VIRTIO && s->virtio_console_dev)
        virtio_console_resize_event(s->virtio_console_dev, width, height);
}

static BOOL riscv_vm_mouse_is_absolute(VirtMachine *s)
{
    (void)s;
    return TRUE;
}

static void riscv_vm_send_mouse_event(VirtMachine *s1, int dx, int dy, int dz,
                                      unsigned int buttons)
{
    RISCVMachine *s = (RISCVMachine *)s1;
    if (s->mouse_dev) {
        virtio_input_send_mouse_event(s->mouse_dev, dx, dy, dz, buttons);
    }
}

const VirtMachineClass riscv_machine_class = {
    riscv_machine_init,
    riscv_machine_end,
    riscv_machine_get_sleep_duration,
    riscv_machine_interp,
    riscv_vm_mouse_is_absolute,
    riscv_vm_send_mouse_event,
    riscv_vm_send_key_event,
    riscv_vm_console_receive_space,
    riscv_vm_console_receive,
    riscv_vm_console_resize,
};
