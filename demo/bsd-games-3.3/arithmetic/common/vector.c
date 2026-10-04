// Copyright (c) 2016 by Mike Sharov <msharov@users.sourceforge.net>
// This file is free software, distributed under the BSD License.

#include "common.h"

void* xalloc (size_t sz)
{
    void* p = xrealloc (NULL, sz);
    memset (p, 0, sz);
    return p;
}

void* xrealloc (void* p, size_t sz)
{
    p = realloc (p, sz);
    if (!p) {
	cleanup_curses();
	puts ("Error: out of memory");
	exit (EXIT_FAILURE);
    }
    return p;
}

void vector_init (void* vv, size_t elsz)
{
    struct CharVector* v = vv;
    v->d = NULL;
    v->size = 0;
    v->allocated = 0;
    v->elsize = elsz;
}

void vector_reserve (void* vv, size_t sz)
{
    struct CharVector* v = vv;
    if (v->allocated >= sz)
	return;
    size_t nsz = v->allocated + !v->allocated;
    while (nsz < sz)
	nsz *= 2;
    v->d = xrealloc (v->d, nsz * v->elsize);
    memset (v->d + v->allocated * v->elsize, 0, (nsz - v->allocated) * v->elsize);
    v->allocated = nsz;
}

void vector_deallocate (void* vv)
{
    assert (vv);
    struct CharVector* v = vv;
    xfree (v->d);
    v->size = 0;
    v->allocated = 0;
}

void* vector_emplace (void* vv, size_t ip)
{
    assert (vv);
    struct CharVector* v = vv;
    assert (ip <= v->size && "out of bounds insert");
    vector_reserve (vv, v->size+1);
    char* ii = v->d + ip*v->elsize;
    memmove (ii+v->elsize, ii, (v->size-ip)*v->elsize);
    memset (ii, 0, v->elsize);
    ++v->size;
    return ii;
}

void vector_insert (void* vv, size_t ip, const void* e)
{
    struct CharVector* v = vv;
    void* h = vector_emplace (v, ip);
    memcpy (h, e, v->elsize);
}

void vector_insert_n (void* vv, size_t ip, const void* e, size_t en)
{
    struct CharVector* v = vv;
    assert (ip <= v->size && "out of bounds insert");
    vector_reserve (v, v->size+en);
    char* ii = v->d + ip*v->elsize;
    memmove (ii+en*v->elsize, ii, (v->size-ip)*v->elsize);
    memcpy (ii, e, en*v->elsize);
    v->size += en;
}

void vector_erase_n (void* vv, size_t ep, size_t n)
{
    struct CharVector* v = vv;
    char* ei = v->d + ep * v->elsize;
    char* eei = v->d + (ep+n) * v->elsize;
    assert (ep <= v->size && ep+n <= v->size && "out of bounds erase");
    memmove (ei, eei, (v->size - (ep+n)) * v->elsize);
    v->size -= n;
}

void vector_swap (void* vv1, void* vv2)
{
    struct CharVector *v1 = vv1, *v2 = vv2;
    assert (v1->elsize == v2->elsize && "can only swap identical vectors");
    struct CharVector t;
    memcpy (&t, v1, sizeof(struct CharVector));
    memcpy (v1, v2, sizeof(struct CharVector));
    memcpy (v2, &t, sizeof(struct CharVector));
}

size_t vector_lower_bound (const void* vv, vector_compare_fn_t cmp, const void* e)
{
    const struct CharVector* v = vv;
    size_t f = 0, l = v->size;
    while (f < l) {
	size_t m = (f + l)/2;
	if (0 > cmp (v->d + m*v->elsize, e))
	    f = m + 1;
	else
	    l = m;
    }
    return f;
}
