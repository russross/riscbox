// Copyright (c) 1994 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "gomoku.h"

enum {
    BITS_PER_INT = sizeof(int) * CHAR_BIT,
    MAPSZ = BAREA / BITS_PER_INT
};

enum {
    A=1, B, C, D, E, F, G, H, J,
    K, L, M, N, O, P, Q, R, S, T
};

#define PT(x,y)		((x) + BSZ1 * (y))

#define BIT_SET(a, b)	((a)[(b)/BITS_PER_INT] |= (1 << ((b) % BITS_PER_INT)))
#define BIT_CLR(a, b)	((a)[(b)/BITS_PER_INT] &= ~(1 << ((b) % BITS_PER_INT)))
#define BIT_TEST(a, b)	((a)[(b)/BITS_PER_INT] & (1 << ((b) % BITS_PER_INT)))

static struct combostr* _hashcombos[FAREA];	// hash list for finding duplicates
static struct combostr* _sortcombos;		// combos at higher levels
static unsigned _combolen = 0;		// number of combos in _sortcombos
static unsigned _nextcolor = 0;		// color of next move
static unsigned _elistcnt = 0;		// count of struct elist allocated
static unsigned _combocnt = 0;		// count of struct combostr allocated
static unsigned _forcemap [MAPSZ];	// map for blocking <1,x> combos
static unsigned _tmpmap [MAPSZ];	// map for blocking <1,x> combos
static unsigned _nforce;		// count of opponent <1,x> combos

//----------------------------------------------------------------------

static bool better (const struct spotstr *sp, const struct spotstr *sp1, int us);
static void scanframes (unsigned color);
static void updatecombo (struct combostr* cbp, unsigned color);
static void makecombo2 (struct combostr* ocbp, struct spotstr *osp, int off, int s);
static void makecombo (struct combostr* ocbp, struct spotstr *osp, int off, int s);
static void makeempty (struct combostr* ocbp);
static void appendcombo (struct combostr* cbp, int color UNUSED);
static int sortcombo (struct combostr** scbpp, struct combostr** cbpp, struct combostr* fcbp);
static int checkframes (struct combostr* cbp, struct combostr* fcbp, struct spotstr* osp, int s, struct ovlp_info* vertices);
static void addframes (unsigned level);

//----------------------------------------------------------------------

int pickmove (int us)
{
    // first move is easy
    if (_movenum == 1) {
	_lastHumanMove = PT(K, 11);
	return PT(K, 10);
    }

    // initialize all the board values
    for (struct spotstr* sp = &_board[PT(T, 20)]; --sp >= &_board[PT(A, 1)];) {
	sp->s_combo[BLACK].s = MAXCOMBO + 1;
	sp->s_combo[WHITE].s = MAXCOMBO + 1;
	sp->s_level[BLACK] = 255;
	sp->s_level[WHITE] = 255;
	sp->s_nforce[BLACK] = 0;
	sp->s_nforce[WHITE] = 0;
	sp->s_flg &= ~(FFLAGALL | MFLAGALL);
    }
    _nforce = 0;
    memset(_forcemap, 0, sizeof(_forcemap));

    // compute new values
    _nextcolor = us;
    scanframes(BLACK);
    scanframes(WHITE);

    // find the spot with the highest value
    struct spotstr *sp = &_board[PT(T, 19)], *sp1 = sp, *sp2 = sp;
    while (--sp >= &_board[PT(A, 1)]) {
	if (sp->s_occ != EMPTY)
	    continue;
	// pick the best black move
	if (better(sp, sp1, BLACK))
	    sp1 = sp;
	// pick the best white move
	if (better(sp, sp2, WHITE))
	    sp2 = sp;
    }

    const union comboval* Ocp = &sp1->s_combo[BLACK];
    const union comboval* Tcp = &sp2->s_combo[WHITE];
    if (us == BLACK) {
	const union comboval* t = Ocp;
	Ocp = Tcp;
	Tcp = t;
	sp = sp1;
	sp1 = sp2;
	sp2 = sp;
    }
    // Block their combo only if we have to (i.e., if they are one move
    // away from completing a force and we don't have a force that
    // we can complete which takes fewer moves to win).
    if (Tcp->c.a <= 1 && (Ocp->c.a > 1 || Tcp->c.a + Tcp->c.b < Ocp->c.a + Ocp->c.b))
	return sp2 - _board;
    return sp1 - _board;
}

// Return true if spot 'sp' is better than spot 'sp1' for color 'us'.
static bool better (const struct spotstr *sp, const struct spotstr *sp1, int us)
{
    if (sp->s_combo[us].s < sp1->s_combo[us].s)
	return true;
    if (sp->s_combo[us].s != sp1->s_combo[us].s)
	return false;
    if (sp->s_level[us] < sp1->s_level[us])
	return true;
    if (sp->s_level[us] != sp1->s_level[us])
	return false;
    if (sp->s_nforce[us] > sp1->s_nforce[us])
	return true;
    if (sp->s_nforce[us] != sp1->s_nforce[us])
	return false;

    unsigned them = !us;
    unsigned s = sp - _board, s1 = sp1 - _board;
    if (BIT_TEST(_forcemap, s) && !BIT_TEST(_forcemap, s1))
	return true;
    if (!BIT_TEST(_forcemap, s) && BIT_TEST(_forcemap, s1))
	return false;
    if (sp->s_combo[them].s < sp1->s_combo[them].s)
	return true;
    if (sp->s_combo[them].s != sp1->s_combo[them].s)
	return false;
    if (sp->s_level[them] < sp1->s_level[them])
	return true;
    if (sp->s_level[them] != sp1->s_level[them])
	return false;
    if (sp->s_nforce[them] > sp1->s_nforce[them])
	return true;
    if (sp->s_nforce[them] != sp1->s_nforce[them])
	return false;

    if (sp->s_wval > sp1->s_wval)
	return true;
    if (sp->s_wval != sp1->s_wval)
	return false;

    return nrand(2);
}

static unsigned _curcolor = 0;	// implicit parameter to makecombo()
static unsigned _curlevel = 0;	// implicit parameter to makecombo()

// Scan the sorted list of non-empty frames and
// update the minimum combo values for each empty spot.
// Also, try to combine frames to find more complex (chained) moves.
static void scanframes (unsigned color)
{
    _curcolor = color;

    // check for empty list of frames
    struct combostr* cbp = _sortframes[color];
    if (!cbp)
	return;

    // quick check for four in a row
    struct spotstr* sp = &_board[cbp->c_vertex];
    int d = cbp->c_dir;
    union comboval cb = {.s = sp->s_fval[color][d].s};
    if (cb.s < 0x101) {
	d = _dd[d];
	for (int i = 5 + cb.c.b; --i >= 0; sp += d) {
	    if (sp->s_occ != EMPTY)
		continue;
	    sp->s_combo[color].s = cb.s;
	    sp->s_level[color] = 1;
	}
	return;
    }

    // Update the minimum combo value for each spot in the frame
    // and try making all combinations of two frames intersecting at
    // an empty spot.
    unsigned n = _combolen;
    struct combostr* ecbp = cbp;
    do {
	sp = &_board[cbp->c_vertex];
	int r = cbp->c_dir;
	union comboval* cp = &sp->s_fval[color][r];
	d = _dd[r];
	int i = 0;
	if (cp->c.b) {
	    // Since this is the first spot of an open ended
	    // frame, we treat it as a closed frame.
	    cb.c.a = cp->c.a + 1;
	    cb.c.b = 0;
	    if (cb.s < sp->s_combo[color].s) {
		sp->s_combo[color].s = cb.s;
		sp->s_level[color] = 1;
	    }
	    // Try combining other frames that intersect
	    // at this spot.
	    makecombo2(cbp, sp, 0, cb.s);
	    if (cp->s != 0x101)
		cb.s = cp->s;
	    else if (color != _nextcolor)
		memset(_tmpmap, 0, sizeof(_tmpmap));
	    sp += d;
	    i = 1;
	} else
	    cb.s = cp->s;
	for (; i < 5; ++i, sp += d) {	// for each spot
	    if (sp->s_occ != EMPTY)
		continue;
	    if (cp->s < sp->s_combo[color].s) {
		sp->s_combo[color].s = cp->s;
		sp->s_level[color] = 1;
	    }
	    if (cp->s == 0x101) {
		++sp->s_nforce[color];
		if (color != _nextcolor) {
		    n = sp - _board;
		    BIT_SET(_tmpmap, n);
		}
	    }
	    // Try combining other frames that intersect
	    // at this spot.
	    makecombo2(cbp, sp, i, cb.s);
	}
	if (cp->s == 0x101 && color != _nextcolor) {
	    if (_nforce == 0)
		memcpy(_forcemap, _tmpmap, sizeof(_tmpmap));
	    else
		for (i = 0; (unsigned) i < MAPSZ; ++i)
		    _forcemap[i] &= _tmpmap[i];
	}
	// mark frame as having been processed
	_board[cbp->c_vertex].s_flg |= MFLAG << r;
    } while ((cbp = cbp->c_next) != ecbp);

    // Try to make new 3rd level combos, 4th level, etc.
    // Limit the search depth early in the game.
    d = 2;
    while (d <= (int)((_movenum + 1) >> 1) && _combolen > n) {
	n = _combolen;
	addframes(d);
	++d;
    }

    // scan for combos at empty spots
    for (sp = &_board[PT(T, 20)]; --sp >= &_board[PT(A, 1)];) {
	struct elist* nep = NULL;
	for (struct elist* ep = sp->s_empty; ep; ep = nep) {
	    cbp = ep->e_combo;
	    if (cbp->c_combo.s <= sp->s_combo[color].s) {
		if (cbp->c_combo.s != sp->s_combo[color].s) {
		    sp->s_combo[color].s = cbp->c_combo.s;
		    sp->s_level[color] = cbp->c_nframes;
		} else if (cbp->c_nframes < sp->s_level[color])
		    sp->s_level[color] = cbp->c_nframes;
	    }
	    nep = ep->e_next;
	    free(ep);
	    --_elistcnt;
	}
	sp->s_empty = NULL;
	for (struct elist* ep = sp->s_nempty; ep; ep = nep) {
	    cbp = ep->e_combo;
	    if (cbp->c_combo.s <= sp->s_combo[color].s) {
		if (cbp->c_combo.s != sp->s_combo[color].s) {
		    sp->s_combo[color].s = cbp->c_combo.s;
		    sp->s_level[color] = cbp->c_nframes;
		} else if (cbp->c_nframes < sp->s_level[color])
		    sp->s_level[color] = cbp->c_nframes;
	    }
	    nep = ep->e_next;
	    free(ep);
	    --_elistcnt;
	}
	sp->s_nempty = NULL;
    }

    // remove old combos
    if ((cbp = _sortcombos) != (struct combostr *) 0) {
	// scan the list
	ecbp = cbp;
	struct combostr *ncbp;
	do {
	    ncbp = cbp->c_next;
	    free(cbp);
	    --_combocnt;
	} while ((cbp = ncbp) != ecbp);
	_sortcombos = NULL;
    }
    _combolen = 0;
}

// Compute all level 2 combos of frames intersecting spot 'osp'
// within the frame 'ocbp' and combo value 's'.
static void makecombo2 (struct combostr *ocbp, struct spotstr *osp, int off, int s)
{
    // try to combine a new frame with those found so far
    union comboval ocb = {.s = s};
    int baseB = ocb.c.a + ocb.c.b - 1;
    int fcnt = ocb.c.a - 2;
    unsigned emask = fcnt ? ((ocb.c.b ? 0x1E : 0x1F) & ~(1 << off)) : 0;
    for (int r = 4; --r >= 0;) {   // for each direction
	// don't include frames that overlap in the same direction
	if (r == ocbp->c_dir)
	    continue;
	int d = _dd[r];
	// Frame A combined with B is the same value as B combined with A
	// so skip frames that have already been processed (MFLAG).
	// Also skip blocked frames (BFLAG) and frames that are <1,x>
	// since combining another frame with it isn't valid.
	unsigned bmask = (BFLAG | FFLAG | MFLAG) << r;
	struct spotstr* fsp = osp;
	for (unsigned f = 0; f < 5; ++f, fsp -= d) {	// for each frame
	    if (fsp->s_occ == BORDER)
		break;
	    if (fsp->s_flg & bmask)
		continue;

	    // don't include frames of the wrong color
	    union comboval fcb = {.s = fsp->s_fval[_curcolor][r].s};
	    if (fcb.c.a >= MAXA)
		continue;

	    // Get the combo value for this frame.
	    // If this is the end point of the frame,
	    // use the closed ended value for the frame.
	    if ((f == 0 && fcb.c.b) || fcb.s == 0x101) {
		++fcb.c.a;
		fcb.c.b = 0;
	    }

	    // compute combo value
	    int c = fcb.c.a + ocb.c.a - 3;
	    if (c > 4)
		continue;
	    int n = fcb.c.a + fcb.c.b - 1;
	    if (baseB < n)
		n = baseB;

	    // make a new combo!
	    struct combostr* ncbp = (struct combostr*) malloc(sizeof(struct combostr) + 2*sizeof(struct combostr*));
	    if (!ncbp)
		exit (EXIT_FAILURE);
	    struct combostr** scbpp = (struct combostr**)(ncbp + 1);
	    struct combostr* fcbp = fsp->s_frame[r];
	    if (ocbp < fcbp) {
		scbpp[0] = ocbp;
		scbpp[1] = fcbp;
	    } else {
		scbpp[0] = fcbp;
		scbpp[1] = ocbp;
	    }
	    ncbp->c_combo.c.a = c;
	    ncbp->c_combo.c.b = n;
	    ncbp->c_link[0] = ocbp;
	    ncbp->c_link[1] = fcbp;
	    ncbp->c_linkv[0].s = ocb.s;
	    ncbp->c_linkv[1].s = fcb.s;
	    ncbp->c_voff[0] = off;
	    ncbp->c_voff[1] = f;
	    ncbp->c_vertex = osp - _board;
	    ncbp->c_nframes = 2;
	    ncbp->c_dir = 0;
	    ncbp->c_frameindex = 0;
	    ncbp->c_flg = (ocb.c.b) ? C_OPEN_0 : 0;
	    if (fcb.c.b)
		ncbp->c_flg |= C_OPEN_1;
	    ncbp->c_framecnt[0] = fcnt;
	    ncbp->c_emask[0] = emask;
	    ncbp->c_framecnt[1] = fcb.c.a - 2;
	    ncbp->c_emask[1] = ncbp->c_framecnt[1] ? ((fcb.c.b ? 0x1E : 0x1F) & ~(1 << f)) : 0;
	    ++_combocnt;

	    if (c > 1) {
		// record the empty spots that will complete this combo
		makeempty(ncbp);

		// add the new combo to the end of the list
		appendcombo(ncbp, _curcolor);
	    } else {
		updatecombo(ncbp, _curcolor);
		free(ncbp);
		--_combocnt;
	    }
	}
    }
}

// Scan the sorted list of frames and try to add a frame to
// combinations of 'level' number of frames.
static void addframes (unsigned level)
{
    _curlevel = level;

    // scan for combos at empty spots
    unsigned i = _curcolor;
    for (struct spotstr* sp = &_board[PT(T, 20)]; --sp >= &_board[PT(A, 1)];) {
	for (struct elist* ep = sp->s_empty, *nep = NULL; ep; ep = nep) {
	    struct combostr* cbp = ep->e_combo;
	    if (cbp->c_combo.s <= sp->s_combo[i].s) {
		if (cbp->c_combo.s != sp->s_combo[i].s) {
		    sp->s_combo[i].s = cbp->c_combo.s;
		    sp->s_level[i] = cbp->c_nframes;
		} else if (cbp->c_nframes < sp->s_level[i])
		    sp->s_level[i] = cbp->c_nframes;
	    }
	    nep = ep->e_next;
	    free(ep);
	    --_elistcnt;
	}
	sp->s_empty = sp->s_nempty;
	sp->s_nempty = (struct elist *) 0;
    }

    // try to add frames to the uncompleted combos at level _curlevel
    struct combostr* cbp = _sortframes[_curcolor], *ecbp = cbp;
    do {
	struct spotstr* fsp = &_board[cbp->c_vertex];
	int r = cbp->c_dir;
	// skip frames that are part of a <1,x> combo
	if (fsp->s_flg & (FFLAG << r))
	    continue;

	// Don't include <1,x> combo frames,
	// treat it as a closed three in a row instead.
	union comboval fcb = {.s = fsp->s_fval[_curcolor][r].s};
	if (fcb.s == 0x101)
	    fcb.s = 0x200;
	// If this is an open ended frame, use
	// the combo value with the end closed.
	if (fsp->s_occ == EMPTY) {
	    union comboval cb = fcb;
	    if (fcb.c.b) {
		cb.c.a = fcb.c.a + 1;
		cb.c.b = 0;
	    }
	    makecombo(cbp, fsp, 0, cb.s);
	}

	// The next four spots are handled the same for both
	// open and closed ended frames.
	int d = _dd[r];
	struct spotstr* sp = fsp + d;
	for (unsigned j = 1; j < 5; ++j, sp += d) {
	    if (sp->s_occ != EMPTY)
		continue;
	    makecombo(cbp, sp, j, fcb.s);
	}
    } while ((cbp = cbp->c_next) != ecbp);

    // put all the combos in the hash list on the sorted list
    struct combostr** cbpp = &_hashcombos[FAREA];
    do {
	cbp = *--cbpp;
	if (cbp == (struct combostr *) 0)
	    continue;
	*cbpp = (struct combostr *) 0;
	ecbp = _sortcombos;
	if (ecbp == (struct combostr *) 0)
	    _sortcombos = cbp;
	else {
	    // append to sort list
	    struct combostr* pcbp = ecbp->c_prev;
	    pcbp->c_next = cbp;
	    ecbp->c_prev = cbp->c_prev;
	    cbp->c_prev->c_next = ecbp;
	    cbp->c_prev = pcbp;
	}
    } while (cbpp != _hashcombos);
}

// Compute all level N combos of frames intersecting spot 'osp'
// within the frame 'ocbp' and combo value 's'.
static void makecombo (struct combostr *ocbp, struct spotstr *osp, int off, int s)
{
    union comboval ocb = {.s = s};
    int baseB = ocb.c.a + ocb.c.b - 1;
    int fcnt = ocb.c.a - 2;
    unsigned emask = fcnt ? ((ocb.c.b ? 0x1E : 0x1F) & ~(1 << off)) : 0;
    for (struct elist* ep = osp->s_empty; ep; ep = ep->e_next) {
	// check for various kinds of overlap
	struct combostr* cbp = ep->e_combo;
	struct ovlp_info vertices[1];
	int verts = checkframes(cbp, ocbp, osp, s, vertices);
	if (verts < 0)
	    continue;

	// check to see if this frame forms a valid loop
	if (verts) {
	    struct spotstr* sp = &_board[vertices[0].o_intersect];
	    // It is a valid loop if the intersection spot
	    // of the frame we are trying to attach is one
	    // of the completion spots of the combostr
	    // we are trying to attach the frame to.
	    for (struct elist* nep = sp->s_empty; nep; nep = nep->e_next) {
		if (nep->e_combo == cbp)
		    goto fnd;
		if (nep->e_combo->c_nframes < cbp->c_nframes)
		    break;
	    }
	    // frame overlaps but not at a valid spot
	    continue;
	  fnd:
	    ;
	}

	// compute the first half of the combo value
	int c = cbp->c_combo.c.a + ocb.c.a - verts - 3;
	if (c > 4)
	    continue;

	// compute the second half of the combo value
	int n = ep->e_fval.c.a + ep->e_fval.c.b - 1;
	if (baseB < n)
	    n = baseB;

	// make a new combo!
	struct combostr* ncbp = (struct combostr*) malloc (sizeof(struct combostr) + (cbp->c_nframes + 1) * sizeof(struct combostr*));
	if (ncbp == NULL)
	    exit (EXIT_FAILURE);
	struct combostr** scbpp = (struct combostr**)(ncbp + 1);
	if (sortcombo (scbpp, (struct combostr**)(cbp + 1), ocbp)) {
	    free (ncbp);
	    continue;
	}
	++_combocnt;

	ncbp->c_combo.c.a = c;
	ncbp->c_combo.c.b = n;
	ncbp->c_link[0] = cbp;
	ncbp->c_link[1] = ocbp;
	ncbp->c_linkv[1].s = ocb.s;
	ncbp->c_voff[1] = off;
	ncbp->c_vertex = osp - _board;
	ncbp->c_nframes = cbp->c_nframes + 1;
	ncbp->c_flg = ocb.c.b ? C_OPEN_1 : 0;
	ncbp->c_frameindex = ep->e_frameindex;

	// Update the completion spot mask of the frame we
	// are attaching 'ocbp' to so the intersection isn't
	// listed twice.
	ncbp->c_framecnt[0] = ep->e_framecnt;
	ncbp->c_emask[0] = ep->e_emask;
	if (verts) {
	    ncbp->c_flg |= C_LOOP;
	    ncbp->c_dir = vertices[0].o_frameindex;
	    ncbp->c_framecnt[1] = fcnt - 1;
	    if (ncbp->c_framecnt[1]) {
		n = (vertices[0].o_intersect - ocbp->c_vertex) / _dd[ocbp->c_dir];
		ncbp->c_emask[1] = emask & ~(1 << n);
	    } else
		ncbp->c_emask[1] = 0;
	    ncbp->c_voff[0] = vertices[0].o_off;
	} else {
	    ncbp->c_dir = 0;
	    ncbp->c_framecnt[1] = fcnt;
	    ncbp->c_emask[1] = emask;
	    ncbp->c_voff[0] = ep->e_off;
	}

	if (c > 1) {
	    // record the empty spots that will complete this combo
	    makeempty(ncbp);
	    ++_combolen;
	} else {
	    // update board values
	    updatecombo(ncbp, _curcolor);
	}
    }
}

enum { MAXDEPTH = 100 };
static struct elist einfo [MAXDEPTH];
static struct combostr* ecombo [MAXDEPTH];	// separate from elist to save space

// Add the combostr 'ocbp' to the empty spots list for each empty spot
// in 'ocbp' that will complete the combo.
static void makeempty (struct combostr *ocbp)
{
    // should never happen but check anyway
    unsigned nframes = ocbp->c_nframes;
    if (nframes >= MAXDEPTH)
	return;

    // The lower level combo can be pointed to by more than one
    // higher level 'struct combostr' so we can't modify the
    // lower level. Therefore, higher level combos store the
    // real mask of the lower level frame in c_emask[0] and the
    // frame number in c_frameindex.
    //
    // First we traverse the tree from top to bottom and save the
    // connection info. Then we traverse the tree from bottom to
    // top overwriting lower levels with the newer emask information.
    //
    struct elist* ep = &einfo[nframes];
    struct combostr** cbpp = &ecombo[nframes];
    struct combostr* cbp = ocbp;
    for (; cbp->c_link[1]; cbp = cbp->c_link[0]) {
	--ep;
	ep->e_combo = cbp;
	*--cbpp = cbp->c_link[1];
	ep->e_off = cbp->c_voff[1];
	ep->e_frameindex = cbp->c_frameindex;
	ep->e_fval.s = cbp->c_linkv[1].s;
	ep->e_framecnt = cbp->c_framecnt[1];
	ep->e_emask = cbp->c_emask[1];
    }
    cbp = ep->e_combo;
    --ep;
    ep->e_combo = cbp;
    *--cbpp = cbp->c_link[0];
    ep->e_off = cbp->c_voff[0];
    ep->e_frameindex = 0;
    ep->e_fval.s = cbp->c_linkv[0].s;
    ep->e_framecnt = cbp->c_framecnt[0];
    ep->e_emask = cbp->c_emask[0];

    // now update the emask info
    unsigned nloops = 0;
    ep += 2;
    for (unsigned i = 2; i < nframes; ++i, ++ep) {
	cbp = ep->e_combo;
	struct elist* nep = &einfo[ep->e_frameindex];
	nep->e_framecnt = cbp->c_framecnt[0];
	nep->e_emask = cbp->c_emask[0];

	if (cbp->c_flg & C_LOOP) {
	    ++nloops;
	    // Account for the fact that this frame connects
	    // to a previous one (thus forming a loop).
	    nep = &einfo[cbp->c_dir];
	    if (--nep->e_framecnt)
		nep->e_emask &= ~(1 << cbp->c_voff[0]);
	    else
		nep->e_emask = 0;
	}
    }

    // We only need to update the emask values of "complete" loops
    // to include the intersection spots.
    if (nloops && ocbp->c_combo.c.a == 2) {
	// process loops from the top down
	ep = &einfo[nframes];
	do {
	    --ep;
	    cbp = ep->e_combo;
	    if (!(cbp->c_flg & C_LOOP))
		continue;

	    // Update the emask values to include the
	    // intersection spots.
	    struct elist* nep = &einfo[cbp->c_dir];
	    nep->e_framecnt = 1;
	    nep->e_emask = 1 << cbp->c_voff[0];
	    ep->e_framecnt = 1;
	    ep->e_emask = 1 << ep->e_off;
	    ep = &einfo[ep->e_frameindex];
	    do {
		ep->e_framecnt = 1;
		ep->e_emask = 1 << ep->e_off;
		ep = &einfo[ep->e_frameindex];
	    } while (ep > nep);
	} while (ep != einfo);
    }

    // check all the frames for completion spots
    ep = einfo;
    cbpp = ecombo;
    for (unsigned i = 0; i < nframes; ++i, ++ep, ++cbpp) {
	// skip this frame if there are no incomplete spots in it
	unsigned emask = ep->e_emask;
	if (!emask)
	    continue;
	cbp = *cbpp;
	struct spotstr* sp = &_board[cbp->c_vertex];
	int d = _dd[cbp->c_dir];
	for (unsigned s = 0, m = 1; s < 5; ++s, sp += d, m <<= 1) {
	    if (sp->s_occ != EMPTY || !(emask & m))
		continue;

	    // add the combo to the list of empty spots
	    struct elist* nep = (struct elist*) malloc (sizeof(struct elist));
	    if (!nep)
		exit (EXIT_FAILURE);
	    nep->e_combo = ocbp;
	    nep->e_off = s;
	    nep->e_frameindex = i;
	    if (ep->e_framecnt > 1) {
		nep->e_framecnt = ep->e_framecnt - 1;
		nep->e_emask = emask & ~m;
	    } else {
		nep->e_framecnt = 0;
		nep->e_emask = 0;
	    }
	    nep->e_fval.s = ep->e_fval.s;

	    // sort by the number of frames in the combo
	    nep->e_next = sp->s_nempty;
	    sp->s_nempty = nep;
	    ++_elistcnt;
	}
    }
}

// Update the board value based on the combostr.
// This is called only if 'cbp' is a <1,x> combo.
// We handle things differently depending on whether the next move
// would be trying to "complete" the combo or trying to block it.
static void updatecombo (struct combostr* cbp, unsigned color)
{
    // save the top level value for the whole combo
    union comboval cb = {.c.a = cbp->c_combo.c.a};
    unsigned nframes = cbp->c_nframes;

    if (color != _nextcolor)
	memset(_tmpmap, 0, sizeof(_tmpmap));

    unsigned flg = 0;
    for (struct combostr* tcbp; (tcbp = cbp->c_link[1]); cbp = cbp->c_link[0]) {
	flg = cbp->c_flg;
	cb.c.b = cbp->c_combo.c.b;
	if (color == _nextcolor) {
	    // update the board value for the vertex
	    struct spotstr* sp = &_board[cbp->c_vertex];
	    ++sp->s_nforce[color];
	    if (cb.s <= sp->s_combo[color].s) {
		if (cb.s != sp->s_combo[color].s) {
		    sp->s_combo[color].s = cb.s;
		    sp->s_level[color] = nframes;
		} else if (nframes < sp->s_level[color])
		    sp->s_level[color] = nframes;
	    }
	} else {
	    // update the board values for each spot in frame
	    int s = tcbp->c_vertex;
	    struct spotstr* sp = &_board[s];
	    int d = _dd[tcbp->c_dir];
	    for (int i = (flg & C_OPEN_1) ? 6 : 5; --i >= 0; sp += d, s += d) {
		if (sp->s_occ != EMPTY)
		    continue;
		++sp->s_nforce[color];
		if (cb.s <= sp->s_combo[color].s) {
		    if (cb.s != sp->s_combo[color].s) {
			sp->s_combo[color].s = cb.s;
			sp->s_level[color] = nframes;
		    } else if (nframes < sp->s_level[color])
			sp->s_level[color] = nframes;
		}
		BIT_SET(_tmpmap, s);
	    }
	}

	// mark the frame as being part of a <1,x> combo
	_board[tcbp->c_vertex].s_flg |= FFLAG << tcbp->c_dir;
    }

    if (color != _nextcolor) {
	// update the board values for each spot in frame
	int s = cbp->c_vertex;
	struct spotstr* sp = &_board[s];
	int d = _dd[cbp->c_dir];
	for (int i = (flg & C_OPEN_0) ? 6 : 5; --i >= 0; sp += d, s += d) {
	    if (sp->s_occ != EMPTY)
		continue;
	    ++sp->s_nforce[color];
	    if (cb.s <= sp->s_combo[color].s) {
		if (cb.s != sp->s_combo[color].s) {
		    sp->s_combo[color].s = cb.s;
		    sp->s_level[color] = nframes;
		} else if (nframes < sp->s_level[color])
		    sp->s_level[color] = nframes;
	    }
	    BIT_SET(_tmpmap, s);
	}
	if (_nforce == 0)
	    memcpy(_forcemap, _tmpmap, sizeof(_tmpmap));
	else {
	    for (unsigned i = 0; i < MAPSZ; ++i)
		_forcemap[i] &= _tmpmap[i];
	}
	++_nforce;
    }

    // mark the frame as being part of a <1,x> combo
    _board[cbp->c_vertex].s_flg |= FFLAG << cbp->c_dir;
}

// Add combo to the end of the list.
static void appendcombo (struct combostr* cbp, int color UNUSED)
{
    ++_combolen;
    struct combostr* ncbp = _sortcombos;
    if (ncbp == (struct combostr *) 0) {
	_sortcombos = cbp;
	cbp->c_next = cbp;
	cbp->c_prev = cbp;
	return;
    }
    struct combostr* pcbp = ncbp->c_prev;
    cbp->c_next = ncbp;
    cbp->c_prev = pcbp;
    ncbp->c_prev = cbp;
    pcbp->c_next = cbp;
}

// Return zero if it is valid to combine frame 'fcbp' with the frames
// in 'cbp' and forms a linked chain of frames (i.e., a tree; no loops).
// Return positive if combining frame 'fcbp' to the frames in 'cbp'
// would form some kind of valid loop. Also return the intersection spots
// in 'vertices[]' beside the known intersection at spot 'osp'.
// Return -1 if 'fcbp' should not be combined with 'cbp'.
// 's' is the combo value for frame 'fcpb'.
static int checkframes (struct combostr* cbp, struct combostr* fcbp, struct spotstr* osp, int s, struct ovlp_info* vertices)
{
    unsigned flg = 0;
    union comboval cb = {.s = s};
    int fcnt = cb.c.a - 2;
    int verts = 0;
    int index = cbp->c_nframes;
    int n = (fcbp - _frames) * FAREA;
    unsigned char* str = &_overlap[n];
    const short* ip = &_intersect[n];
    // i == which overlap bit to test based on whether 'fcbp' is
    // an open or closed frame.
    int i = cb.c.b ? 2 : 0;
    struct combostr* lcbp = NULL;
    for (const struct combostr* tcbp; (tcbp = cbp->c_link[1]); lcbp = cbp, cbp = cbp->c_link[0]) {
	if (tcbp == fcbp)
	    return -1;       // fcbp is already included

	// check for intersection of 'tcbp' with 'fcbp'
	--index;
	unsigned mask = str[tcbp - _frames];
	flg = cbp->c_flg;
	n = i + ((flg & C_OPEN_1) != 0);
	if (mask & (1 << n)) {
	    // The two frames are not independent if they
	    // both lie in the same line and intersect at
	    // more than one point.
	    if (tcbp->c_dir == fcbp->c_dir && (mask & (0x10 << n)))
		return -1;
	    // If this is not the spot we are attaching
	    // 'fcbp' to and it is a reasonable intersection
	    // spot, then there might be a loop.
	    n = ip[tcbp - _frames];
	    if (osp != &_board[n]) {
		// check to see if this is a valid loop
		if (verts)
		    return -1;
		if (fcnt == 0 || cbp->c_framecnt[1] == 0)
		    return -1;
		// Check to be sure the intersection is not
		// one of the end points if it is an open
		// ended frame.
		if ((flg & C_OPEN_1) && (n == tcbp->c_vertex || n == tcbp->c_vertex + 5 * _dd[tcbp->c_dir]))
		    return -1;	// invalid overlap
		if (cb.c.b && (n == fcbp->c_vertex || n == fcbp->c_vertex + 5 * _dd[fcbp->c_dir]))
		    return -1;	// invalid overlap

		vertices->o_intersect = n;
		vertices->o_fcombo = cbp;
		vertices->o_link = 1;
		vertices->o_off = (n - tcbp->c_vertex) / _dd[tcbp->c_dir];
		vertices->o_frameindex = index;
		++verts;
	    }
	}
	n = i + ((flg & C_OPEN_0) != 0);
    }
    if (cbp == fcbp)
	return -1;	       // fcbp is already included

    // check for intersection of 'cbp' with 'fcbp'
    unsigned mask = str[cbp - _frames];
    if (mask & (1 << n)) {
	// The two frames are not independent if they
	// both lie in the same line and intersect at
	// more than one point.
	if (cbp->c_dir == fcbp->c_dir && (mask & (0x10 << n)))
	    return -1;
	// If this is not the spot we are attaching
	// 'fcbp' to and it is a reasonable intersection
	// spot, then there might be a loop.
	n = ip[cbp - _frames];
	if (osp != &_board[n]) {
	    // check to see if this is a valid loop
	    if (verts)
		return -1;
	    if (fcnt == 0 || lcbp->c_framecnt[0] == 0)
		return -1;
	    // Check to be sure the intersection is not
	    // one of the end points if it is an open
	    // ended frame.
	    if ((flg & C_OPEN_0) && (n == cbp->c_vertex || n == cbp->c_vertex + 5 * _dd[cbp->c_dir]))
		return -1;   // invalid overlap
	    if (cb.c.b && (n == fcbp->c_vertex || n == fcbp->c_vertex + 5 * _dd[fcbp->c_dir]))
		return -1;   // invalid overlap

	    vertices->o_intersect = n;
	    vertices->o_fcombo = lcbp;
	    vertices->o_link = 0;
	    vertices->o_off = (n - cbp->c_vertex) / _dd[cbp->c_dir];
	    vertices->o_frameindex = 0;
	    ++verts;
	}
    }
    return verts;
}

// Merge sort the frame 'fcbp' and the sorted list of frames 'cbpp' and
// store the result in 'scbpp'. '_curlevel' is the size of the 'cbpp' array.
// Return true if this list of frames is already in the hash list.
// Otherwise, add the new combo to the hash list.
static int sortcombo (struct combostr** scbpp, struct combostr** cbpp, struct combostr* fcbp)
{
    struct combostr **spp;
    struct combostr *ecbp;
    int n, inx;

    // first build the new sorted list
    n = _curlevel + 1;
    spp = scbpp + n;
    struct combostr** cpp = cbpp + _curlevel;
    do {
	--cpp;
	if (fcbp > *cpp) {
	    *--spp = fcbp;
	    do
		*--spp = *cpp;
	    while (cpp-- != cbpp);
	    goto inserted;
	}
	*--spp = *cpp;
    } while (cpp != cbpp);
    *--spp = fcbp;
  inserted:;

    // now check to see if this list of frames has already been seen
    struct combostr* cbp = _hashcombos[inx = *scbpp - _frames];
    if (!cbp) {
	// Easy case, this list hasn't been seen.
	// Add it to the hash list.
	fcbp = (struct combostr*)((char*)scbpp - sizeof(struct combostr));
	_hashcombos[inx] = fcbp;
	fcbp->c_next = fcbp->c_prev = fcbp;
	return 0;
    }
    ecbp = cbp;
    do {
	cbpp = (struct combostr**) (cbp + 1);
	cpp = cbpp + n;
	spp = scbpp + n;
	++cbpp;		       // first frame is always the same
	do {
	    if (*--spp != *--cpp)
		goto next;
	} while (cpp != cbpp);
	// we found a match
	return 1;
      next:
	;
    } while ((cbp = cbp->c_next) != ecbp);
    // This list of frames hasn't been seen.
    // Add it to the hash list.
    ecbp = cbp->c_prev;
    fcbp = (struct combostr *) ((char *) scbpp - sizeof(struct combostr));
    fcbp->c_next = cbp;
    fcbp->c_prev = ecbp;
    cbp->c_prev = fcbp;
    ecbp->c_next = fcbp;
    return 0;
}
