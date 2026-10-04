// Copyright (c) 1994 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "gomoku.h"

//----------------------------------------------------------------------

static void update_overlap (const struct spotstr* osp);

//----------------------------------------------------------------------

static const int _weight[5] = { 0, 1, 7, 22, 100 };

//----------------------------------------------------------------------

// Return values:
//      MOVEOK  everything is OK.
//      RESIGN  Player resigned.
//      ILLEGAL Illegal move.
//      WIN     The winning move was just played.
//      TIE     The game is a tie.
int makemove (int us, int mv)
{
    // check for end of game
    if (mv == RESIGN)
	return RESIGN;

    // check for illegal move
    struct spotstr* sp = &_board[mv];
    if (sp->s_occ != EMPTY)
	return ILLEGAL;

    // make move
    sp->s_occ = us;
    if (++_movenum == BSZ * BSZ)
	return TIE;

    // compute new frame values
    sp->s_wval = 0;
    struct spotstr* osp = sp;
    for (int r = 4; --r >= 0;) {   // for each direction
	int d = _dd[r];
	struct spotstr* fsp = osp;
	for (int f = 5; --f >= 0; fsp -= d) {	// for each frame
	    if (fsp->s_occ == BORDER)
		goto nextr;
	    if (fsp->s_flg & (BFLAG<<r))
		continue;

	    // remove this frame from the sorted list of frames
	    struct combostr* cbp = fsp->s_frame[r];
	    if (cbp->c_next) {
		if (_sortframes[BLACK] == cbp)
		    _sortframes[BLACK] = cbp->c_next;
		if (_sortframes[WHITE] == cbp)
		    _sortframes[WHITE] = cbp->c_next;
		cbp->c_next->c_prev = cbp->c_prev;
		cbp->c_prev->c_next = cbp->c_next;
	    }

	    // compute old weight value for this frame
	    union comboval* cp = &fsp->s_fval[BLACK][r];
	    int val = 0;
	    if (cp->s <= 0x500)
		val = _weight[5 - cp->c.a - cp->c.b];
	    cp = &fsp->s_fval[WHITE][r];
	    if (cp->s <= 0x500)
		val += _weight[5 - cp->c.a - cp->c.b];

	    // compute new combo value for this frame
	    sp = fsp;
	    unsigned space = sp->s_occ == EMPTY;
	    unsigned n = 0;
	    for (int i = 5; --i >= 0; sp += d) {	// for each spot
		if (sp->s_occ == us)
		    ++n;
		else if (sp->s_occ == EMPTY)
		    sp->s_wval -= val;
		else {
		    // this frame is now blocked, adjust values
		    fsp->s_flg |= (BFLAG<<r);
		    fsp->s_fval[BLACK][r].s = MAXCOMBO;
		    fsp->s_fval[WHITE][r].s = MAXCOMBO;
		    while (--i >= 0) {
			sp += d;
			if (sp->s_occ == EMPTY)
			    sp->s_wval -= val;
		    }
		    goto nextf;
		}
	    }

	    // check for game over
	    if (n == 5)
		return WIN;

	    // compute new value & combo number for this frame & color
	    fsp->s_fval[!us][r].s = MAXCOMBO;
	    cp = &fsp->s_fval[us][r];
	    // both ends open?
	    if (space && sp->s_occ == EMPTY) {
		cp->c.a = 4 - n;
		cp->c.b = 1;
	    } else {
		cp->c.a = 5 - n;
		cp->c.b = 0;
	    }
	    val = _weight[n];
	    sp = fsp;
	    for (int i = 5; --i >= 0; sp += d)	// for each spot
		if (sp->s_occ == EMPTY)
		    sp->s_wval += val;

	    // add this frame to the sorted list of frames by combo value
	    struct combostr* cbp1 = _sortframes[us];
	    if (!cbp1)
		_sortframes[us] = cbp->c_next = cbp->c_prev = cbp;
	    else {
		const union comboval* cp1 = &_board[cbp1->c_vertex].s_fval[us][cbp1->c_dir];
		if (cp->s <= cp1->s) {
		    // insert at the head of the list
		    _sortframes[us] = cbp;
		} else {
		    do {
			cbp1 = cbp1->c_next;
			cp1 = &_board[cbp1->c_vertex].s_fval[us][cbp1->c_dir];
			if (cp->s <= cp1->s)
			    break;
		    } while (cbp1 != _sortframes[us]);
		}
		cbp->c_next = cbp1;
		cbp->c_prev = cbp1->c_prev;
		cbp1->c_prev->c_next = cbp;
		cbp1->c_prev = cbp;
	    }

	  nextf:
	    ;
	}

	// both ends open?
	if (fsp->s_occ == EMPTY) {
	    union comboval* cp = &fsp->s_fval[BLACK][r];
	    if (cp->c.b) {
		cp->c.a += 1;
		cp->c.b = 0;
	    }
	    cp = &fsp->s_fval[WHITE][r];
	    if (cp->c.b) {
		cp->c.a += 1;
		cp->c.b = 0;
	    }
	}

      nextr:
	;
    }
    update_overlap(osp);
    return MOVEOK;
}

// fix up the overlap array due to updating spot osp.
static void update_overlap (const struct spotstr* osp)
{
    const struct spotstr* esp = NULL;
    for (int r = 4; --r >= 0;) {   // for each direction
	int d = _dd[r];
	const struct spotstr* sp1 = osp;
	for (unsigned f = 0; f < 6; ++f, sp1 -= d) {	// for each frame
	    if (sp1->s_occ == BORDER)
		break;
	    if (sp1->s_flg & (BFLAG<<r))
		continue;
	    // Update all other frames that intersect the current one
	    // to indicate whether they still overlap or not.
	    // Since F1 overlap F2 == F2 overlap F1, we only need to
	    // do the rows 0 <= r1 <= r. The r1 == r case is special
	    // since the two frames can overlap at more than one point.
	    unsigned a = sp1->s_frame[r] - _frames;
	    unsigned char* str = &_overlap[a*FAREA];
	    const struct spotstr* sp2 = sp1 - d;
	    for (unsigned i = f + 1; i < 6; ++i, sp2 -= d) {
		if (sp2->s_occ == BORDER)
		    break;
		if (sp2->s_flg & (BFLAG<<r))
		    continue;
		// count the number of empty spots to see if there is still an overlap.
		unsigned n = 0;
		const struct spotstr* sp = sp1;
		for (unsigned b = i - f; b < 5; ++b, sp += d) {
		    if (sp->s_occ == EMPTY) {
			esp = sp;	// save the intersection point
			++n;
		    }
		}
		unsigned b = sp2->s_frame[r] - _frames;
		if (n == 0) {
		    if (sp->s_occ == EMPTY) {
			str[b] &= 0xA;
			_overlap[b * FAREA + a] &= 0xC;
			_intersect[a * FAREA + b] = n = sp - _board;
			_intersect[b * FAREA + a] = n;
		    } else {
			str[b] = 0;
			_overlap[b * FAREA + a] = 0;
		    }
		} else if (n == 1) {
		    if (sp->s_occ == EMPTY) {
			str[b] &= 0xAF;
			_overlap[b * FAREA + a] &= 0xCF;
		    } else {
			str[b] &= 0xF;
			_overlap[b * FAREA + a] &= 0xF;
		    }
		    _intersect[a * FAREA + b] = n = esp - _board;
		    _intersect[b * FAREA + a] = n;
		}
		// else no change, still multiple overlap
	    }

	    // the other directions can only intersect at spot osp
	    for (int r1 = r; --r1 >= 0;) {
		int d1 = _dd[r1];
		const struct spotstr* sp = osp;
		for (int i = 6; --i >= 0; sp -= d1) {	// for each spot
		    if (sp->s_occ == BORDER)
			break;
		    if (sp->s_flg & (BFLAG<<r1))
			continue;
		    unsigned b = sp->s_frame[r1] - _frames;
		    str[b] = 0;
		    _overlap[b * FAREA + a] = 0;
		}
	    }
	}
    }
}
