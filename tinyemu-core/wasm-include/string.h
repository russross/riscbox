#ifndef TINYEMU_STRING_H
#define TINYEMU_STRING_H

#include <stddef.h>

static inline __attribute__((always_inline)) void *memset(void *dst, int byte, size_t size)
{
    return __builtin_memset(dst, byte, size);
}

#endif
