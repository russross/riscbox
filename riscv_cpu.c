/*
 * RISCV CPU emulator
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

#include "cutils.h"
#include "iomem.h"
#include "riscv_cpu.h"

#define MAX_XLEN 64
//#define DUMP_INVALID_MEM_ACCESS
//#define DUMP_MMU_EXCEPTIONS
//#define DUMP_INTERRUPTS
//#define DUMP_INVALID_CSR
//#define DUMP_EXCEPTIONS
//#define DUMP_CSR
//#define CONFIG_LOGFILE

#include "riscv_cpu_priv.h"

#if FLEN > 0
#include "softfp.h"
#endif

#ifdef USE_GLOBAL_STATE
static RISCVCPUState riscv_cpu_global_state;
#endif
#ifdef USE_GLOBAL_VARIABLES
#define code_ptr s->__code_ptr
#define code_end s->__code_end
#define code_to_pc_addend s->__code_to_pc_addend
#endif

static void log_vprintf(const char *fmt, va_list ap)
    __attribute__((format(printf, 1, 0)));

#ifdef CONFIG_LOGFILE
static FILE *log_file;

static void log_vprintf(const char *fmt, va_list ap)
{
    if (!log_file)
        log_file = fopen("/tmp/riscemu.log", "wb");
    vfprintf(log_file, fmt, ap);
}
#else
static void log_vprintf(const char *fmt, va_list ap)
{
    vprintf(fmt, ap);
}
#endif

static void __attribute__((format(printf, 1, 2), unused)) log_printf(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    log_vprintf(fmt, ap);
    va_end(ap);
}

static void fprint_target_ulong(FILE *f, target_ulong a)
{
    fprintf(f, "%" PR_target_ulong, a);
}

static void print_target_ulong(target_ulong a)
{
    fprint_target_ulong(stdout, a);
}

static char *reg_name[32] = {
"zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2",
"s0", "s1", "a0", "a1", "a2", "a3", "a4", "a5",
"a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7",
"s8", "s9", "s10", "s11", "t3", "t4", "t5", "t6"
};

static void dump_regs(RISCVCPUState *s)
{
    int i, cols;
    const char priv_str[4] = "USHM";
    cols = 4;
    printf("pc =");
    print_target_ulong(s->pc);
    printf(" ");
    for(i = 1; i < 32; i++) {
        printf("%-3s=", reg_name[i]);
        print_target_ulong(s->reg[i]);
        if ((i & (cols - 1)) == (cols - 1))
            printf("\n");
        else
            printf(" ");
    }
    printf("priv=%c", priv_str[s->priv]);
    printf(" mstatus=");
    print_target_ulong(s->mstatus);
    printf(" cycles=%" PRId64, s->elapsed_cycles);
    printf("\n");
#if 1
    printf(" mideleg=");
    print_target_ulong(s->mideleg);
    printf(" mie=");
    print_target_ulong(s->mie);
    printf(" mip=");
    print_target_ulong(s->mip);
    printf("\n");
#endif
}

static __attribute__((unused)) void cpu_abort(RISCVCPUState *s)
{
    dump_regs(s);
    abort();
}

#define PTE_V_MASK (1 << 0)
#define PTE_U_MASK (1 << 4)
#define PTE_A_MASK (1 << 6)
#define PTE_D_MASK (1 << 7)
#define PTE_HIGH_RESERVED_MASK ((uint64_t)0x3ff << 54)
#define PTE_NONLEAF_RESERVED_MASK (PTE_U_MASK | PTE_A_MASK | PTE_D_MASK)

#define SATP_MODE_SHIFT 60
#define SATP_MODE_MASK 0xf
#define SATP_MODE_BARE 0
#define SATP_MODE_SV39 8
#define SATP_PPN_MASK (((uint64_t)1 << 44) - 1)

#define SV39_LEVELS 3
#define SV39_VPN_BITS 9
#define SV39_VADDR_BITS (PG_SHIFT + SV39_LEVELS * SV39_VPN_BITS)

#define ACCESS_READ  0
#define ACCESS_WRITE 1
#define ACCESS_CODE  2

#define MENVCFG_ADUE ((target_ulong)1 << 61)
#define MENVCFG_STCE ((target_ulong)1 << 63)
#define MENVCFG_MASK (MENVCFG_ADUE | MENVCFG_STCE)

#define PMP_CFG_R       (1 << 0)
#define PMP_CFG_W       (1 << 1)
#define PMP_CFG_X       (1 << 2)
#define PMP_CFG_A_SHIFT 3
#define PMP_CFG_A_MASK  (3 << PMP_CFG_A_SHIFT)
#define PMP_CFG_A_TOR   (1 << PMP_CFG_A_SHIFT)
#define PMP_CFG_A_NA4   (2 << PMP_CFG_A_SHIFT)
#define PMP_CFG_A_NAPOT (3 << PMP_CFG_A_SHIFT)
#define PMP_CFG_L       (1 << 7)
#define PMP_ADDR_MASK   (((target_ulong)1 << 54) - 1)

typedef enum {
    TRANSLATE_OK,
    TRANSLATE_PAGE_FAULT,
    TRANSLATE_ACCESS_FAULT,
} TranslationResult;

static int get_effective_priv(RISCVCPUState *s, int access)
{
    if ((s->mstatus & MSTATUS_MPRV) && access != ACCESS_CODE)
        return (s->mstatus >> MSTATUS_MPP_SHIFT) & 3;
    return s->priv;
}

static BOOL pmp_access_ok(RISCVCPUState *s, target_ulong paddr,
                          target_ulong size, int access, int priv)
{
    target_ulong access_end, lower, upper, addr, mask, region_size;
    int i, mode, trailing_ones;
    uint8_t cfg;

    access_end = paddr + size;
    if (access_end < paddr)
        return FALSE;

    for(i = 0; i < PMP_ENTRY_COUNT; i++) {
        cfg = s->pmpcfg[i];
        mode = cfg & PMP_CFG_A_MASK;
        if (mode == 0)
            continue;

        addr = s->pmpaddr[i];
        if (mode == PMP_CFG_A_TOR) {
            lower = i == 0 ? 0 : s->pmpaddr[i - 1] << 2;
            upper = addr << 2;
        } else if (mode == PMP_CFG_A_NA4) {
            lower = addr << 2;
            upper = lower + 4;
        } else {
            trailing_ones = __builtin_ctzll(~addr);
            mask = ((target_ulong)1 << trailing_ones) - 1;
            lower = (addr & ~mask) << 2;
            region_size = (target_ulong)1 << (trailing_ones + 3);
            upper = lower + region_size;
        }

        if (paddr >= upper || access_end <= lower)
            continue;
        if (paddr < lower || access_end > upper)
            return FALSE;
        if (priv == PRV_M && !(cfg & PMP_CFG_L))
            return TRUE;
        return (cfg & (1 << access)) != 0;
    }
    return priv == PRV_M;
}

static BOOL phys_ram_access_ok(RISCVCPUState *s, PhysMemoryRange **ppr,
                               target_ulong paddr, size_t size, int access)
{
    PhysMemoryRange *pr;

    if (!pmp_access_ok(s, paddr, size, access, PRV_S))
        return FALSE;
    pr = get_phys_mem_range(s->mem_map, paddr);
    if (!pr || !pr->is_ram || size > pr->size ||
        paddr - pr->addr > pr->size - size)
        return FALSE;
    *ppr = pr;
    return TRUE;
}

static BOOL phys_read_pte(RISCVCPUState *s, uint64_t *pval,
                          target_ulong paddr)
{
    PhysMemoryRange *pr;

    if (!phys_ram_access_ok(s, &pr, paddr, sizeof(*pval), ACCESS_READ))
        return FALSE;
    *pval = *(uint64_t *)(pr->phys_mem + (uintptr_t)(paddr - pr->addr));
    return TRUE;
}

static BOOL phys_write_pte(RISCVCPUState *s, target_ulong paddr,
                           uint64_t val)
{
    PhysMemoryRange *pr;

    if (!phys_ram_access_ok(s, &pr, paddr, sizeof(val), ACCESS_WRITE) ||
        (pr->devram_flags & DEVRAM_FLAG_ROM))
        return FALSE;
    phys_mem_set_dirty_bit(pr, paddr - pr->addr);
    *(uint64_t *)(pr->phys_mem + (uintptr_t)(paddr - pr->addr)) = val;
    return TRUE;
}

/* access = 0: read, 1 = write, 2 = code */
static TranslationResult get_phys_addr(RISCVCPUState *s,
                                       target_ulong *ppaddr,
                                       target_ulong vaddr,
                                       int size, int access)
{
    int pte_idx, xwr, priv;
    int need_write, vaddr_shift, i;
    target_ulong pte_addr, pte, vaddr_mask, paddr, vaddr_high;

    priv = get_effective_priv(s, access);

    if (priv == PRV_M) {
        paddr = vaddr;
        goto pmp_check;
    }
    if ((s->satp >> SATP_MODE_SHIFT) == SATP_MODE_BARE) {
        /* bare: no translation */
        paddr = vaddr;
        goto pmp_check;
    }

    vaddr_high = vaddr >> (SV39_VADDR_BITS - 1);
    if (vaddr_high != 0 &&
        vaddr_high != (target_ulong)-1 >> (SV39_VADDR_BITS - 1))
        return TRANSLATE_PAGE_FAULT;

    pte_addr = (s->satp & SATP_PPN_MASK) << PG_SHIFT;
    for(i = 0; i < SV39_LEVELS; i++) {
        vaddr_shift = PG_SHIFT + SV39_VPN_BITS * (SV39_LEVELS - 1 - i);
        pte_idx = (vaddr >> vaddr_shift) & ((1 << SV39_VPN_BITS) - 1);
        pte_addr += pte_idx * sizeof(uint64_t);
        if (!phys_read_pte(s, &pte, pte_addr))
            return TRANSLATE_ACCESS_FAULT;
        //printf("pte=0x%08" PRIx64 "\n", pte);
        if (!(pte & PTE_V_MASK) || (pte & PTE_HIGH_RESERVED_MASK))
            return TRANSLATE_PAGE_FAULT; /* invalid PTE */
        paddr = (pte >> 10) << PG_SHIFT;
        xwr = (pte >> 1) & 7;
        if (xwr != 0) {
            if (xwr == 2 || xwr == 6)
                return TRANSLATE_PAGE_FAULT;
            vaddr_mask = ((target_ulong)1 << vaddr_shift) - 1;
            if (paddr & vaddr_mask)
                return TRANSLATE_PAGE_FAULT;
            /* priviledge check */
            if (priv == PRV_S) {
                if ((pte & PTE_U_MASK) && !(s->mstatus & MSTATUS_SUM))
                    return TRANSLATE_PAGE_FAULT;
            } else {
                if (!(pte & PTE_U_MASK))
                    return TRANSLATE_PAGE_FAULT;
            }
            /* protection check */
            /* MXR allows read access to execute-only pages */
            if (s->mstatus & MSTATUS_MXR)
                xwr |= (xwr >> 2);

            if (((xwr >> access) & 1) == 0)
                return TRANSLATE_PAGE_FAULT;
            need_write = !(pte & PTE_A_MASK) ||
                (!(pte & PTE_D_MASK) && access == ACCESS_WRITE);
            if (need_write && !(s->menvcfg & MENVCFG_ADUE))
                return TRANSLATE_PAGE_FAULT;
            pte |= PTE_A_MASK;
            if (access == ACCESS_WRITE)
                pte |= PTE_D_MASK;
            if (need_write && !phys_write_pte(s, pte_addr, pte))
                return TRANSLATE_ACCESS_FAULT;
            paddr = (vaddr & vaddr_mask) | (paddr & ~vaddr_mask);
            goto pmp_check;
        } else {
            if (pte & PTE_NONLEAF_RESERVED_MASK)
                return TRANSLATE_PAGE_FAULT;
            pte_addr = paddr;
        }
    }
    return TRANSLATE_PAGE_FAULT;

pmp_check:
    if (!pmp_access_ok(s, paddr, size, access, priv))
        return TRANSLATE_ACCESS_FAULT;
    *ppaddr = paddr;
    return TRANSLATE_OK;
}

/* return 0 if OK, != 0 if exception */
int target_read_slow(RISCVCPUState *s, mem_uint_t *pval,
                     target_ulong addr, int size_log2)
{
    int size, tlb_idx, err, al;
    TranslationResult translation_result;
    target_ulong paddr, offset;
    uint8_t *ptr;
    PhysMemoryRange *pr;
    mem_uint_t ret;

    /* first handle unaligned accesses */
    size = 1 << size_log2;
    al = addr & (size - 1);
    if (al != 0) {
        switch(size_log2) {
        case 1:
            {
                uint8_t v0, v1;
                err = target_read_u8(s, &v0, addr);
                if (err)
                    return err;
                err = target_read_u8(s, &v1, addr + 1);
                if (err)
                    return err;
                ret = v0 | (v1 << 8);
            }
            break;
        case 2:
            {
                uint32_t v0, v1;
                addr -= al;
                err = target_read_u32(s, &v0, addr);
                if (err)
                    return err;
                err = target_read_u32(s, &v1, addr + 4);
                if (err)
                    return err;
                ret = (v0 >> (al * 8)) | (v1 << (32 - al * 8));
            }
            break;
        case 3:
            {
                uint64_t v0, v1;
                addr -= al;
                err = target_read_u64(s, &v0, addr);
                if (err)
                    return err;
                err = target_read_u64(s, &v1, addr + 8);
                if (err)
                    return err;
                ret = (v0 >> (al * 8)) | (v1 << (64 - al * 8));
            }
            break;
        default:
            abort();
        }
    } else {
        translation_result = get_phys_addr(s, &paddr, addr, size,
                                           ACCESS_READ);
        if (translation_result != TRANSLATE_OK) {
            s->pending_tval = addr;
            s->pending_exception =
                translation_result == TRANSLATE_ACCESS_FAULT ?
                CAUSE_FAULT_LOAD : CAUSE_LOAD_PAGE_FAULT;
            return -1;
        }
        pr = get_phys_mem_range(s->mem_map, paddr);
        if (!pr) {
#ifdef DUMP_INVALID_MEM_ACCESS
            printf("target_read_slow: invalid physical address 0x");
            print_target_ulong(paddr);
            printf("\n");
#endif
            return 0;
        } else if (pr->is_ram) {
            tlb_idx = (addr >> PG_SHIFT) & (TLB_SIZE - 1);
            ptr = pr->phys_mem + (uintptr_t)(paddr - pr->addr);
            if (pmp_access_ok(s, paddr & ~PG_MASK, PG_MASK + 1,
                              ACCESS_READ,
                              get_effective_priv(s, ACCESS_READ))) {
                s->tlb_read[tlb_idx].vaddr = addr & ~PG_MASK;
                s->tlb_read[tlb_idx].mem_addend = (uintptr_t)ptr - addr;
            }
            switch(size_log2) {
            case 0:
                ret = *(uint8_t *)ptr;
                break;
            case 1:
                ret = *(uint16_t *)ptr;
                break;
            case 2:
                ret = *(uint32_t *)ptr;
                break;
            case 3:
                ret = *(uint64_t *)ptr;
                break;
            default:
                abort();
            }
        } else {
            offset = paddr - pr->addr;
            if (((pr->devio_flags >> size_log2) & 1) != 0) {
                ret = pr->read_func(pr->opaque, offset, size_log2);
            }
            else if ((pr->devio_flags & DEVIO_SIZE32) && size_log2 == 3) {
                /* emulate 64 bit access */
                ret = pr->read_func(pr->opaque, offset, 2);
                ret |= (uint64_t)pr->read_func(pr->opaque, offset + 4, 2) << 32;
                
            }
            else {
#ifdef DUMP_INVALID_MEM_ACCESS
                printf("unsupported device read access: addr=0x");
                print_target_ulong(paddr);
                printf(" width=%d bits\n", 1 << (3 + size_log2));
#endif
                ret = 0;
            }
        }
    }
    *pval = ret;
    return 0;
}

/* return 0 if OK, != 0 if exception */
int target_write_slow(RISCVCPUState *s, target_ulong addr,
                      mem_uint_t val, int size_log2)
{
    int size, i, tlb_idx, err;
    TranslationResult translation_result;
    target_ulong paddr, offset;
    uint8_t *ptr;
    PhysMemoryRange *pr;
    
    /* first handle unaligned accesses */
    size = 1 << size_log2;
    if ((addr & (size - 1)) != 0) {
        /* XXX: should avoid modifying the memory in case of exception */
        for(i = 0; i < size; i++) {
            err = target_write_u8(s, addr + i, (val >> (8 * i)) & 0xff);
            if (err)
                return err;
        }
    } else {
        translation_result = get_phys_addr(s, &paddr, addr, size,
                                           ACCESS_WRITE);
        if (translation_result != TRANSLATE_OK) {
            s->pending_tval = addr;
            s->pending_exception =
                translation_result == TRANSLATE_ACCESS_FAULT ?
                CAUSE_FAULT_STORE : CAUSE_STORE_PAGE_FAULT;
            return -1;
        }
        pr = get_phys_mem_range(s->mem_map, paddr);
        if (!pr) {
#ifdef DUMP_INVALID_MEM_ACCESS
            printf("target_write_slow: invalid physical address 0x");
            print_target_ulong(paddr);
            printf("\n");
#endif
        } else if (pr->is_ram) {
            phys_mem_set_dirty_bit(pr, paddr - pr->addr);
            tlb_idx = (addr >> PG_SHIFT) & (TLB_SIZE - 1);
            ptr = pr->phys_mem + (uintptr_t)(paddr - pr->addr);
            if (pmp_access_ok(s, paddr & ~PG_MASK, PG_MASK + 1,
                              ACCESS_WRITE,
                              get_effective_priv(s, ACCESS_WRITE))) {
                s->tlb_write[tlb_idx].vaddr = addr & ~PG_MASK;
                s->tlb_write[tlb_idx].mem_addend = (uintptr_t)ptr - addr;
            }
            switch(size_log2) {
            case 0:
                *(uint8_t *)ptr = val;
                break;
            case 1:
                *(uint16_t *)ptr = val;
                break;
            case 2:
                *(uint32_t *)ptr = val;
                break;
            case 3:
                *(uint64_t *)ptr = val;
                break;
            default:
                abort();
            }
        } else {
            offset = paddr - pr->addr;
            if (((pr->devio_flags >> size_log2) & 1) != 0) {
                pr->write_func(pr->opaque, offset, val, size_log2);
            }
            else if ((pr->devio_flags & DEVIO_SIZE32) && size_log2 == 3) {
                /* emulate 64 bit access */
                pr->write_func(pr->opaque, offset,
                               val & 0xffffffff, 2);
                pr->write_func(pr->opaque, offset + 4,
                               (val >> 32) & 0xffffffff, 2);
            }
            else {
#ifdef DUMP_INVALID_MEM_ACCESS
                printf("unsupported device write access: addr=0x");
                print_target_ulong(paddr);
                printf(" width=%d bits\n", 1 << (3 + size_log2));
#endif
            }
        }
    }
    return 0;
}

static __exception int target_write_check(RISCVCPUState *s,
                                          target_ulong addr, int size)
{
    target_ulong paddr;
    TranslationResult translation_result;

    translation_result = get_phys_addr(s, &paddr, addr, size, ACCESS_WRITE);
    if (translation_result != TRANSLATE_OK) {
        s->pending_tval = addr;
        s->pending_exception =
            translation_result == TRANSLATE_ACCESS_FAULT ?
            CAUSE_FAULT_STORE : CAUSE_STORE_PAGE_FAULT;
        return -1;
    }
    return 0;
}

struct __attribute__((packed)) unaligned_u32 {
    uint32_t u32;
};

/* unaligned access at an address known to be a multiple of 2 */
static uint32_t get_insn32(uint8_t *ptr)
{
#if defined(EMSCRIPTEN)
    return ((uint16_t *)ptr)[0] | ((uint32_t)((uint16_t *)ptr)[1] << 16);
#else
    return ((struct unaligned_u32 *)ptr)->u32;
#endif
}

/* return 0 if OK, != 0 if exception */
static no_inline __exception int target_read_insn_slow(RISCVCPUState *s,
                                                       uint8_t **pptr,
                                                       int *pspan,
                                                       target_ulong addr)
{
    int tlb_idx;
    target_ulong paddr;
    TranslationResult translation_result;
    uint8_t *ptr;
    PhysMemoryRange *pr;
    
    translation_result = get_phys_addr(s, &paddr, addr, sizeof(uint16_t),
                                       ACCESS_CODE);
    if (translation_result != TRANSLATE_OK) {
        s->pending_tval = addr;
        s->pending_exception =
            translation_result == TRANSLATE_ACCESS_FAULT ?
            CAUSE_FAULT_FETCH : CAUSE_FETCH_PAGE_FAULT;
        return -1;
    }
    pr = get_phys_mem_range(s->mem_map, paddr);
    if (!pr || !pr->is_ram) {
        /* XXX: we only access to execute code from RAM */
        s->pending_tval = addr;
        s->pending_exception = CAUSE_FAULT_FETCH;
        return -1;
    }
    tlb_idx = (addr >> PG_SHIFT) & (TLB_SIZE - 1);
    ptr = pr->phys_mem + (uintptr_t)(paddr - pr->addr);
    *pspan = sizeof(uint16_t);
    if (pmp_access_ok(s, paddr & ~PG_MASK, PG_MASK + 1, ACCESS_CODE,
                      get_effective_priv(s, ACCESS_CODE))) {
        s->tlb_code[tlb_idx].vaddr = addr & ~PG_MASK;
        s->tlb_code[tlb_idx].mem_addend = (uintptr_t)ptr - addr;
        *pspan = (PG_MASK + 1) - (addr & PG_MASK);
    }
    *pptr = ptr;
    return 0;
}

/* addr must be aligned */
static inline __exception int target_read_insn_u16(RISCVCPUState *s, uint16_t *pinsn,
                                                   target_ulong addr)
{
    int span;
    uint32_t tlb_idx;
    uint8_t *ptr;
    
    tlb_idx = (addr >> PG_SHIFT) & (TLB_SIZE - 1);
    if (likely(s->tlb_code[tlb_idx].vaddr == (addr & ~PG_MASK))) {
        ptr = (uint8_t *)(s->tlb_code[tlb_idx].mem_addend +
                          (uintptr_t)addr);
    } else {
        if (target_read_insn_slow(s, &ptr, &span, addr))
            return -1;
    }
    *pinsn = *(uint16_t *)ptr;
    return 0;
}

static void tlb_init(RISCVCPUState *s)
{
    int i;
    
    for(i = 0; i < TLB_SIZE; i++) {
        s->tlb_read[i].vaddr = -1;
        s->tlb_write[i].vaddr = -1;
        s->tlb_code[i].vaddr = -1;
    }
}

static void tlb_flush_all(RISCVCPUState *s)
{
    tlb_init(s);
}

static void tlb_flush_vaddr(RISCVCPUState *s, target_ulong vaddr)
{
    (void)vaddr;
    tlb_flush_all(s);
}

/* XXX: inefficient but not critical as long as it is seldom used */
static void glue(riscv_cpu_flush_tlb_write_range_ram,
                 MAX_XLEN)(RISCVCPUState *s,
                           uint8_t *ram_ptr, size_t ram_size)
{
    uint8_t *ptr, *ram_end;
    int i;
    
    ram_end = ram_ptr + ram_size;
    for(i = 0; i < TLB_SIZE; i++) {
        if (s->tlb_write[i].vaddr != (target_ulong)-1) {
            ptr = (uint8_t *)(s->tlb_write[i].mem_addend +
                              (uintptr_t)s->tlb_write[i].vaddr);
            if (ptr >= ram_ptr && ptr < ram_end) {
                s->tlb_write[i].vaddr = -1;
            }
        }
    }
}


#define SSTATUS_MASK0 (MSTATUS_UIE | MSTATUS_SIE |       \
                      MSTATUS_UPIE | MSTATUS_SPIE |     \
                      MSTATUS_SPP | \
                      MSTATUS_FS | MSTATUS_XS | \
                      MSTATUS_SUM | MSTATUS_MXR | MSTATUS_UXL)
#define SSTATUS_MASK SSTATUS_MASK0


#define MSTATUS_MASK (MSTATUS_UIE | MSTATUS_SIE | MSTATUS_MIE |      \
                      MSTATUS_UPIE | MSTATUS_SPIE | MSTATUS_MPIE |    \
                      MSTATUS_SPP | MSTATUS_MPP | \
                      MSTATUS_FS | \
                      MSTATUS_MPRV | MSTATUS_SUM | MSTATUS_MXR | \
                      MSTATUS_TVM | MSTATUS_TW | MSTATUS_TSR)

/* cycle and insn counters */
#define COUNTEREN_MASK ((1 << 0) | (1 << 1) | (1 << 2))

/* return the complete mstatus with the SD bit */
static target_ulong get_mstatus(RISCVCPUState *s, target_ulong mask)
{
    target_ulong val;
    BOOL sd;
    val = s->mstatus | (s->fs << MSTATUS_FS_SHIFT) |
        ((target_ulong)2 << MSTATUS_UXL_SHIFT) |
        ((target_ulong)2 << MSTATUS_SXL_SHIFT);
    val &= mask;
    sd = ((val & MSTATUS_FS) == MSTATUS_FS) |
        ((val & MSTATUS_XS) == MSTATUS_XS);
    if (sd)
        val |= (target_ulong)1 << 63;
    return val;
}
                              
static void set_mstatus(RISCVCPUState *s, target_ulong val)
{
    target_ulong mod, mask;
    
    /* flush the TLBs if change of MMU config */
    mod = s->mstatus ^ val;
    if ((mod & (MSTATUS_MPRV | MSTATUS_SUM | MSTATUS_MXR)) != 0 ||
        ((s->mstatus & MSTATUS_MPRV) && (mod & MSTATUS_MPP) != 0)) {
        tlb_flush_all(s);
    }
    s->fs = (val >> MSTATUS_FS_SHIFT) & 3;

    mask = MSTATUS_MASK & ~MSTATUS_FS;
    s->mstatus = (s->mstatus & ~mask) | (val & mask);
}

static BOOL counter_access_enabled(RISCVCPUState *s, uint32_t counter_index)
{
    uint32_t mask;

    if (s->priv == PRV_M)
        return TRUE;
    mask = 1 << counter_index;
    if (!(s->mcounteren & mask))
        return FALSE;
    if (s->priv == PRV_U && !(s->scounteren & mask))
        return FALSE;
    return TRUE;
}

static target_ulong get_pmpcfg(RISCVCPUState *s, int first_entry)
{
    target_ulong val;
    int i;

    val = 0;
    for(i = 0; i < 8; i++)
        val |= (target_ulong)s->pmpcfg[first_entry + i] << (i * 8);
    return val;
}

static BOOL set_pmpcfg(RISCVCPUState *s, int first_entry,
                       target_ulong val)
{
    BOOL changed;
    int i;
    uint8_t cfg;

    changed = FALSE;
    for(i = 0; i < 8; i++) {
        if (s->pmpcfg[first_entry + i] & PMP_CFG_L)
            continue;
        cfg = (val >> (i * 8)) &
            (PMP_CFG_L | PMP_CFG_A_MASK |
             PMP_CFG_X | PMP_CFG_W | PMP_CFG_R);
        if ((cfg & (PMP_CFG_R | PMP_CFG_W)) == PMP_CFG_W)
            cfg &= ~PMP_CFG_W;
        if (s->pmpcfg[first_entry + i] == cfg)
            continue;
        s->pmpcfg[first_entry + i] = cfg;
        changed = TRUE;
    }
    return changed;
}

static BOOL set_pmpaddr(RISCVCPUState *s, int entry, target_ulong val)
{
    uint8_t next_cfg;

    if (s->pmpcfg[entry] & PMP_CFG_L)
        return FALSE;
    if (entry + 1 < PMP_ENTRY_COUNT) {
        next_cfg = s->pmpcfg[entry + 1];
        if ((next_cfg & (PMP_CFG_L | PMP_CFG_A_MASK)) ==
            (PMP_CFG_L | PMP_CFG_A_TOR))
            return FALSE;
    }
    val &= PMP_ADDR_MASK;
    if (s->pmpaddr[entry] == val)
        return FALSE;
    s->pmpaddr[entry] = val;
    return TRUE;
}

static void update_stimecmp_irq(RISCVCPUState *s)
{
    if (s->get_time)
        riscv_cpu_update_time(s, s->get_time(s->time_opaque));
}

/* return -1 if invalid CSR. 0 if OK. 'will_write' indicate that the
   csr will be written after (used for CSR access check) */
static int csr_read(RISCVCPUState *s, target_ulong *pval, uint32_t csr,
                     BOOL will_write)
{
    target_ulong val;

    if (((csr & 0xc00) == 0xc00) && will_write)
        return -1; /* read-only CSR */
    if (s->priv < ((csr >> 8) & 3))
        return -1; /* not enough priviledge */
    if (csr == 0x180 && s->priv == PRV_S && (s->mstatus & MSTATUS_TVM))
        return -1;
    if (csr == 0x14d && s->priv != PRV_M &&
        (!(s->menvcfg & MENVCFG_STCE) || !(s->mcounteren & (1 << 1))))
        return -1;
    
    switch(csr) {
#if FLEN > 0
    case 0x001: /* fflags */
        if (s->fs == 0)
            return -1;
        val = s->fflags;
        break;
    case 0x002: /* frm */
        if (s->fs == 0)
            return -1;
        val = s->frm;
        break;
    case 0x003:
        if (s->fs == 0)
            return -1;
        val = s->fflags | (s->frm << 5);
        break;
#endif
    case 0xc00: /* ucycle */
        if (!counter_access_enabled(s, csr & 0x1f))
            goto invalid_csr;
        val = (int64_t)s->cycle_counter;
        break;
    case 0xc01: /* time */
        if (!counter_access_enabled(s, csr & 0x1f) || !s->get_time)
            goto invalid_csr;
        val = s->get_time(s->time_opaque);
        break;
    case 0xc02: /* uinstret */
        if (!counter_access_enabled(s, csr & 0x1f))
            goto invalid_csr;
        val = (int64_t)s->minstret_counter;
        break;
    case 0x100:
        val = get_mstatus(s, SSTATUS_MASK);
        break;
    case 0x104: /* sie */
        val = s->mie & s->mideleg;
        break;
    case 0x105:
        val = s->stvec;
        break;
    case 0x106:
        val = s->scounteren;
        break;
    case 0x10a: /* senvcfg: no S-mode features implemented */
        val = 0;
        break;
    case 0x140:
        val = s->sscratch;
        break;
    case 0x141:
        val = s->sepc;
        break;
    case 0x142:
        val = s->scause;
        break;
    case 0x143:
        val = s->stval;
        break;
    case 0x144: /* sip */
        val = s->mip & s->mideleg;
        break;
    case 0x14d: /* stimecmp */
        val = s->stimecmp;
        break;
    case 0x180:
        val = s->satp;
        break;
    case 0x300:
        val = get_mstatus(s, (target_ulong)-1);
        break;
    case 0x301:
        val = s->misa;
        val |= (target_ulong)2 << 62;
        break;
    case 0x302:
        val = s->medeleg;
        break;
    case 0x303:
        val = s->mideleg;
        break;
    case 0x304:
        val = s->mie;
        break;
    case 0x305:
        val = s->mtvec;
        break;
    case 0x306:
        val = s->mcounteren;
        break;
    case 0x30a: /* menvcfg */
        val = s->menvcfg;
        break;
    case 0x320: /* mcountinhibit: no counters can be inhibited */
        val = 0;
        break;
    case 0x3a0: /* pmpcfg0 */
        val = get_pmpcfg(s, 0);
        break;
    case 0x3a2: /* pmpcfg2 */
        val = get_pmpcfg(s, 8);
        break;
    case 0x340:
        val = s->mscratch;
        break;
    case 0x341:
        val = s->mepc;
        break;
    case 0x342:
        val = s->mcause;
        break;
    case 0x343:
        val = s->mtval;
        break;
    case 0x344:
        val = s->mip;
        break;
    case 0xb00: /* mcycle */
        val = (int64_t)s->cycle_counter;
        break;
    case 0xb02: /* minstret */
        val = (int64_t)s->minstret_counter;
        break;
    case 0xf11: /* mvendorid: not implemented */
    case 0xf12: /* marchid: not implemented */
    case 0xf13: /* mimpid: not implemented */
    case 0xf15: /* mconfigptr: no configuration string */
        val = 0;
        break;
    case 0xf14:
        val = s->mhartid;
        break;
    default:
        if (csr >= 0x3b0 && csr < 0x3b0 + PMP_ENTRY_COUNT) {
            val = s->pmpaddr[csr - 0x3b0];
            break;
        }
    invalid_csr:
#ifdef DUMP_INVALID_CSR
        /* the 'time' counter is usually emulated */
        if (csr != 0xc01 && csr != 0xc81) {
            printf("csr_read: invalid CSR=0x%x\n", csr);
        }
#endif
        *pval = 0;
        return -1;
    }
    *pval = val;
    return 0;
}

#if FLEN > 0
static void set_frm(RISCVCPUState *s, unsigned int val)
{
    if (val >= 5)
        val = 0;
    s->frm = val;
}

/* return -1 if invalid roundind mode */
static int get_insn_rm(RISCVCPUState *s, unsigned int rm)
{
    if (rm == 7)
        return s->frm;
    if (rm >= 5)
        return -1;
    else
        return rm;
}
#endif

typedef enum {
    CSR_WRITE_ERROR = -1,
    CSR_WRITE_OK,
    CSR_WRITE_FLUSH_TLB,
    CSR_WRITE_MINSTRET,
    CSR_WRITE_INTERRUPT,
} CSRWriteResult;

static CSRWriteResult csr_write(RISCVCPUState *s, uint32_t csr,
                                target_ulong val)
{
    target_ulong mask, old;

    if (csr == 0x180 && s->priv == PRV_S && (s->mstatus & MSTATUS_TVM))
        return CSR_WRITE_ERROR;

#if defined(DUMP_CSR)
    printf("csr_write: csr=0x%03x val=0x", csr);
    print_target_ulong(val);
    printf("\n");
#endif
    switch(csr) {
#if FLEN > 0
    case 0x001: /* fflags */
        s->fflags = val & 0x1f;
        s->fs = 3;
        break;
    case 0x002: /* frm */
        set_frm(s, val & 7);
        s->fs = 3;
        break;
    case 0x003: /* fcsr */
        set_frm(s, (val >> 5) & 7);
        s->fflags = val & 0x1f;
        s->fs = 3;
        break;
#endif
    case 0x100: /* sstatus */
        old = s->mstatus;
        set_mstatus(s, (s->mstatus & ~SSTATUS_MASK) | (val & SSTATUS_MASK));
        if (!(old & MSTATUS_SIE) && (s->mstatus & MSTATUS_SIE) &&
            (s->mip & s->mie & s->mideleg))
            return CSR_WRITE_INTERRUPT;
        break;
    case 0x104: /* sie */
        mask = s->mideleg;
        s->mie = (s->mie & ~mask) | (val & mask);
        break;
    case 0x105:
        s->stvec = val & ~3;
        break;
    case 0x106:
        s->scounteren = val & COUNTEREN_MASK;
        break;
    case 0x10a: /* senvcfg: hardwired to zero */
        break;
    case 0x140:
        s->sscratch = val;
        break;
    case 0x141:
        s->sepc = val & ~1;
        break;
    case 0x142:
        s->scause = val;
        break;
    case 0x143:
        s->stval = val;
        break;
    case 0x144: /* sip */
        mask = s->mideleg;
        if (s->menvcfg & MENVCFG_STCE)
            mask &= ~MIP_STIP;
        s->mip = (s->mip & ~mask) | (val & mask);
        break;
    case 0x14d: /* stimecmp */
        if (s->priv != PRV_M &&
            (!(s->menvcfg & MENVCFG_STCE) ||
             !(s->mcounteren & (1 << 1))))
            return CSR_WRITE_ERROR;
        s->stimecmp = val;
        update_stimecmp_irq(s);
        return CSR_WRITE_INTERRUPT;
    case 0x180:
        /* no ASID implemented */
        {
            int mode, new_mode;
            mode = s->satp >> SATP_MODE_SHIFT;
            new_mode = (val >> SATP_MODE_SHIFT) & SATP_MODE_MASK;
            if (new_mode == SATP_MODE_BARE || new_mode == SATP_MODE_SV39)
                mode = new_mode;
            s->satp = (val & SATP_PPN_MASK) |
                ((uint64_t)mode << SATP_MODE_SHIFT);
        }
        tlb_flush_all(s);
        return CSR_WRITE_FLUSH_TLB;
        
    case 0x300:
        old = s->mstatus;
        set_mstatus(s, val);
        if (!(old & MSTATUS_MIE) && (s->mstatus & MSTATUS_MIE) &&
            (s->mip & s->mie & ~s->mideleg))
            return CSR_WRITE_INTERRUPT;
        break;
    case 0x301: /* misa */
        break;
    case 0x302:
        mask = (1 << (CAUSE_STORE_PAGE_FAULT + 1)) - 1;
        s->medeleg = (s->medeleg & ~mask) | (val & mask);
        break;
    case 0x303:
        mask = MIP_SSIP | MIP_STIP | MIP_SEIP;
        s->mideleg = (s->mideleg & ~mask) | (val & mask);
        break;
    case 0x304:
        mask = MIP_MSIP | MIP_MTIP | MIP_SSIP | MIP_STIP | MIP_SEIP;
        s->mie = (s->mie & ~mask) | (val & mask);
        break;
    case 0x305:
        s->mtvec = val & ~3;
        break;
    case 0x306:
        s->mcounteren = val & COUNTEREN_MASK;
        break;
    case 0x30a: /* menvcfg */
        old = s->menvcfg;
        s->menvcfg = val & MENVCFG_MASK;
        if (s->menvcfg == old)
            break;
        if (s->menvcfg & MENVCFG_STCE)
            update_stimecmp_irq(s);
        else
            s->mip &= ~MIP_STIP;
        if ((s->menvcfg ^ old) & MENVCFG_ADUE) {
            tlb_flush_all(s);
            return CSR_WRITE_FLUSH_TLB;
        }
        return CSR_WRITE_INTERRUPT;
    case 0x320: /* mcountinhibit: no counters can be inhibited */
        break;
    case 0x3a0: /* pmpcfg0 */
        if (!set_pmpcfg(s, 0, val))
            break;
        tlb_flush_all(s);
        return CSR_WRITE_FLUSH_TLB;
    case 0x3a2: /* pmpcfg2 */
        if (!set_pmpcfg(s, 8, val))
            break;
        tlb_flush_all(s);
        return CSR_WRITE_FLUSH_TLB;
    case 0xb00: /* mcycle */
        s->cycle_counter = val;
        break;
    case 0xb02: /* minstret */
        s->minstret_counter = val;
        return CSR_WRITE_MINSTRET;
    case 0x340:
        s->mscratch = val;
        break;
    case 0x341:
        s->mepc = val & ~1;
        break;
    case 0x342:
        s->mcause = val;
        break;
    case 0x343:
        s->mtval = val;
        break;
    case 0x344:
        mask = MIP_SSIP | MIP_STIP;
        if (s->menvcfg & MENVCFG_STCE)
            mask &= ~MIP_STIP;
        s->mip = (s->mip & ~mask) | (val & mask);
        break;
    default:
        if (csr >= 0x3b0 && csr < 0x3b0 + PMP_ENTRY_COUNT) {
            if (!set_pmpaddr(s, csr - 0x3b0, val))
                break;
            tlb_flush_all(s);
            return CSR_WRITE_FLUSH_TLB;
        }
#ifdef DUMP_INVALID_CSR
        printf("csr_write: invalid CSR=0x%x\n", csr);
#endif
        return CSR_WRITE_ERROR;
    }
    return CSR_WRITE_OK;
}

static void set_priv(RISCVCPUState *s, int priv)
{
    if (s->priv != priv) {
        tlb_flush_all(s);
        s->priv = priv;
    }
}

static void raise_exception2(RISCVCPUState *s, uint32_t cause,
                             target_ulong tval)
{
    BOOL deleg;
    target_ulong causel;
    
#if defined(DUMP_EXCEPTIONS) || defined(DUMP_MMU_EXCEPTIONS) || defined(DUMP_INTERRUPTS)
    {
        int flag;
        flag = 0;
#ifdef DUMP_MMU_EXCEPTIONS
        if (cause == CAUSE_FAULT_FETCH ||
            cause == CAUSE_FAULT_LOAD ||
            cause == CAUSE_FAULT_STORE ||
            cause == CAUSE_FETCH_PAGE_FAULT ||
            cause == CAUSE_LOAD_PAGE_FAULT ||
            cause == CAUSE_STORE_PAGE_FAULT)
            flag = 1;
#endif
#ifdef DUMP_INTERRUPTS
        flag |= (cause & CAUSE_INTERRUPT) != 0;
#endif
#ifdef DUMP_EXCEPTIONS
        flag = 1;
        flag = (cause & CAUSE_INTERRUPT) == 0;
        if (cause == CAUSE_SUPERVISOR_ECALL || cause == CAUSE_ILLEGAL_INSTRUCTION)
            flag = 0;
#endif
        if (flag) {
            log_printf("raise_exception: cause=0x%08x tval=0x", cause);
#ifdef CONFIG_LOGFILE
            fprint_target_ulong(log_file, tval);
#else
            print_target_ulong(tval);
#endif
            log_printf("\n");
            dump_regs(s);
        }
    }
#endif

    if (s->priv <= PRV_S) {
        /* delegate the exception to the supervisor priviledge */
        if (cause & CAUSE_INTERRUPT)
            deleg = (s->mideleg >> (cause & 63)) & 1;
        else
            deleg = (s->medeleg >> cause) & 1;
    } else {
        deleg = 0;
    }
    
    causel = cause & 0x7fffffff;
    if (cause & CAUSE_INTERRUPT)
    causel |= (target_ulong)1 << 63;
    
    if (deleg) {
        s->scause = causel;
        s->sepc = s->pc;
        s->stval = tval;
        s->mstatus = (s->mstatus & ~MSTATUS_SPIE) |
            (((s->mstatus >> s->priv) & 1) << MSTATUS_SPIE_SHIFT);
        s->mstatus = (s->mstatus & ~MSTATUS_SPP) |
            (s->priv << MSTATUS_SPP_SHIFT);
        s->mstatus &= ~MSTATUS_SIE;
        set_priv(s, PRV_S);
        s->pc = s->stvec;
    } else {
        s->mcause = causel;
        s->mepc = s->pc;
        s->mtval = tval;
        s->mstatus = (s->mstatus & ~MSTATUS_MPIE) |
            (((s->mstatus >> s->priv) & 1) << MSTATUS_MPIE_SHIFT);
        s->mstatus = (s->mstatus & ~MSTATUS_MPP) |
            (s->priv << MSTATUS_MPP_SHIFT);
        s->mstatus &= ~MSTATUS_MIE;
        set_priv(s, PRV_M);
        s->pc = s->mtvec;
    }
}

static void raise_exception(RISCVCPUState *s, uint32_t cause)
{
    raise_exception2(s, cause, 0);
}

static void handle_sret(RISCVCPUState *s)
{
    int spp, spie;
    spp = (s->mstatus >> MSTATUS_SPP_SHIFT) & 1;
    spie = (s->mstatus >> MSTATUS_SPIE_SHIFT) & 1;
    s->mstatus = (s->mstatus & ~MSTATUS_SIE) |
        (spie ? MSTATUS_SIE : 0);
    /* set SPIE to 1 */
    s->mstatus |= MSTATUS_SPIE;
    /* set SPP to U */
    s->mstatus &= ~MSTATUS_SPP;
    s->mstatus &= ~MSTATUS_MPRV;
    set_priv(s, spp);
    s->pc = s->sepc;
}

static void handle_mret(RISCVCPUState *s)
{
    int mpp, mpie;
    mpp = (s->mstatus >> MSTATUS_MPP_SHIFT) & 3;
    mpie = (s->mstatus >> MSTATUS_MPIE_SHIFT) & 1;
    s->mstatus = (s->mstatus & ~MSTATUS_MIE) |
        (mpie ? MSTATUS_MIE : 0);
    /* set MPIE to 1 */
    s->mstatus |= MSTATUS_MPIE;
    /* set MPP to U */
    s->mstatus &= ~MSTATUS_MPP;
    if (mpp < PRV_M)
        s->mstatus &= ~MSTATUS_MPRV;
    set_priv(s, mpp);
    s->pc = s->mepc;
}

static inline uint32_t get_pending_irq_mask(RISCVCPUState *s)
{
    uint32_t pending_ints, enabled_ints;

    pending_ints = s->mip & s->mie;
    if (pending_ints == 0)
        return 0;

    enabled_ints = 0;
    switch(s->priv) {
    case PRV_M:
        if (s->mstatus & MSTATUS_MIE)
            enabled_ints = ~s->mideleg;
        break;
    case PRV_S:
        enabled_ints = ~s->mideleg;
        if (s->mstatus & MSTATUS_SIE)
            enabled_ints |= s->mideleg;
        break;
    default:
    case PRV_U:
        enabled_ints = -1;
        break;
    }
    return pending_ints & enabled_ints;
}

static __exception int raise_interrupt(RISCVCPUState *s)
{
    uint32_t mask;
    int irq_num;

    mask = get_pending_irq_mask(s);
    if (mask == 0)
        return 0;
    irq_num = ctz32(mask);
    raise_exception(s, irq_num | CAUSE_INTERRUPT);
    return -1;
}

static inline int32_t sext(int32_t val, int n)
{
    uint32_t sign_mask = (uint32_t)1 << (n - 1);
    uint32_t uval = val;

    return (int32_t)((int64_t)(uval & (sign_mask - 1)) -
                     (int64_t)(uval & sign_mask));
}

static inline uint32_t get_field1(uint32_t val, int src_pos, 
                                  int dst_pos, int dst_pos_max)
{
    int mask;
    assert(dst_pos_max >= dst_pos);
    mask = ((1 << (dst_pos_max - dst_pos + 1)) - 1) << dst_pos;
    if (dst_pos >= src_pos)
        return (val << (dst_pos - src_pos)) & mask;
    else
        return (val >> (src_pos - dst_pos)) & mask;
}

#define XLEN 64
#include "riscv_cpu_template.h"

static void glue(riscv_cpu_interp, MAX_XLEN)(RISCVCPUState *s, int n_cycles)
{
#ifdef USE_GLOBAL_STATE
    s = &riscv_cpu_global_state;
#endif
    uint64_t timeout;

    timeout = s->elapsed_cycles + n_cycles;
    while (!s->power_down_flag &&
           (int)(timeout - s->elapsed_cycles) > 0) {
        n_cycles = timeout - s->elapsed_cycles;
        riscv_cpu_interp_x64(s, n_cycles);
    }
}

/* Note: the value is not accurate when called in riscv_cpu_interp() */
static uint64_t glue(riscv_cpu_get_cycles, MAX_XLEN)(RISCVCPUState *s)
{
    return s->elapsed_cycles;
}

static void glue(riscv_cpu_set_mip, MAX_XLEN)(RISCVCPUState *s, uint32_t mask)
{
    s->mip |= mask;
    /* exit from power down if an interrupt is pending */
    if (s->power_down_flag && (s->mip & s->mie) != 0)
        s->power_down_flag = FALSE;
}

static void glue(riscv_cpu_reset_mip, MAX_XLEN)(RISCVCPUState *s, uint32_t mask)
{
    s->mip &= ~mask;
}

static uint32_t glue(riscv_cpu_get_mip, MAX_XLEN)(RISCVCPUState *s)
{
    return s->mip;
}

static BOOL glue(riscv_cpu_get_power_down, MAX_XLEN)(RISCVCPUState *s)
{
    return s->power_down_flag;
}

static RISCVCPUState *glue(riscv_cpu_init, MAX_XLEN)(PhysMemoryMap *mem_map)
{
    RISCVCPUState *s;
    
#ifdef USE_GLOBAL_STATE
    s = &riscv_cpu_global_state;
#else
    s = mallocz(sizeof(*s));
#endif
    s->common.class_ptr = &glue(riscv_cpu_class, MAX_XLEN);
    s->mem_map = mem_map;
    s->pc = 0x1000;
    s->priv = PRV_M;
    s->mstatus = 0;
    s->stimecmp = UINT64_MAX;
    s->misa |= MCPUID_SUPER | MCPUID_USER | MCPUID_I | MCPUID_M | MCPUID_A;
#if FLEN >= 32
    s->misa |= MCPUID_F;
#endif
#if FLEN >= 64
    s->misa |= MCPUID_D;
#endif
#ifdef CONFIG_EXT_C
    s->misa |= MCPUID_C;
#endif
    tlb_init(s);
    return s;
}

static void glue(riscv_cpu_end, MAX_XLEN)(RISCVCPUState *s)
{
#ifdef USE_GLOBAL_STATE
    free(s);
#else
    (void)s;
#endif
}

static uint32_t glue(riscv_cpu_get_misa, MAX_XLEN)(RISCVCPUState *s)
{
    return s->misa;
}

const RISCVCPUClass glue(riscv_cpu_class, MAX_XLEN) = {
    glue(riscv_cpu_init, MAX_XLEN),
    glue(riscv_cpu_end, MAX_XLEN),
    glue(riscv_cpu_interp, MAX_XLEN),
    glue(riscv_cpu_get_cycles, MAX_XLEN),
    glue(riscv_cpu_set_mip, MAX_XLEN),
    glue(riscv_cpu_reset_mip, MAX_XLEN),
    glue(riscv_cpu_get_mip, MAX_XLEN),
    glue(riscv_cpu_get_power_down, MAX_XLEN),
    glue(riscv_cpu_get_misa, MAX_XLEN),
    glue(riscv_cpu_flush_tlb_write_range_ram, MAX_XLEN),
};

RISCVCPUState *riscv_cpu_init(PhysMemoryMap *mem_map)
{
    return riscv_cpu_class64.riscv_cpu_init(mem_map);
}

void riscv_cpu_set_time_source(RISCVCPUState *s,
                               RISCVCPUTimeFunc *get_time,
                               void *opaque)
{
    s->get_time = get_time;
    s->time_opaque = opaque;
}

void riscv_cpu_update_time(RISCVCPUState *s, uint64_t time)
{
    if (!(s->menvcfg & MENVCFG_STCE))
        return;
    if (time >= s->stimecmp) {
        s->mip |= MIP_STIP;
        if (s->power_down_flag && (s->mip & s->mie) != 0)
            s->power_down_flag = FALSE;
    } else {
        s->mip &= ~MIP_STIP;
    }
}

uint64_t riscv_cpu_get_stimecmp(RISCVCPUState *s)
{
    if (!(s->menvcfg & MENVCFG_STCE))
        return UINT64_MAX;
    return s->stimecmp;
}
