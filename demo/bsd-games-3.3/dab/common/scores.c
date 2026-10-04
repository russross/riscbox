// Copyright (c) 2016 by Mike Sharov <msharov@users.sourceforge.net>
// This file is free software, distributed under the BSD license.

#include "../config.h"
#include <sys/file.h>
#include <sys/uio.h>
#include <sys/stat.h>

struct ScorefileHeader {
    char	magic [6];
    uint16_t	sum;
};

bool read_score_file (const char* filename, const char* magic, void* scores, size_t scoresSize)
{
    int fd = open (filename, O_RDONLY);
    if (fd < 0)
	return false;
    struct ScorefileHeader header;
    struct iovec iov[2] = {
	{ &header, sizeof(header)},
	{ scores, scoresSize}
    };
    // Interrupted locks can be retried; other failures end the read.
    while (flock (fd, LOCK_SH) < 0) {
        if (errno == EINTR)
            continue;
        perror (filename);
        close (fd);
        return false;
    }
    ssize_t br = readv (fd, iov, ArraySize(iov));
    close (fd);
    // Check that score list appears valid
    return br >= 0 && (size_t) br == sizeof(header)+scoresSize
	    && memcmp (header.magic, magic, sizeof(header.magic)) == 0
	    && header.sum == bsdsum (scores, scoresSize, 0);
}

void write_score_file (const char* filename, const char* magic, const void* scores, size_t scoresSize)
{
    // Create a local score file without truncating before taking the lock.
    int fd = open (filename, O_WRONLY|O_CREAT, S_IRUSR|S_IWUSR);
    if (fd < 0) {
        perror (filename);
        return;
    }
    struct ScorefileHeader header;
    memcpy (header.magic, magic, sizeof(header.magic));
    header.sum = bsdsum (scores, scoresSize, 0);
    const struct iovec iov[2] = {
	{ &header, sizeof(header)},
	{ (void*) scores, scoresSize}
    };
    // Hold the lock through writing and truncating the previous record.
    while (flock (fd, LOCK_EX) < 0) {
        if (errno == EINTR)
            continue;
        perror (filename);
        close (fd);
        return;
    }
    ssize_t written = writev (fd, iov, ArraySize(iov));
    if (written < 0)
        perror (filename);
    else if ((size_t) written != sizeof(header)+scoresSize)
        fprintf (stderr, "Error: incomplete score write to '%s'\n", filename);
    else if (ftruncate (fd, written) < 0)
        perror (filename);
    if (close (fd) < 0)
        perror (filename);
}
