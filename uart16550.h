/* 16550A UART emulation */

#ifndef UART16550_H
#define UART16550_H

#include <stdint.h>

#include "iomem.h"

typedef struct UART16550State UART16550State;

typedef void UART16550TxFunc(void *opaque, const uint8_t *buf, int len);

UART16550State *uart16550_init(PhysMemoryMap *map, uint64_t base_addr,
                               uint64_t region_size, IRQSignal *irq,
                               UART16550TxFunc *tx_func, void *tx_opaque);
void uart16550_end(UART16550State *s);

int uart16550_receive_space(UART16550State *s);
int uart16550_receive(UART16550State *s, const uint8_t *buf, int len);

#endif /* UART16550_H */
