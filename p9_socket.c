/*
 * 9P server connection over a Unix-domain socket
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
#include <errno.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

#include "cutils.h"
#include "p9.h"

typedef struct {
    P9Server common;
    int fd;
} P9SocketServer;

static int p9_socket_fail(P9SocketServer *s, int error)
{
    if (s->fd >= 0) {
        close(s->fd);
        s->fd = -1;
    }
    return error;
}

static int write_all(int fd, const uint8_t *buf, size_t len)
{
    while (len > 0) {
        ssize_t ret;

        ret = send(fd, buf, len, MSG_NOSIGNAL);
        if (ret < 0) {
            if (errno == EINTR)
                continue;
            return -errno;
        }
        if (ret == 0)
            return -EIO;
        buf += ret;
        len -= ret;
    }
    return 0;
}

static int read_all(int fd, uint8_t *buf, size_t len)
{
    while (len > 0) {
        ssize_t ret;

        ret = recv(fd, buf, len, 0);
        if (ret < 0) {
            if (errno == EINTR)
                continue;
            return -errno;
        }
        if (ret == 0)
            return -EIO;
        buf += ret;
        len -= ret;
    }
    return 0;
}

static int p9_socket_request(P9Server *server,
                             const uint8_t *request, size_t request_size,
                             uint8_t *reply, size_t reply_capacity)
{
    P9SocketServer *s = (P9SocketServer *)server;
    uint32_t reply_size;
    int ret;

    if (s->fd < 0)
        return -EIO;
    ret = write_all(s->fd, request, request_size);
    if (ret < 0)
        return p9_socket_fail(s, ret);
    if (reply_capacity < sizeof(reply_size))
        return -ENOBUFS;
    ret = read_all(s->fd, reply, sizeof(reply_size));
    if (ret < 0)
        return p9_socket_fail(s, ret);
    reply_size = get_le32(reply);
    if (reply_size < 7 || reply_size > reply_capacity)
        return p9_socket_fail(s, -EPROTO);
    ret = read_all(s->fd, reply + sizeof(reply_size),
                   reply_size - sizeof(reply_size));
    if (ret < 0)
        return p9_socket_fail(s, ret);
    return reply_size;
}

static void p9_socket_end(P9Server *server)
{
    P9SocketServer *s = (P9SocketServer *)server;

    if (s->fd >= 0)
        close(s->fd);
}

P9Server *p9_socket_init(const char *socket_path)
{
    P9SocketServer *s;
    struct sockaddr_un addr;
    size_t path_len;
    int fd;

    path_len = strlen(socket_path);
    if (path_len == 0 || path_len >= sizeof(addr.sun_path)) {
        errno = ENAMETOOLONG;
        return NULL;
    }
    fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0)
        return NULL;

    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    memcpy(addr.sun_path, socket_path, path_len + 1);
    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        int saved_errno = errno;

        close(fd);
        errno = saved_errno;
        return NULL;
    }

    s = mallocz(sizeof(*s));
    s->common.request = p9_socket_request;
    s->common.end = p9_socket_end;
    s->fd = fd;
    return &s->common;
}

void p9_server_end(P9Server *server)
{
    server->end(server);
    free(server);
}
