/* Goldfish real-time clock emulation */

#ifndef GOLDFISH_RTC_H
#define GOLDFISH_RTC_H

#include <stdint.h>

#include "iomem.h"

typedef struct GoldfishRTCState GoldfishRTCState;

GoldfishRTCState *goldfish_rtc_init(PhysMemoryMap *map, uint64_t base_addr,
                                    uint64_t region_size, IRQSignal *irq);
void goldfish_rtc_end(GoldfishRTCState *s);
int goldfish_rtc_get_sleep_duration(GoldfishRTCState *s, int delay_ms);

#endif /* GOLDFISH_RTC_H */
