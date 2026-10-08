#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* The fixed recurrence has a tiny working set and no host services in its loop.
 * Unsigned arithmetic defines wrapping behavior on every supported compiler. */
static uint64_t run(uint64_t count) {
    uint64_t values[16];
    for (unsigned i = 0; i < 16; ++i) values[i] = i + 1;
    uint64_t state = UINT64_C(0x123456789abcdef0);
    for (uint64_t i = 0; i < count; ++i) {
        unsigned slot = (unsigned)(state & 15);
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state += values[slot];
        if (state & 1) state *= 33;
        else state += UINT64_C(0x9e3779b97f4a7c15);
        values[slot] = state;
    }
    for (unsigned i = 0; i < 16; ++i) state ^= values[i];
    return state;
}

/* The printed checksum lets the host check repeated runs and compare builds
 * without adding verification work to the inner loop. */
int main(int argc, char **argv) {
    if (argc != 2) { fputs("usage: loop POSITIVE_COUNT\n", stderr); return 2; }
    char *end;
    errno = 0;
    uint64_t count = strtoull(argv[1], &end, 10);
    if (errno || !*argv[1] || *end || argv[1][0] == '-' || count == 0) {
        fputs("invalid loop count\n", stderr); return 2;
    }
    printf("loop-checksum=%016" PRIx64 "\n", run(count));
    return 0;
}
