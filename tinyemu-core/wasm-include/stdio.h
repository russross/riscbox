#ifndef TINYEMU_STDIO_H
#define TINYEMU_STDIO_H

#include <stdarg.h>

typedef struct TinyemuFile FILE;
extern FILE *stderr;
extern FILE *stdout;
int printf(const char *format, ...);
int fprintf(FILE *file, const char *format, ...);
int vprintf(const char *format, va_list args);
int vfprintf(FILE *file, const char *format, va_list args);
FILE *fopen(const char *path, const char *mode);

#endif
