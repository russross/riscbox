/*
 * Simple PCI bus driver
 * 
 * Copyright (c) 2017 Fabrice Bellard
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
#include <string.h>
#include <inttypes.h>
#include <assert.h>
#include <stdarg.h>

#include "cutils.h"
#include "pci.h"

//#define DEBUG_CONFIG

typedef struct {
    uint32_t size; /* 0 means no mapping defined */
    uint8_t type;
    uint8_t enabled; /* true if mapping is enabled */
    void *opaque;
    PCIBarSetFunc *bar_set;
} PCIIORegion;

struct PCIDevice {
    PCIBus *bus;
    uint8_t devfn;
    IRQSignal irq[4];
    uint8_t config[256];
    uint8_t next_cap_offset; /* offset of the next capability */
    char *name; /* for debug only */
    PCIIORegion io_regions[PCI_NUM_REGIONS];
};

struct PCIBus {
    int bus_num;
    PCIDevice *device[256];
    PhysMemoryMap *mem_map;
    PhysMemoryMap *port_map;
    uint32_t irq_state[4][8]; /* one bit per device */
    IRQSignal irq[4];
};

static int bus_map_irq(PCIDevice *d, int irq_num)
{
    int slot_addend;
    slot_addend = (d->devfn >> 3) - 1;
    return (irq_num + slot_addend) & 3;
}

static void pci_device_set_irq(void *opaque, int irq_num, int level)
{
    PCIDevice *d = opaque;
    PCIBus *b = d->bus;
    uint32_t mask;
    int i, irq_level;
    
    //    printf("%s: pci_device_seq_irq: %d %d\n", d->name, irq_num, level);
    irq_num = bus_map_irq(d, irq_num);
    mask = 1 << (d->devfn & 0x1f);
    if (level)
        b->irq_state[irq_num][d->devfn >> 5] |= mask;
    else
        b->irq_state[irq_num][d->devfn >> 5] &= ~mask;

    /* compute the IRQ state */
    mask = 0;
    for(i = 0; i < 8; i++)
        mask |= b->irq_state[irq_num][i];
    irq_level = (mask != 0);
    set_irq(&b->irq[irq_num], irq_level);
}

static int devfn_alloc(PCIBus *b)
{
    int devfn;
    for(devfn = 0; devfn < 256; devfn += 8) {
        if (!b->device[devfn])
            return devfn;
    }
    return -1;
}

/* devfn < 0 means to allocate it */
PCIDevice *pci_register_device(PCIBus *b, const char *name, int devfn,
                               uint16_t vendor_id, uint16_t device_id,
                               uint8_t revision, uint16_t class_id)
{
    PCIDevice *d;
    int i;
    
    if (devfn < 0) {
        devfn = devfn_alloc(b);
        if (devfn < 0)
            return NULL;
    }
    if (b->device[devfn])
        return NULL;

    d = mallocz(sizeof(PCIDevice));
    d->bus = b;
    d->name = strdup(name);
    d->devfn = devfn;

    put_le16(d->config + 0x00, vendor_id);
    put_le16(d->config + 0x02, device_id);
    d->config[0x08] = revision;
    put_le16(d->config + 0x0a, class_id);
    d->config[0x0e] = 0x00; /* header type */
    d->next_cap_offset = 0x40;
    
    for(i = 0; i < 4; i++)
        irq_init(&d->irq[i], pci_device_set_irq, d, i);
    b->device[devfn] = d;

    return d;
}

IRQSignal *pci_device_get_irq(PCIDevice *d, unsigned int irq_num)
{
    assert(irq_num < 4);
    return &d->irq[irq_num];
}

PhysMemoryMap *pci_device_get_mem_map(PCIDevice *d)
{
    return d->bus->mem_map;
}

PhysMemoryMap *pci_device_get_port_map(PCIDevice *d)
{
    return d->bus->port_map;
}

void pci_register_bar(PCIDevice *d, unsigned int bar_num,
                      uint32_t size, int type,
                      void *opaque, PCIBarSetFunc *bar_set)
{
    PCIIORegion *r;
    uint32_t val, config_addr;
    
    assert(bar_num < PCI_NUM_REGIONS);
    assert((size & (size - 1)) == 0); /* power of two */
    assert(size >= 4);
    r = &d->io_regions[bar_num];
    assert(r->size == 0);
    r->size = size;
    r->type = type;
    r->enabled = FALSE;
    r->opaque = opaque;
    r->bar_set = bar_set;
    /* set the config value */
    val = 0;
    if (bar_num == PCI_ROM_SLOT) {
        config_addr = 0x30;
    } else {
        val |= r->type;
        config_addr = 0x10 + 4 * bar_num;
    }
    put_le32(&d->config[config_addr], val);
}

/* warning: only valid for one DEVIO page. Return NULL if no memory at
   the given address */
uint8_t *pci_device_get_dma_ptr(PCIDevice *d, uint64_t addr, BOOL is_rw)
{
    return phys_mem_get_ram_ptr(d->bus->mem_map, addr, is_rw);
}

void pci_device_set_config8(PCIDevice *d, uint8_t addr, uint8_t val)
{
    d->config[addr] = val;
}

void pci_device_set_config16(PCIDevice *d, uint8_t addr, uint16_t val)
{
    put_le16(&d->config[addr], val);
}

int pci_device_get_devfn(PCIDevice *d)
{
    return d->devfn;
}

/* return the offset of the capability or < 0 if error. */
int pci_add_capability(PCIDevice *d, const uint8_t *buf, int size)
{
    int offset;
    
    offset = d->next_cap_offset;
    if ((offset + size) > 256)
        return -1;
    d->next_cap_offset += size;
    d->config[PCI_STATUS] |= PCI_STATUS_CAP_LIST;
    memcpy(d->config + offset, buf, size);
    d->config[offset + 1] = d->config[PCI_CAPABILITY_LIST];
    d->config[PCI_CAPABILITY_LIST] = offset;
    return offset;
}
