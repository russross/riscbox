// Copyright (c) 1994 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "config.h"
#include <endian.h>

enum {
    BSZ		= 19,	// board dimensions
    BSZ1,
    BSZ2,
    BAREA	= (BSZ2*BSZ1+1),

    FSZ1	= BSZ,	// frame dimentions (based on 5 in a row)
    FSZ2	= (BSZ-4),
    FAREA	= (FSZ1*FSZ2 + FSZ2*FSZ2 + FSZ1*FSZ2 + FSZ2*FSZ2),

    MUP		= (BSZ1),
    MDOWN	= (-BSZ1),
    MLEFT	= (-1),
    MRIGHT	= (1)
};

// values for s_occ
enum {
    BLACK,
    WHITE,
    EMPTY,
    BORDER	// borders are used as sentinels
};

// return values for makemove()
enum {
    MOVEOK,
    RESIGN,
    ILLEGAL,
    WIN,
    TIE
};

// A 'frame' is a group of five or six contiguous board locations.
// An open ended frame is one with spaces on both ends; otherwise, its closed.
// A 'combo' is a group of intersecting frames and consists of two numbers:
// 'A' is the number of moves to make the combo non-blockable.
// 'B' is the minimum number of moves needed to win once it can't be blocked.
// A 'force' is a combo that is one move away from being non-blockable
//
// Single frame combo values:
//     <A,B>    board values
//      5,0     . . . . . O
//      4,1     . . . . . .
//      4,0     . . . . X O
//      3,1     . . . . X .
//      3,0     . . . X X O
//      2,1     . . . X X .
//      2,0     . . X X X O
//      1,1     . . X X X .
//      1,0     . X X X X O
//      0,1     . X X X X .
//      0,0     X X X X X O
//
// The rule for combining two combos (<A1,B1> <A2,B2>)
// with V valid intersection points, is:
//      A' = A1 + A2 - 2 - V
//      B' = MIN(A1 + B1 - 1, A2 + B2 - 1)
// Each time a frame is added to the combo, the number of moves to complete
// the force is the number of moves needed to 'fill' the frame plus one at
// the intersection point. The number of moves to win is the number of moves
// to complete the best frame minus the last move to complete the force.
// Note that it doesn't make sense to combine a <1,x> with anything since
// it is already a force. Also, the frames have to be independent so a
// single move doesn't affect more than one frame making up the combo.
//
// Rules for comparing which of two combos (<A1,B1> <A2,B2>) is better:
// Both the same color:
//      <A',B'> = (A1 < A2 || A1 == A2 && B1 <= B2) ? <A1,B1> : <A2,B2>
//      We want to complete the force first, then the combo with the
//      fewest moves to win.
// Different colors, <A1,B1> is the combo for the player with the next move:
//      <A',B'> = A2 <= 1 && (A1 > 1 || A2 + B2 < A1 + B1) ? <A2,B2> : <A1,B1>
//      We want to block only if we have to (i.e., if they are one move away
//      from completing a force and we don't have a force that we can
//      complete which takes fewer or the same number of moves to win).

enum {
    MAXA	= 6,
    MAXB	= 2,
    MAXCOMBO	= 0x600
};

union comboval {
    struct {
#if BYTE_ORDER == BIG_ENDIAN
	unsigned char a;		// # moves to complete force
	unsigned char b;		// # moves to win
#elif BYTE_ORDER == LITTLE_ENDIAN
	unsigned char b;		// # moves to win
	unsigned char a;		// # moves to complete force
#endif
    } c;
    unsigned short s;
};

// This structure is used to record information about single frames (F) and
// combinations of two more frames (C).
// For combinations of two or more frames, there is an additional
// array of pointers to the frames of the combination which is sorted
// by the index into the frames[] array. This is used to prevent duplication
// since frame A combined with B is the same as B with A.
//      struct combostr *c_sort[size c_nframes];
// The leaves of the tree (frames) are numbered 0 (bottom, leftmost)
// to c_nframes - 1 (top, right). This is stored in c_frameindex and
// c_dir if C_LOOP is set.
struct combostr {
    struct combostr *c_next;	// list of combos at the same level
    struct combostr *c_prev;	// list of combos at the same level
    struct combostr *c_link[2];	// C:previous level or F:NULL
    union comboval c_linkv[2];	// C:combo value for link[0,1]
    union comboval c_combo;	// C:combo value for this level
    unsigned short c_vertex;	// C:intersection or F:frame head
    unsigned char c_nframes;	// number of frames in the combo
    unsigned char c_dir;	// C:loop frame or F:frame direction
    unsigned char c_flg;	// C:combo flags
    unsigned char c_frameindex;	// C:intersection frame index
    unsigned char c_framecnt[2];// number of frames left to attach
    unsigned char c_emask[2];	// C:bit mask of completion spots for link[0] and link[1]
    unsigned char c_voff[2];	// C:vertex offset within frame
};

// flag values for c_flg
enum {
    C_OPEN_0	= (1<<0),	// link[0] is an open ended frame
    C_OPEN_1	= (1<<1),	// link[1] is an open ended frame
    C_LOOP	= (1<<2),	// link[1] intersects previous frame
    C_MARK	= (1<<3)	// indicates combo processed
};

// This structure is used for recording the completion points of
// multi frame combos.
struct elist {
    struct elist*	e_next;		// list of completion points
    struct combostr*	e_combo;	// the whole combo
    unsigned char	e_off;		// offset in frame of this empty spot
    unsigned char	e_frameindex;	// intersection frame index
    unsigned char	e_framecnt;	// number of frames left to attach
    unsigned char	e_emask;	// real value of the frame's emask
    union comboval	e_fval;		// frame combo value
};

// One spot structure for each location on the board.
// A frame consists of the combination for the current spot plus the five spots
// 0: right, 1: right & down, 2: down, 3: down & left.
struct spotstr {
    unsigned short	s_occ;		// color of occupant
    short		s_wval;		// weighted value
    unsigned		s_flg;		// flags for graph walks
    struct combostr*	s_frame[4];	// level 1 combo for frame[dir]
    union comboval	s_fval[2][4];	// combo value for [color][frame]
    union comboval	s_combo[2];	// minimum combo value for BLK & WHT
    unsigned char	s_level[2];	// number of frames in the min combo
    unsigned char	s_nforce[2];	// number of <1,x> combos
    struct elist*	s_empty;	// level n combo completion spots
    struct elist*	s_nempty;	// level n+1 combo completion spots
    unsigned		dummy[2];
};

// flag values for s_flg
enum {
    CFLAG	= 0x000001,	// frame is part of a combo
    CFLAGALL	= 0x00000F,	// all frame directions marked
    IFLAG	= 0x000010,	// legal intersection point
    IFLAGALL	= 0x0000F0,	// any intersection points?
    FFLAG	= 0x000100,	// frame is part of a <1,x> combo
    FFLAGALL	= 0x000F00,	// all force frames
    MFLAG	= 0x001000,	// frame has already been seen
    MFLAGALL	= 0x00F000,	// all frames seen
    BFLAG	= 0x010000,	// frame intersects border or dead
    BFLAGALL	= 0x0F0000	// all frames dead
};

// This structure is used to store overlap information between frames.
struct ovlp_info {
    unsigned		o_intersect;	// intersection spot
    struct combostr*	o_fcombo;	// the connecting combo
    unsigned char	o_link;		// which link to update (0 or 1)
    unsigned char	o_off;		// offset in frame of intersection
    unsigned char	o_frameindex;	// intersection frame index
};

//----------------------------------------------------------------------

extern const int	_dd[4];
extern struct spotstr	_board[BAREA];		// info for board
extern struct combostr	_frames[FAREA];		// storage for single frames
extern struct combostr*	_sortframes[2];		// sorted, non-empty frames
extern unsigned char	_overlap[FAREA*FAREA];	// frame [a][b] overlap
extern short		_intersect[FAREA*FAREA];// frame [a][b] intersection
extern unsigned		_movenum;
extern unsigned		_lastHumanMove;
extern unsigned		_lastComputerMove;
extern unsigned		_humanPlayer;		// which player is human

//----------------------------------------------------------------------

void initialize_field_window (void);
void bdisp (void);
void display_game_result_message (unsigned i, bool humanPlayer);
int usermove (void);
int pickmove (int);
int makemove (int, int);

//----------------------------------------------------------------------
