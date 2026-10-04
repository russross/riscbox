// Copyright (c) 2016 by Mike Sharov <msharov@users.sourceforge.net>
// This file is free software, distributed under the BSD License.
//
// This game was rewritten from scratch, but the gameplay and notably
// the computer player algorithm are derived from code contributed to
// The NetBSD Foundation by Christos Zoulas. So:
// Copyright (c) 2003 The NetBSD Foundation, Inc.

#include "config.h"

//----------------------------------------------------------------------

enum {
    S_COLS	= 80,
    S_LINES	= 24,
    MINLEVEL	= 1,
    MAXLEVEL	= ((S_LINES-1)-1)/2-2,
    MINFW	= MINLEVEL+2,
    MAXFW	= MAXLEVEL+2,
};
enum {
    HUMAN,
    COMPUTER,
    NPLAYERS
};
enum {
    owner_None,
    owner_Human,
    owner_Computer
};
enum {
    color_None,
    color_Human,
    color_Computer
};
struct EdgeCoord {
    uint8_t	l;
    uint8_t	c;
};
enum EDir {
    dir_N,
    dir_E,
    dir_S,
    dir_W,
    NDIRS
};

//----------------------------------------------------------------------

static unsigned	_fw = MINFW;
static uint8_t	_boxes [MAXFW][MAXFW]		= {{owner_None}};
static bool	_edges [MAXFW*2+1][MAXFW+1]	= {{false}};

static struct EdgeCoord _lastMove [NPLAYERS]	= {{0,0}};
static uint8_t	_owned [NPLAYERS]		= {0};

static WINDOW* _wfield = NULL;

//----------------------------------------------------------------------

static void init_level (void);
static void computer_make_move (void);
static bool check_for_closures (unsigned player);
static unsigned have_winner (void);
static unsigned edge_array_height (void);
static unsigned edge_array_width (unsigned l);
static int edge_char (unsigned el, unsigned ec);
static bool* last_move_edge_ptr (unsigned player);
static bool last_move_edge (unsigned player);
static void set_last_move_edge (unsigned player);
static bool* box_edge_ptr (unsigned l, unsigned c, enum EDir d);
static bool box_edge (unsigned l, unsigned c, enum EDir d);
static unsigned box_edge_count (unsigned l, unsigned c);
static void create_field_window (void);
static void draw_edge (unsigned l, unsigned c);
static void draw_field (void);

//----------------------------------------------------------------------

int main (int argc, const char* const* argv)
{
    if (argc == 2) {
	int level = atoi(argv[1]);
	if (level < MINLEVEL || level > MAXLEVEL) {
	    printf ("Error: level must be between %u and %u\n", MINLEVEL, MAXLEVEL);
	    return EXIT_FAILURE;
	}
	_fw = level+2;
    } else if (argc != 1) {
	puts ("Usage: dab [level]");
	return EXIT_SUCCESS;
    }
    initialize_curses();
    static const struct color_pair c_Pairs[] = {
	{ COLOR_WHITE,	COLOR_BLUE	},	// color_Human
	{ COLOR_YELLOW,	COLOR_RED	}	// color_Computer
    };
    init_pairs (ArrayBlock (c_Pairs));
    init_level();

    unsigned player = HUMAN;
    for (int k = 0;;) {
	draw_field();
	if (player == HUMAN) {
	    k = wgetch (_wfield);
	    if (k == 'q' || k == KEY_F(10))
		break;
	    else if (k == KEY_RESIZE)
		create_field_window();
	    else if ((k == 'h' || k == KEY_LEFT) && _lastMove[HUMAN].c)
		--_lastMove[HUMAN].c;
	    else if ((k == 'l' || k == KEY_RIGHT) && _lastMove[HUMAN].c+1u < edge_array_width(_lastMove[HUMAN].l))
		++_lastMove[HUMAN].c;
	    else if ((k == 'k' || k == KEY_UP) && _lastMove[HUMAN].l) {
		--_lastMove[HUMAN].l;
		if (_lastMove[HUMAN].c >= edge_array_width(_lastMove[HUMAN].l))
		    --_lastMove[HUMAN].c;
	    } else if ((k == 'j' || k == KEY_DOWN) && _lastMove[HUMAN].l+1u < edge_array_height()) {
		++_lastMove[HUMAN].l;
		if (_lastMove[HUMAN].c >= edge_array_width(_lastMove[HUMAN].l))
		    --_lastMove[HUMAN].c;
	    }
	} else
	    computer_make_move();

	if (!last_move_edge(player) && (player == COMPUTER || (k == ' ' || k == '\n' || k == KEY_ENTER))) {
	    set_last_move_edge (player);
	    if (!check_for_closures (player))
		player = !player;
	    else if (player == COMPUTER) {
		draw_field();
		usleep (500000);
		flushinp();
	    }
	}
	// Check for win condition
	unsigned winner = have_winner();
	if (winner) {
	    draw_field();
	    flushinp();
	    wgetch (_wfield);
	    if (winner != owner_Human)
		break;
	    if (_fw < MAXFW)
		++_fw;
	    init_level();
	}
    }
    return EXIT_SUCCESS;
}

static void init_level (void)
{
    create_field_window();
    memset (_boxes, 0, sizeof(_boxes));
    memset (_edges, 0, sizeof(_edges));
    memset (_lastMove, 0, sizeof(_lastMove));
    memset (_owned, 0, sizeof(_owned));
}

static bool check_for_closures (unsigned player)
{
    bool madeClosures = false;
    for (unsigned l = 0; l < _fw; ++l) {
	for (unsigned c = 0; c < _fw; ++c) {
	    if (_boxes[l][c] != owner_None || box_edge_count(l, c) != 4)
		continue;
	    // New closure detected. Take ownership.
	    ++_owned[player];
	    _boxes[l][c] = player+1;	// owner_ enum has None=0
	    madeClosures = true;
	}
    }
    return madeClosures;
}

static unsigned have_winner (void)
{
    if (_owned[HUMAN]+_owned[COMPUTER] < _fw*_fw)
	return owner_None;
    return _owned[HUMAN] < _owned[COMPUTER] ? owner_Computer : owner_Human;
}

//----------------------------------------------------------------------
// Edge access

static unsigned edge_array_height (void)
    { return _fw*2+1; }
static unsigned edge_array_width (unsigned l)
    { return _fw+l%2; }
static int edge_char (unsigned el, unsigned ec)
    { return _edges[el][ec] ? (el % 2 ? ACS_VLINE : ACS_HLINE) : ' '; }

//----------------------------------------------------------------------
// Last move edge access

static bool* last_move_edge_ptr (unsigned player)
    { return &_edges[_lastMove[player].l][_lastMove[player].c]; }
static bool last_move_edge (unsigned player)
    { return *last_move_edge_ptr(player); }
static void set_last_move_edge (unsigned player)
    { *last_move_edge_ptr(player) = true; }

//----------------------------------------------------------------------
// Edges accessed through box coordinates

static bool* box_edge_ptr (unsigned l, unsigned c, enum EDir d)
{
    if (d == dir_N)		return &_edges[l*2][c];
    else if (d == dir_W)	return &_edges[l*2+1][c];
    else if (d == dir_E)	return &_edges[l*2+1][c+1];
    else if (d == dir_S)	return &_edges[l*2+2][c];
    return NULL;
}

static bool box_edge (unsigned l, unsigned c, enum EDir d)
{
    bool* pbe = box_edge_ptr (l, c, d);
    return pbe ? *pbe : false;
}

static unsigned box_edge_count (unsigned l, unsigned c)
{
    unsigned r = 0;
    for (unsigned d = dir_N; d <= dir_W; ++d)
	r += box_edge (l, c, d);
    return r;
}

//----------------------------------------------------------------------
// Field drawing

static void create_field_window (void)
{
    if (_wfield)
	delwin (_wfield);
    unsigned w = _fw*2+1;
    _wfield = newwin (w, w, LINES-w-1, (S_COLS-w)/2);
    keypad (_wfield, true);
}

static void draw_edge (unsigned l, unsigned c)
{
    mvwaddch (_wfield, l, c*2+!(l%2), edge_char(l,c));
}

static void draw_field (void)
{
    erase();
    werase (_wfield);
    // Draw grid
    wattr_set (_wfield, A_NORMAL, color_None, NULL);
    box (_wfield, 0, 0);				// corners
    for (unsigned c = 1; c < _fw; ++c) {
	mvwaddch (_wfield, 0, 2*c, ACS_TTEE);		// top line tees
	mvwaddch (_wfield, getmaxy(_wfield)-1, 2*c, ACS_BTEE);	// bottom line tees
    }
    for (unsigned l = 1; l < _fw; ++l) {
	mvwaddch (_wfield, 2*l, 0, ACS_LTEE);		// left column tees
	mvwaddch (_wfield, 2*l, getmaxx(_wfield)-1, ACS_RTEE);	// right column tees
	for (unsigned c = 1; c < _fw; ++c)
	    mvwaddch (_wfield, 2*l, 2*c, ACS_PLUS);	// and crosses in the middle
    }
    // Draw the box edge states
    for (unsigned l = 0; l < edge_array_height(); ++l)
	for (unsigned c = 0; c < edge_array_width(l); ++c)
	    draw_edge (l, c);
    // Draw the box ownership letters
    for (unsigned l = 0; l < _fw; ++l) {
	for (unsigned c = 0; c < _fw; ++c) {
	    wcolor_set (_wfield, _boxes[l][c], NULL);
	    mvwaddch (_wfield, l*2+1, c*2+1, " hc"[_boxes[l][c]]);
	}
    }
    // Draw last moves
    wcolor_set (_wfield, color_Computer, NULL);
    draw_edge (_lastMove[COMPUTER].l, _lastMove[COMPUTER].c);
    wcolor_set (_wfield, color_Human, NULL);
    draw_edge (_lastMove[HUMAN].l, _lastMove[HUMAN].c);
    // Draw stats
    unsigned winner = have_winner();
    if (!winner)
	mvprintw (LINES-1, S_COLS/2-15, "Level %u: human %u, computer %u", _fw-2, _owned[HUMAN], _owned[COMPUTER]);
    else {
	static const char c_Name[NPLAYERS][4] = {"You","I"};
	unsigned player = winner-1;
	mvprintw (LINES-1, S_COLS/2-10, "Level %u: %s won %u-%u!", _fw-2, c_Name[player], _owned[player], _owned[!player]);
    }
    wnoutrefresh (stdscr);
    wrefresh (_wfield);
}

//----------------------------------------------------------------------
// Computer AI

struct EdgeScore {
    struct EdgeCoord	loc;
    unsigned short	score;	// = number of boxes won
};
enum {
    EDGE_ALREADY_SET,
    LOSE_TWO_BOXES,
    LOSE_ONE_BOX,
    AVERAGE_MOVE,
    WIN_ONE_BOX,
    WIN_TWO_BOXES
};

static int compare_edge_score (const void* v1, const void* v2)
{
    const struct EdgeScore* sc1 = (const struct EdgeScore*) v1;
    const struct EdgeScore* sc2 = (const struct EdgeScore*) v2;
    return sign (sc2->score - sc1->score);	// in descending order
}

static short compute_edge_score (unsigned el, unsigned ec)
{
    if (_edges[el][ec])
	return EDGE_ALREADY_SET;

    // Can be either a horizontal or vertical edge
    // It will be part of two boxes, unless at the edge of the board
    unsigned box1Edges = 0, box2Edges = 0;
    if (el%2) {	// vertical edge
	if (ec) {
	    box1Edges += _edges[el-1][ec-1];
	    box1Edges += _edges[el  ][ec-1];
	    box1Edges += _edges[el+1][ec-1];
	}
	if (ec+1 < edge_array_width(el)) {
	    box2Edges += _edges[el-1][ec];
	    box2Edges += _edges[el  ][ec+1];
	    box2Edges += _edges[el+1][ec];
	}
    } else {	// horizontal edge
	if (el) {
	    box1Edges += _edges[el-2][ec];
	    box1Edges += _edges[el-1][ec];
	    box1Edges += _edges[el-1][ec+1];
	}
	if (el+2 < edge_array_height()) {
	    box2Edges += _edges[el+1][ec];
	    box2Edges += _edges[el+1][ec+1];
	    box2Edges += _edges[el+2][ec];
	}
    }
    if (box1Edges == 3 && box2Edges == 3)
	return WIN_TWO_BOXES;
    else if (box1Edges == 2 && box2Edges == 2)
	return LOSE_TWO_BOXES;
    else if (box1Edges == 3 || box2Edges == 3)
	return WIN_ONE_BOX;
    else if (box1Edges == 2 || box2Edges == 2)
	return LOSE_ONE_BOX;
    return AVERAGE_MOVE;
}

static void computer_make_move (void)
{
    unsigned nEdges = edge_array_height()*edge_array_width(1);
    struct EdgeScore esc [nEdges];
    memset (esc, 0, sizeof(esc[0])*nEdges);

    // First compute the scores for all edges
    for (unsigned l = 0, e = 0; l < edge_array_height(); ++l) {
	for (unsigned c = 0; c < edge_array_width(l); ++c, ++e) {
	    struct EdgeScore* sc = &esc[e];
	    sc->loc.l = l;
	    sc->loc.c = c;
	    sc->score = compute_edge_score (l, c);
	}
    }
    qsort (esc, nEdges, sizeof(esc[0]), compare_edge_score);

    // Find last playable edge
    for (; nEdges; --nEdges)
	if (esc[nEdges-1].score > EDGE_ALREADY_SET)
	    break;

    // Count the edges with the highest score
    unsigned nChoices = 1;
    while (nChoices < nEdges && esc[nChoices].score == esc[0].score)
	++nChoices;

    // And choose a random edge in the resultant set
    _lastMove[COMPUTER] = esc[nrand(nChoices)].loc;
}
