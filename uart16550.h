/* 16550A UART emulation */

#ifndef UART16550_H
#define UART16550_H

#include <stdint.h>

#include "cutils.h"
#include "iomem.h"

typedef struct UART16550State UART16550State;

typedef void UART16550TxFunc(void *opaque, const uint8_t *buf, int len);

struct UART16550State {
    uint64_t base_addr;
    IRQSignal *irq;
    UART16550TxFunc *tx_func;
    void *tx_opaque;
    uint8_t dll, dlm; /* divisor latch, visible when LCR_DLAB is set */
    uint8_t ier, fcr, lcr, mcr, scr;
    uint8_t rx_fifo[16]; /* 16550A receive FIFO depth */
    int rx_head, rx_count;
    BOOL rx_overrun; /* latched LSR_OE until LSR is read */
};

UART16550State *uart16550_init(PhysMemoryMap *map, uint64_t base_addr,
                               IRQSignal *irq, UART16550TxFunc *tx_func,
                               void *tx_opaque);

/* Free bytes in the receive FIFO. Host input must not exceed this;
   bytes beyond it wait in the host pipe, as with hardware flow. */
int uart16550_receive_space(UART16550State *s);

/* Feed host input bytes into the receive FIFO. Returns the number of
   bytes stored; the caller routes the remainder elsewhere. */
int uart16550_receive(UART16550State *s, const uint8_t *buf, int len);

#endif /* UART16550_H */
