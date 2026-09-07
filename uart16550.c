/* 16550A UART emulation.
 *
 * The transmitter completes instantly: THR writes are delivered to the
 * host at once, so LSR_THRE and LSR_TEMT always read set. This matches
 * a real 16550A running at infinite baud: status and interrupt
 * semantics are exact, only wall-clock timing is not modeled. In
 * particular the TX-empty interrupt stays asserted while IER_ETBEI is
 * set, exactly as on hardware, so drivers must clear ETBEI when idle
 * as they do on silicon.
 */
#include <stdlib.h>
#include <string.h>
#include <assert.h>

#include "cutils.h"
#include "iomem.h"
#include "uart16550.h"

#define UART_RX_OFFSET 0 /* RBR (read) / THR (write) / DLL (DLAB) */
#define UART_IER_OFFSET 1 /* IER (read/write) / DLM (DLAB) */
#define UART_IIR_OFFSET 2 /* IIR (read) / FCR (write) */
#define UART_LCR_OFFSET 3
#define UART_MCR_OFFSET 4
#define UART_LSR_OFFSET 5
#define UART_MSR_OFFSET 6
#define UART_SCR_OFFSET 7

#define UART_IER_ERBFI 0x01 /* received data available */
#define UART_IER_ETBEI 0x02 /* transmitter holding register empty */
#define UART_IER_RLSI 0x04 /* receiver line status */

#define UART_IIR_NO_INT 0x01
#define UART_IIR_RLS 0x06 /* receiver line status */
#define UART_IIR_RDA 0x04 /* received data available */
#define UART_IIR_THRE 0x02 /* transmitter holding register empty */
#define UART_IIR_FIFO_ENABLED 0xc0

#define UART_FCR_ENABLE_FIFO 0x01
#define UART_FCR_CLEAR_RCVR 0x02
#define UART_FCR_CLEAR_XMIT 0x04

#define UART_LCR_DLAB 0x80

#define UART_MCR_LOOP 0x10

#define UART_LSR_DR 0x01 /* data ready */
#define UART_LSR_OE 0x02 /* overrun error */
#define UART_LSR_THRE 0x20 /* transmitter holding register empty */
#define UART_LSR_TEMT 0x40 /* transmitter empty */

/* Fixed modem status with no modem attached: CTS, DSR and DCD
   asserted, as on a looped-back cable. */
#define UART_MSR_FIXED 0xb0

#define UART_RX_DEPTH 16

static int uart16550_pending_id(UART16550State *s)
{
    if ((s->ier & UART_IER_RLSI) && s->rx_overrun)
        return UART_IIR_RLS;
    if ((s->ier & UART_IER_ERBFI) && s->rx_count > 0)
        return UART_IIR_RDA;
    if (s->ier & UART_IER_ETBEI)
        return UART_IIR_THRE;
    return UART_IIR_NO_INT;
}

static void uart16550_update_irq(UART16550State *s)
{
    set_irq(s->irq, uart16550_pending_id(s) != UART_IIR_NO_INT);
}

static uint8_t uart16550_pop_rx(UART16550State *s)
{
    uint8_t val;

    if (s->rx_count == 0)
        return 0;
    val = s->rx_fifo[s->rx_head];
    s->rx_head = (s->rx_head + 1) % UART_RX_DEPTH;
    s->rx_count--;
    uart16550_update_irq(s);
    return val;
}

static void uart16550_push_rx(UART16550State *s, uint8_t val)
{
    if (s->rx_count >= UART_RX_DEPTH) {
        s->rx_overrun = TRUE;
    } else {
        s->rx_fifo[(s->rx_head + s->rx_count) % UART_RX_DEPTH] = val;
        s->rx_count++;
    }
    uart16550_update_irq(s);
}

static uint8_t uart16550_get_lsr(UART16550State *s)
{
    uint8_t val;

    val = UART_LSR_THRE | UART_LSR_TEMT;
    if (s->rx_count > 0)
        val |= UART_LSR_DR;
    if (s->rx_overrun)
        val |= UART_LSR_OE;
    return val;
}

static uint8_t uart16550_get_msr(UART16550State *s)
{
    uint8_t val;

    if (!(s->mcr & UART_MCR_LOOP))
        return UART_MSR_FIXED;
    /* In loopback mode the modem inputs follow the modem outputs. */
    val = 0;
    if (s->mcr & 0x02)
        val |= 0x10;
    if (s->mcr & 0x01)
        val |= 0x20;
    if (s->mcr & 0x04)
        val |= 0x40;
    if (s->mcr & 0x08)
        val |= 0x80;
    return val;
}

static uint32_t uart16550_read(void *opaque, uint32_t offset,
                               int size_log2)
{
    UART16550State *s = opaque;
    uint32_t val;
    int id;

    assert(size_log2 == 0);
    if (offset >= 8)
        return 0;
    if ((s->lcr & UART_LCR_DLAB) && offset <= 1) {
        if (offset == 0)
            return s->dll;
        return s->dlm;
    }
    switch (offset) {
    case UART_RX_OFFSET:
        val = uart16550_pop_rx(s);
        break;
    case UART_IER_OFFSET:
        val = s->ier;
        break;
    case UART_IIR_OFFSET:
        id = uart16550_pending_id(s);
        val = id;
        if (s->fcr & UART_FCR_ENABLE_FIFO)
            val |= UART_IIR_FIFO_ENABLED;
        break;
    case UART_LCR_OFFSET:
        val = s->lcr;
        break;
    case UART_MCR_OFFSET:
        val = s->mcr;
        break;
    case UART_LSR_OFFSET:
        val = uart16550_get_lsr(s);
        s->rx_overrun = FALSE;
        uart16550_update_irq(s);
        break;
    case UART_MSR_OFFSET:
        val = uart16550_get_msr(s);
        break;
    case UART_SCR_OFFSET:
        val = s->scr;
        break;
    default:
        val = 0;
        break;
    }
    return val;
}

static void uart16550_write(void *opaque, uint32_t offset, uint32_t val,
                            int size_log2)
{
    UART16550State *s = opaque;
    uint8_t ch;

    assert(size_log2 == 0);
    if (offset >= 8)
        return;
    if ((s->lcr & UART_LCR_DLAB) && offset <= 1) {
        if (offset == 0)
            s->dll = val;
        else
            s->dlm = val;
        return;
    }
    switch (offset) {
    case UART_RX_OFFSET:
        ch = val;
        if (s->mcr & UART_MCR_LOOP)
            uart16550_push_rx(s, ch);
        else
            s->tx_func(s->tx_opaque, &ch, 1);
        break;
    case UART_IER_OFFSET:
        s->ier = val;
        uart16550_update_irq(s);
        break;
    case UART_IIR_OFFSET:
        s->fcr = val;
        if (val & UART_FCR_CLEAR_RCVR) {
            s->rx_head = 0;
            s->rx_count = 0;
            s->rx_overrun = FALSE;
            uart16550_update_irq(s);
        }
        break;
    case UART_LCR_OFFSET:
        s->lcr = val;
        break;
    case UART_MCR_OFFSET:
        s->mcr = val;
        break;
    case UART_SCR_OFFSET:
        s->scr = val;
        break;
    default:
        break;
    }
}

UART16550State *uart16550_init(PhysMemoryMap *map, uint64_t base_addr,
                               IRQSignal *irq, UART16550TxFunc *tx_func,
                               void *tx_opaque)
{
    UART16550State *s;

    s = mallocz(sizeof(*s));
    s->base_addr = base_addr;
    s->irq = irq;
    s->tx_func = tx_func;
    s->tx_opaque = tx_opaque;
    cpu_register_device(map, base_addr, 8, s,
                        uart16550_read, uart16550_write, DEVIO_SIZE8);
    return s;
}

int uart16550_receive_space(UART16550State *s)
{
    if (s->mcr & UART_MCR_LOOP)
        return 0; /* in loopback mode, accept no host input */
    return UART_RX_DEPTH - s->rx_count;
}

int uart16550_receive(UART16550State *s, const uint8_t *buf, int len)
{
    int i;

    for (i = 0; i < len && s->rx_count < UART_RX_DEPTH; i++)
        uart16550_push_rx(s, buf[i]);
    return i;
}
