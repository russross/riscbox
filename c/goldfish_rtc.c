/* Goldfish real-time clock emulation */

#include <assert.h>
#include <stdlib.h>
#include <time.h>

#include "cutils.h"
#include "goldfish_rtc.h"

#define RTC_TIME_LOW 0x00
#define RTC_TIME_HIGH 0x04
#define RTC_ALARM_LOW 0x08
#define RTC_ALARM_HIGH 0x0c
#define RTC_IRQ_ENABLED 0x10
#define RTC_CLEAR_ALARM 0x14
#define RTC_ALARM_STATUS 0x18
#define RTC_CLEAR_INTERRUPT 0x1c

#define NANOSECONDS_PER_SECOND 1000000000ULL
#define NANOSECONDS_PER_MILLISECOND 1000000ULL

struct GoldfishRTCState {
    IRQSignal *irq;
    uint64_t time_offset;
    uint32_t time_high;
    uint64_t alarm_next;
    BOOL alarm_running;
    BOOL irq_pending;
    BOOL irq_enabled;
};

static uint64_t goldfish_rtc_host_time(void)
{
    struct timespec ts;

    clock_gettime(CLOCK_REALTIME, &ts);
    return (uint64_t)ts.tv_sec * NANOSECONDS_PER_SECOND + ts.tv_nsec;
}

static uint64_t goldfish_rtc_get_count(GoldfishRTCState *s)
{
    return goldfish_rtc_host_time() + s->time_offset;
}

static void goldfish_rtc_update_irq(GoldfishRTCState *s)
{
    set_irq(s->irq, s->irq_pending && s->irq_enabled);
}

static void goldfish_rtc_check_alarm(GoldfishRTCState *s, uint64_t now)
{
    if (s->alarm_running && s->alarm_next <= now) {
        s->alarm_running = FALSE;
        s->irq_pending = TRUE;
        goldfish_rtc_update_irq(s);
    }
}

static uint32_t goldfish_rtc_read(void *opaque, uint32_t offset,
                                  int size_log2)
{
    GoldfishRTCState *s = opaque;
    uint64_t value;

    assert(size_log2 == 2);
    switch (offset) {
    case RTC_TIME_LOW:
        value = goldfish_rtc_get_count(s);
        s->time_high = value >> 32;
        return value;
    case RTC_TIME_HIGH:
        return s->time_high;
    case RTC_ALARM_LOW:
        return s->alarm_next;
    case RTC_ALARM_HIGH:
        return s->alarm_next >> 32;
    case RTC_IRQ_ENABLED:
        return s->irq_enabled;
    case RTC_ALARM_STATUS:
        goldfish_rtc_check_alarm(s, goldfish_rtc_get_count(s));
        return s->alarm_running;
    default:
        return 0;
    }
}

static void goldfish_rtc_write(void *opaque, uint32_t offset, uint32_t value,
                               int size_log2)
{
    GoldfishRTCState *s = opaque;
    uint64_t current_time, new_time;

    assert(size_log2 == 2);
    switch (offset) {
    case RTC_TIME_LOW:
        current_time = goldfish_rtc_get_count(s);
        new_time = (current_time & ~UINT64_C(0xffffffff)) | value;
        s->time_offset += new_time - current_time;
        break;
    case RTC_TIME_HIGH:
        current_time = goldfish_rtc_get_count(s);
        new_time = (current_time & UINT64_C(0xffffffff)) |
                   ((uint64_t)value << 32);
        s->time_offset += new_time - current_time;
        break;
    case RTC_ALARM_LOW:
        s->alarm_next = (s->alarm_next & ~UINT64_C(0xffffffff)) | value;
        s->alarm_running = TRUE;
        goldfish_rtc_check_alarm(s, goldfish_rtc_get_count(s));
        break;
    case RTC_ALARM_HIGH:
        s->alarm_next = (s->alarm_next & UINT64_C(0xffffffff)) |
                        ((uint64_t)value << 32);
        break;
    case RTC_IRQ_ENABLED:
        s->irq_enabled = value & 1;
        goldfish_rtc_update_irq(s);
        break;
    case RTC_CLEAR_ALARM:
        s->alarm_running = FALSE;
        break;
    case RTC_CLEAR_INTERRUPT:
        s->irq_pending = FALSE;
        goldfish_rtc_update_irq(s);
        break;
    default:
        break;
    }
}

GoldfishRTCState *goldfish_rtc_init(PhysMemoryMap *map, uint64_t base_addr,
                                    uint64_t region_size, IRQSignal *irq)
{
    GoldfishRTCState *s;

    s = mallocz(sizeof(*s));
    s->irq = irq;
    cpu_register_device(map, base_addr, region_size, s,
                        goldfish_rtc_read, goldfish_rtc_write, DEVIO_SIZE32);
    return s;
}

void goldfish_rtc_end(GoldfishRTCState *s)
{
    free(s);
}

int goldfish_rtc_get_sleep_duration(GoldfishRTCState *s, int delay_ms)
{
    uint64_t now, alarm_delay_ms;

    now = goldfish_rtc_get_count(s);
    goldfish_rtc_check_alarm(s, now);
    if (!s->alarm_running)
        return delay_ms;
    alarm_delay_ms = (s->alarm_next - now) / NANOSECONDS_PER_MILLISECOND;
    if (alarm_delay_ms < (uint64_t)delay_ms)
        return alarm_delay_ms;
    return delay_ms;
}
