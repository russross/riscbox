#ifndef TINYEMU_STDLIB_H
#define TINYEMU_STDLIB_H

#include <stddef.h>

void *malloc(size_t size);
void free(void *ptr);
_Noreturn void abort(void);
_Noreturn void exit(int status);

#endif
