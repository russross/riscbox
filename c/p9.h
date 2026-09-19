/*
 * Raw 9P server interface
 *
 * Copyright (c) 2026 Russ Ross
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
#ifndef P9_H
#define P9_H

#include <stddef.h>
#include <stdint.h>

typedef struct P9Server P9Server;

/* Process one complete 9P message and return the complete reply size. */
typedef int P9RequestFunc(P9Server *server,
                         const uint8_t *request, size_t request_size,
                         uint8_t *reply, size_t reply_capacity);

struct P9Server {
    /* Requests are synchronous and serialized by the VirtIO device. */
    P9RequestFunc *request;
    void (*end)(P9Server *server);
};

P9Server *p9_socket_init(const char *socket_path);
P9Server *p9_js_init(void);
void p9_server_end(P9Server *server);

#endif /* P9_H */
