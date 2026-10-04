// Copyright (c) 1994 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "gomoku.h"
#include <signal.h>

//----------------------------------------------------------------------

struct spotstr	_board[BAREA];		// info for board
struct combostr	_frames[FAREA];		// storage for all frames
struct combostr* _sortframes[2];	// sorted list of non-empty frames
unsigned char	_overlap[FAREA*FAREA];	// true if frame [a][b] overlap
short		_intersect[FAREA*FAREA];// frame [a][b] intersection
unsigned	_movenum = 0;		// current move number
const char*	_plyr[2] = { NULL };	// who's who
unsigned	_humanPlayer = 0;	// which player is human
unsigned	_lastHumanMove = BAREA/2;
unsigned	_lastComputerMove = 0;

// direction deltas
const int _dd[4] = { MRIGHT, MRIGHT + MDOWN, MDOWN, MDOWN + MLEFT };

//----------------------------------------------------------------------

static void init_overlap (void);
static void bdinit (struct spotstr *bp);

//----------------------------------------------------------------------

int main (void)
{
    initialize_curses();
    initialize_field_window();
    bdinit (_board);

    // Randomly choose who goes first
    enum {
	USER,	// get input from standard input
	PROGRAM	// get input from program
    };
    unsigned input[2];
    _humanPlayer = nrand(2);
    input[_humanPlayer] = USER;
    input[!_humanPlayer] = PROGRAM;
    _plyr[_humanPlayer] = "you";
    _plyr[!_humanPlayer] = "me";

    for (unsigned color = BLACK;; color = !color) {
	bdisp();
	unsigned curmove;
	if (input[color] == USER)
	    curmove = _lastHumanMove = usermove();
	else
	    curmove = _lastComputerMove = pickmove (color);
	int mv = makemove(color, curmove);
	if (mv != MOVEOK) {	// Game finished. Display result and quit.
	    if (mv != RESIGN)
		display_game_result_message (mv, input[color] == USER);
	    break;
	}
    }
    return EXIT_SUCCESS;
}

//----------------------------------------------------------------------

static void bdinit (struct spotstr *bp)
{
    _movenum = 1;

    // mark the borders as such
    struct spotstr* sp = bp;
    for (int i = BSZ2; --i >= 0; ++sp) {
	sp->s_occ = BORDER;    // top border
	sp->s_flg = BFLAGALL;
    }

    // fill entire board with EMPTY spots
    memset(_frames, 0, sizeof(_frames));
    struct combostr* cbp = _frames;
    for (unsigned j = 0; ++j < BSZ1; ++sp) {		// for each row
	for (unsigned i = 0; ++i < BSZ1; ++sp) {	// for each column
	    sp->s_occ = EMPTY;
	    sp->s_flg = 0;
	    sp->s_wval = 0;
	    if (j < 5) {		// directions 1, 2, 3 are blocked
		sp->s_flg |= (BFLAG << 1) | (BFLAG << 2) | (BFLAG << 3);
		sp->s_fval[BLACK][1].s = MAXCOMBO;
		sp->s_fval[BLACK][2].s = MAXCOMBO;
		sp->s_fval[BLACK][3].s = MAXCOMBO;
		sp->s_fval[WHITE][1].s = MAXCOMBO;
		sp->s_fval[WHITE][2].s = MAXCOMBO;
		sp->s_fval[WHITE][3].s = MAXCOMBO;
	    } else if (j == 5) {	// five spaces, blocked on one side
		sp->s_fval[BLACK][1].s = 0x500;
		sp->s_fval[BLACK][2].s = 0x500;
		sp->s_fval[BLACK][3].s = 0x500;
		sp->s_fval[WHITE][1].s = 0x500;
		sp->s_fval[WHITE][2].s = 0x500;
		sp->s_fval[WHITE][3].s = 0x500;
	    } else {			// six spaces, not blocked
		sp->s_fval[BLACK][1].s = 0x401;
		sp->s_fval[BLACK][2].s = 0x401;
		sp->s_fval[BLACK][3].s = 0x401;
		sp->s_fval[WHITE][1].s = 0x401;
		sp->s_fval[WHITE][2].s = 0x401;
		sp->s_fval[WHITE][3].s = 0x401;
	    }
	    if (i > BSZ-4) {		// directions 0, 1 are blocked
		sp->s_flg |= BFLAG | (BFLAG << 1);
		sp->s_fval[BLACK][0].s = MAXCOMBO;
		sp->s_fval[BLACK][1].s = MAXCOMBO;
		sp->s_fval[WHITE][0].s = MAXCOMBO;
		sp->s_fval[WHITE][1].s = MAXCOMBO;
	    } else if (i == BSZ-4) {	// if direction 1 is not blocked
		sp->s_fval[BLACK][0].s = 0x500;
		sp->s_fval[WHITE][0].s = 0x500;
		if (!(sp->s_flg & (BFLAG << 1))) {
		    sp->s_fval[BLACK][1].s = 0x500;
		    sp->s_fval[WHITE][1].s = 0x500;
		}
	    } else {			// direction 3 is blocked
		sp->s_fval[BLACK][0].s = 0x401;
		sp->s_fval[WHITE][0].s = 0x401;
		if (i < 5) {
		    sp->s_flg |= BFLAG << 3;
		    sp->s_fval[BLACK][3].s = MAXCOMBO;
		    sp->s_fval[WHITE][3].s = MAXCOMBO;
		} else if (i == 5 && !(sp->s_flg & (BFLAG << 3))) {
		    sp->s_fval[BLACK][3].s = 0x500;
		    sp->s_fval[WHITE][3].s = 0x500;
		}
	    }
	    // Allocate a frame structure for non blocked frames.
	    for (int r = 4; --r >= 0;) {
		if (sp->s_flg & (BFLAG << r))
		    continue;
		cbp->c_combo.s = sp->s_fval[BLACK][r].s;
		cbp->c_vertex = sp - _board;
		cbp->c_nframes = 1;
		cbp->c_dir = r;
		sp->s_frame[r] = cbp;
		++cbp;
	    }
	}
	sp->s_occ = BORDER;    // left & right border
	sp->s_flg = BFLAGALL;
    }

    // mark the borders as such
    for (int i = BSZ1; --i >= 0; ++sp) {
	sp->s_occ = BORDER;    // bottom border
	sp->s_flg = BFLAGALL;
    }

    _sortframes[BLACK] = NULL;
    _sortframes[WHITE] = NULL;
    init_overlap();
}

// Initialize the overlap array.
// Each entry in the array is a bit mask with eight bits corresponding
// to whether frame B overlaps frame A (as indexed by overlap[A * FAREA + B]).
// The eight bits coorespond to whether A and B are open ended (length 6) or
// closed (length 5).
//      0       A closed and B closed
//      1       A closed and B open
//      2       A open and B closed
//      3       A open and B open
//      4       A closed and B closed and overlaps in more than one spot
//      5       A closed and B open and overlaps in more than one spot
//      6       A open and B closed and overlaps in more than one spot
//      7       A open and B open and overlaps in more than one spot
// As pieces are played, it can make frames not overlap if there are no
// common open spaces shared between the two frames.
//
static void init_overlap (void)
{
    memset(_overlap, 0, sizeof(_overlap));
    memset(_intersect, 0, sizeof(_intersect));
    unsigned char* str = &_overlap[FAREA * FAREA];
    short* ip = &_intersect[FAREA * FAREA];
    for (struct combostr* cbp = _frames + FAREA; --cbp >= _frames;) {	// each frame
	str -= FAREA;
	ip -= FAREA;
	unsigned vertex = cbp->c_vertex;
	struct spotstr* sp1 = &_board[vertex];
	unsigned fdir = cbp->c_dir;
	int d1 = _dd[fdir];
	// s = 5 if closed, 6 if open.
	// At this point black & white are the same.
	unsigned s = 5 + sp1->s_fval[BLACK][fdir].c.b;
	// for each spot in frame A
	for (unsigned i = 0; i < s; ++i, sp1 += d1, vertex += d1) {
	    // the sixth spot in frame A only overlaps if it is open
	    unsigned short mask = (i == 5) ? 0xC : 0xF;
	    // for each direction
	    for (int r = 4; --r >= 0;) {
		unsigned bmask = BFLAG << r;
		struct spotstr* sp2 = sp1;
		int d2 = _dd[r];
		// for each frame that intersects at spot sp1
		for (unsigned f = 0; f < 6; ++f, sp2 -= d2) {
		    if (sp2->s_occ == BORDER)
			break;
		    if (sp2->s_flg & bmask)
			continue;
		    unsigned n = sp2->s_frame[r] - _frames;
		    ip[n] = vertex;
		    str[n] |= (f == 5) ? mask & 0xA : mask;
		    if (r == cbp->c_dir) {
			// compute the multiple spot overlap values
			if (i == 0) {		// sp1 is the first spot in A
			    if (f == 4)
				str[n] |= 0xA0;
			    else if (f != 5)
				str[n] |= 0xF0;
			} else if (i == 1) {	// sp1 is the second spot in A
			    if (f == 5)
				str[n] |= 0xA0;
			    else
				str[n] |= 0xF0;
			} else if (i == 4) {	// sp1 is the penultimate spot in A
			    if (f == 0)
				str[n] |= 0xC0;
			    else
				str[n] |= 0xF0;
			} else if (i == 5) {	// sp1 is the last spot in A
			    if (f == 1)
				str[n] |= 0xC0;
			    else if (f != 0)
				str[n] |= 0xF0;
			} else
			    str[n] |= 0xF0;
		    }
		}
	    }
	}
    }
}
