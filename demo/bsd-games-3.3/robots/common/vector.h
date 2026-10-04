// Copyright (c) 2016 by Mike Sharov <msharov@users.sourceforge.net>
// This file is free software, distributed under the BSD License.

#pragma once

#define ArraySize(a)	(sizeof(a)/sizeof(a[0]))
#define ArrayBlock(a)	a, ArraySize(a)

void*	xalloc (size_t sz) noexcept __attribute__((malloc));
void*	xrealloc (void* p, size_t sz) noexcept __attribute__((malloc));
#define xfree(p)	do { if (p) { free(p); p = NULL; }} while (false)

typedef int (*vector_compare_fn_t)(const void*, const void*);

struct CharVector {
    char*	d;
    size_t	size;
    size_t	allocated;
    size_t	elsize;
};

// Declares a vector for the given type.
// Use the VECTOR macro to instantiate it. Fields are marked const to
// ensure only the vector_ functions modify it.
#define DECLARE_VECTOR_TYPE(name,type)	\
struct name {				\
    type* const		d;		\
    const size_t	size;		\
    const size_t	allocated;	\
    const size_t	elsize;		\
}

#define VECTOR_INIT(vtype)	{ .elsize = sizeof(*(((vtype*)NULL)->d)) }
#define VECTOR(vtype,name)	vtype name = VECTOR_INIT(vtype)
#define VECTOR_MEMBER_INIT(vtype,name)	vector_init(name, sizeof(*(((vtype*)NULL)->d)))

#define vector_begin(v)			(v)->d
#define vector_end(v)			((v)->d+(v)->size)
#define vector_foreach(vtype,p,v)	for (vtype *p = vector_begin(&(v)), *p##end = vector_end(&(v)); p < p##end; ++p)
#define vector_p2i(v,p)			((p)-vector_begin(v))

void	vector_init (void* v, size_t elsz) noexcept;
void	vector_reserve (void* v, size_t sz) noexcept;
void	vector_deallocate (void* v) noexcept;
void	vector_insert (void* v, size_t ip, const void* e) noexcept __attribute__((nonnull));
void	vector_insert_n (void* v, size_t ip, const void* e, size_t en) noexcept __attribute__((nonnull));
void*	vector_emplace (void* v, size_t ip) noexcept;
void	vector_erase_n (void* v, size_t ep, size_t n) noexcept;
void	vector_swap (void* v1, void* v2) noexcept __attribute__((nonnull));
size_t	vector_lower_bound (const void* vv, vector_compare_fn_t cmp, const void* e) noexcept __attribute__((nonnull));

static inline void vector_erase (void* v, size_t ep)
    { vector_erase_n (v, ep, 1); }
static inline void vector_push_back (void* vv, const void* e)
    { struct CharVector* v = vv; vector_insert (v, v->size, e); }
static inline void* vector_emplace_back (void* vv)
    { struct CharVector* v = vv; return vector_emplace (v, v->size); }
static inline void vector_pop_back (void* vv)
    { struct CharVector* v = vv; vector_erase (v, v->size-1); }
static inline void vector_clear (void* vv)
    { struct CharVector* v = vv; v->size = 0; }
static inline void vector_detach (void* vv)
    { struct CharVector* v = vv; v->d = NULL; v->size = v->allocated = 0; }
static inline __attribute__((nonnull)) void vector_attach (void* vv, void* e, size_t n) {
    struct CharVector* v = vv;
    assert (!v->d && "This vector is already attached to something. Detach or deallocate first.");
    assert (e != NULL && "Attaching requires a non-null pointer");
    v->d = e; v->size = v->allocated = n;
}
static inline __attribute__((nonnull)) void vector_resize (void* vv, size_t sz) {
    struct CharVector* v = vv;
    vector_reserve (v, sz);
    v->size = sz;
}
static inline void __attribute__((nonnull)) vector_insert_sorted (void* vv, vector_compare_fn_t cmp, const void* e)
    { vector_insert (vv, vector_lower_bound (vv, cmp, e), e); }
static inline void __attribute__((nonnull)) vector_sort (void* vv, vector_compare_fn_t cmp)
    { struct CharVector* v = vv; qsort (v->d, v->size, v->elsize, cmp); }
static inline void* vector_unconst_elp (void* v, const void* elp)
    { return (const char*)elp-(const char*)v+(char*)v; }
