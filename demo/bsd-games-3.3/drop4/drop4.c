// Copyright (c) 1992 The Regents of the University of California.
// This file is free software, distributed under the BSD license.
//
// Drop4
//
// Score code for Drop4, by Darren Provine (kilroy@gboro.glassboro.edu)
// modified 22 January 1992, to limit the number of entries any one
// person has. This feature was removed in 2016.
//
// Renamed to drop4 in 2016 to differentiate from tetris.
//
// Excerpt from the ruling in Tetris Holding, LLC vs. Xio Interactive, Inc.:
//
//    Tetris is a puzzle game where a user manipulates pieces composed of
//    square blocks, each made into a different geometric shape, that fall from
//    the top of the game board to the bottom where the pieces accumulate. The
//    user is given a new piece after the current one reaches the bottom of the
//    available game space. While a piece is falling, the user rotates it in
//    order to fit it in with the accumulated pieces. The object of the puzzle
//    is to fill all spaces along a horizontal line. If that is accomplished,
//    the line is erased, points are earned, and more of the game board is
//    available for play. But if the pieces accumulate and reach the top of
//    the screen, then the game is over. These then are the general, abstract
//    ideas underlying tetris and cannot be protected by copyright nor can
//    expressive elements that are inseparable from them.
//
//    The court noted that “if one has to squint to find distinctions
//    only at a granular level, then the works are likely to be substantially
//    similar.” Further, the court pointed to numerous elements of the games
//    that were difficult to distinguish, such as the shape, color, and look of
//    game bricks, the pieces formed from the bricks, the way the game pieces
//    could be fitted together in a complete line, the pieces’ movement and
//    rotation, the precise size of the playing field, the behavior of the
//    display upon certain events
//
// Every precaution must be taken to avoid the above elements. Specific
// differences from tetris in this implementation are:
//
// - the game has a distinctive name
// - the pieces are not formed with distinct bricks, but are solid.
// - all pieces are drawn in the same color
// - pieces are drawn with spaces instead of # used in the original tetris
// - there is no color variations within each piece
// - the size of the board is different
// - row removal is not animated (behavior of display mentioned above)
// - pieces do not pause while being rotated (another behavior)
// - score and next piece display are to the right of the board
//
// The shapes themselves are an exhaustive enumeration of tetrominos with
// mirror images being distinct. Just as other polyomino sets, the number
// of shapes and their shape in the full set for each block count is
// mathematically unique and therefore not copyrightable.
//
// The rest of the game merely implements the game rules as described in
// the first cited paragraph of the ruling and is non-infringing.

#include "config.h"

//----------------------------------------------------------------------
// Definitions for Drop4.

// The display (board) is composed of 23 rows of 10 columns of characters.
// Columns 1 to 10 of rows 1 to 20 are the actual playing area, where
// shapes appear.
enum {
    NSHAPES	= 7,
    S_COLS	= 80,
    S_ROWS	= 24,
    B_COLS	= 12,
    B_ROWS	= S_ROWS-1,
    B_STARTROW	= 1,
    B_STARTCOL	= B_COLS/2,
    BOARD_COL	= (S_COLS-B_COLS*2)/2,
    SCORE_ROWS	= 8,
    SCORE_COLS	= SCORE_ROWS*2,
    SCORE_COL	= S_COLS-SCORE_COLS-1,
    MINLEVEL	= 1,	// Game level must be between 1 and 9.
    MAXLEVEL	= 9,	//	This controls the initial fall rate and affects scoring.
    ROWS_PER_LEVEL= 50	// Bump level every N rows removed
};

// A shape is the fundamental thing that makes up the game.  There
// are 7 basic shapes, comprising all possible combinations of 4 blocks.
//
//      X.X       X.X           X.X
//        X.X   X.X     X.X.X   X.X     X.X.X   X.X.X   X.X.X.X
//                        X             X           X
//
//        0       1       2       3       4       5       6
//
// Except for 3 and 6, the center of each shape is one of the blocks.
// This block is designated (0,0).  The other three blocks can then be
// described as offsets from the center.  Shape 3 is the same under
// rotation, so its center is effectively irrelevant; it has been chosen
// so that it `sticks out' upward and leftward.  Except for shape 6,
// all the blocks are contained in a box going from (-1,-1) to (+1,+1);
// shape 6's center `wobbles' as it rotates, so that while it `sticks out'
// rightward, its rotation---a vertical line---`sticks out' downward.
// The containment box has to include the offset (2,0), making the overall
// containment box range from offset (-1,-1) to (+2,+1).  (This is why
// there is only one row above, but two rows below, the display area.)
//
// The game works by choosing one of these shapes at random and putting
// its center at the middle of the first display row (row 1, column 5).
// The shape is moved steadily downward until it collides with something:
// either  another shape, or the bottom of the board.  When the shape can
// no longer be moved downwards, it is merged into the current board.
// At this time, any completely filled rows are elided, and blocks above
// these rows move down to make more room.  A new random shape is again
// introduced at the top of the board, and the whole process repeats.
// The game ends when the new shape will not fit at (1,5).
//
// While the shapes are falling, the user can rotate them counterclockwise
// 90 degrees (in addition to moving them left or right), provided that the
// rotation puts the blocks in empty spaces.  The table of shapes is set up
// so that each shape contains the index of the new shape obtained by
// rotating the current shape.  Due to symmetry, each shape has exactly
// 1, 2, or 4 rotations total; the first 7 entries in the table represent
// the primary shapes, and the remaining 12 represent their various
// rotated forms.

struct shape {
    uint8_t rot;	// index of rotated version of this shape
    struct {
	int8_t	dy:4;
	int8_t	dx:4;
    } off[3];		// offsets to other blocks if center is at (0,0)
};

enum {
    color_None,
    color_Default,
    color_Background,
    color_Text,
    color_HelpText,
    color_Borders,
    color_Shapes,
    color_Last
};

#define DROP4_SCOREFILE	_PATH_GAME_STATE "drop4.scores"
#define SCOREFILE_MAGIC	"drop4\0"

struct highscore {
    char	name [16];	// login name
    uint32_t	score;		// raw score
    uint32_t	level;		// play level
    time_t	time;		// time at game end
};

enum {
    MAXHISCORES	= 20,
    EXPIRATION	= 7 * 365*24*60*60	// 7 years
};

//----------------------------------------------------------------------
// Local variables

static uint8_t _board [B_ROWS][B_COLS] = {{0}};	// 1 => occupied, 0 => empty

// Scoring is as follows:
//
// When the shape comes to rest, and is integrated into the board,
// we score one point.  If the shape is high up (at a low-numbered row),
// and the user hits the space bar, the shape plummets all the way down,
// and we score a point for each row it falls (plus one more as soon as
// we find that it is at rest and integrate it---until then, it can
// still be moved or rotated).
static unsigned _score = 0;
struct highscore _scores [MAXHISCORES] = {{"",0,0,0}};

static bool _paused = false;
static unsigned _movedelay = 500;	// Delay between movements in us
static uint64_t _nextmove = 0;

static const struct shape* _curshape = NULL;
static const struct shape* _nextshape = NULL;

static WINDOW* _win = NULL;

//----------------------------------------------------------------------
// Local prototypes

static unsigned get_level (void);
static void set_level (unsigned l);

static void gameloop (void);
static void remove_full_rows(void);

static const struct shape* rotateshape (void);
static const struct shape* randshape (void);
static void place (const struct shape *shape, unsigned y, unsigned x, bool onoff);
static bool fits_in (const struct shape* shape, unsigned y, unsigned x);

static void scr_create_window (void);
static void scr_update (void);
static void showscores (unsigned level);
static int cmpscores (const void *x, const void *y);
static void check_scores (void);
static void savescore (unsigned score, unsigned level);

//----------------------------------------------------------------------
// main

int main (int argc, const char* const* argv)
{
    if (argc > 2) {
	printf ("Usage: drop4 [level]\n");
	return EXIT_SUCCESS;
    } else if (argc == 2) {
	int level = atoi(argv[1]);
	if (level < MINLEVEL || level > MAXLEVEL) {
	    printf ("Level must be from %u to %u\n", MINLEVEL, MAXLEVEL);
	    return EXIT_FAILURE;
	}
	set_level (level);
    }

    initialize_curses();
    static const struct color_pair c_Pairs[] = {
	{ COLOR_DEFAULT,COLOR_DEFAULT	},	// color_Default
	{ COLOR_DEFAULT,COLOR_DEFAULT	},	// color_Background
	{ COLOR_CYAN,	COLOR_BLACK	},	// color_Text
	{ COLOR_BLACK,	COLOR_DEFAULT	},	// color_HelpText
	{ COLOR_MAGENTA,COLOR_BLACK	},	// color_Borders
	{ COLOR_CYAN,	COLOR_BLACK	}	// color_Shapes
    };
    init_pairs (ArrayBlock (c_Pairs));
    scr_create_window();
    nodelay (_win, true);
    keypad (_win, true);

    gameloop();
    savescore (_score, get_level());
    showscores (get_level());
    return EXIT_SUCCESS;
}

static unsigned get_level (void)
{
    return 1000/_movedelay;
}

static void set_level (unsigned l)
{
    _movedelay = 1000/l;
}

//----------------------------------------------------------------------
// Game loop

static void gameloop (void)
{
    _nextshape = randshape();
    _curshape = randshape();

    for (int y = B_STARTROW, x = B_STARTCOL;;) {
	place (_curshape, y, x, 1);
	scr_update();
	place (_curshape, y, x, 0);
	uint64_t now = time_ms();
	wtimeout (_win, now >= _nextmove ? 0 : _nextmove - now);
	int c = wgetch (_win);
	if (c == 'q' || c == KEY_F(10) || c == KEY_END)	// quit
	    break;
	else if (c == KEY_RESIZE)			// screen resized, recreate window
	    scr_create_window();
	else if (c == '>' && get_level() < MAXLEVEL)	// go faster
	    set_level (get_level()+1);
	else if ((c == 'j' || c == KEY_LEFT) && fits_in(_curshape, y, x-1))	// move left
	    --x;
	else if ((c == 'l' || c == KEY_RIGHT) && fits_in(_curshape, y, x+1))	// move right
	    ++x;
	else if (c == ' ' || c == KEY_DOWN) {		// move to bottom
	    while (fits_in (_curshape, y+1, x))
		++y, ++_score;
	    c = ERR;	// End turn
	} else if (c == 'k' || c == KEY_UP || c == KEY_B2) {	// turn
	    const struct shape* rotshape = rotateshape();
	    if (fits_in (rotshape, y, x))
		_curshape = rotshape;
	} else if (c == 'p' || c == KEY_BACKSPACE) {	// pause
	    _paused = true;
	    scr_update();
	    wtimeout (_win, -1);
	    if (KEY_RESIZE == wgetch (_win))
		scr_create_window();			// handle window resize while paused
	    _paused = false;
	}
	if (c == ERR) {	// Timeout.  Move down if possible.
	    _nextmove = time_ms() + _movedelay;
	    if (fits_in (_curshape, y+1, x))
		++y;
	    else {		// Current shape has reached the bottom
		place (_curshape, y, x, 1);
		++_score;
		remove_full_rows();

		// Choose a new shape.  If it does not fit, the game is over.
		_curshape = _nextshape;
		_nextshape = randshape();
		if (!fits_in(_curshape, y = B_STARTROW, x = B_STARTCOL))
		    break;
	    }
	}
    }
}

static void remove_full_rows (void)
{
    unsigned firstNonempty = B_ROWS;
    for (unsigned r = 0; r < B_ROWS; ++r) {
	bool rowEmpty = true, rowFull = true;
	for (unsigned c = 0; c < B_COLS; ++c) {
	    if (_board[r][c])
		rowEmpty = false;
	    else
		rowFull = false;
	}
	if (!rowEmpty && firstNonempty > r)
	    firstNonempty = r;
	if (rowFull && r > firstNonempty) {
	    memset (_board[r], 0, B_COLS);
	    memmove (_board[firstNonempty+1], _board[firstNonempty], (r-firstNonempty)*sizeof(_board[0]));
	    memset (_board[firstNonempty], 0, B_COLS);
	    ++firstNonempty;
	    // Bump level every N rows
	    static unsigned s_RowsRemoved = 0;
	    unsigned level = get_level();
	    if (level < MAXLEVEL && ++s_RowsRemoved >= ROWS_PER_LEVEL) {
		s_RowsRemoved = 0;
		set_level (get_level()+1);
	    }
	}
    }
}

//----------------------------------------------------------------------
// Shapes and operations on them

static const struct shape shapes[] = {
    { 7, {{-1,-1}, {-1, 0}, {0, 1}}},
    { 8, {{-1, 0}, {-1, 1}, {0,-1}}},
    { 9, {{ 0,-1}, { 0, 1}, {1, 0}}},
    { 3, {{-1,-1}, {-1, 0}, {0,-1}}},
    {12, {{ 0,-1}, { 1,-1}, {0, 1}}},
    {15, {{ 0,-1}, { 1, 1}, {0, 1}}},
    {18, {{ 0,-1}, { 0, 1}, {0, 2}}},
    { 0, {{-1, 0}, { 0,-1}, {1,-1}}},
    { 1, {{-1, 0}, { 0, 1}, {1, 1}}},
    {10, {{-1, 0}, { 0, 1}, {1, 0}}},
    {11, {{-1, 0}, { 0,-1}, {0, 1}}},
    { 2, {{-1, 0}, { 0,-1}, {1, 0}}},
    {13, {{-1, 0}, { 1, 0}, {1, 1}}},
    {14, {{-1, 1}, { 0,-1}, {0, 1}}},
    { 4, {{-1,-1}, {-1, 0}, {1, 0}}},
    {16, {{-1, 1}, {-1, 0}, {1, 0}}},
    {17, {{-1,-1}, { 0, 1}, {0,-1}}},
    { 5, {{-1, 0}, { 1, 0}, {1,-1}}},
    { 6, {{-1, 0}, { 1, 0}, {2, 0}}}
};

//----------------------------------------------------------------------

static inline const struct shape* rotateshape (void)
    { return &shapes[_curshape->rot]; }
static inline const struct shape* randshape (void)
    { return &shapes[nrand(NSHAPES)]; }

// Return true iff the given shape fits in the given position,
// taking the current board into account.
static bool fits_in (const struct shape* shape, unsigned y, unsigned x)
{
    unsigned py = y, px = x, ip = 0;
    for (;;) {
	if (py >= B_ROWS || px >= B_COLS || _board[py][px])
	    return false;
	if (ip < ArraySize(shape->off)) {
	    py = y + shape->off[ip].dy;
	    px = x + shape->off[ip++].dx;
	} else break;
    }
    return true;
}

// Write the given shape into the current board, turning it on
// if onoff is true, and off if onoff is false.
static void place (const struct shape* shape, unsigned y, unsigned x, bool onoff)
{
    unsigned py = y, px = x, ip = 0;
    for (;;) {
	assert (py < B_ROWS && px < B_COLS);
	_board[py][px] = onoff;
	if (ip < ArraySize(shape->off)) {
	    py = y + shape->off[ip].dy;
	    px = x + shape->off[ip++].dx;
	} else break;
    }
}

//----------------------------------------------------------------------
// Game window drawing

static void scr_create_window (void)
{
    if (_win)
	delwin (_win);
    _win = newwin (S_ROWS, S_COLS, LINES-S_ROWS, 0);
}

static void scr_drawbar (unsigned y, unsigned x, unsigned h, unsigned w)
{
    for (unsigned i = 0; i < h; ++i)
	mvwhline (_win, y+i, x, ' ', w);
}

static void scr_update(void)
{
    werase (_win);
    wattr_set (_win, A_NORMAL, color_Background, NULL);
    scr_drawbar (0, 0, S_ROWS, S_COLS);

    // Explicitly draw black boxes under
    wcolor_set (_win, color_Shapes, NULL);
    scr_drawbar (0, BOARD_COL, B_ROWS, B_COLS*2);
    scr_drawbar (1, SCORE_COL, SCORE_ROWS, SCORE_COLS);

    // Draw score and level
    wcolor_set (_win, color_Text, NULL);
    mvwprintw (_win, 2, SCORE_COL+2, "Score: %d", _score);
    mvwprintw (_win, 3, SCORE_COL+2, "Level: %u", get_level());

    if (_paused)
	mvwaddstr (_win, S_ROWS/2, (S_COLS-strlen("Paused"))/2, "Paused");
    else {
	// draw preview of nextpattern
	wattr_set (_win, A_REVERSE, color_Shapes, NULL);
	unsigned py = 1, px = 1, ip = 0;
	for (;;) {
	    mvwaddstr (_win, 5+py, SCORE_COL+4+px*2, "  ");
	    if (ip < ArraySize(_nextshape->off)) {
		py = 1 + _nextshape->off[ip].dy;
		px = 1 + _nextshape->off[ip++].dx;
	    } else break;
	}

	// Draw the board
	for (unsigned y = 0; y < B_ROWS; ++y)
	    for (unsigned x = 0; x < B_COLS; ++x)
		if (_board[y][x])
		    mvwaddstr (_win, y, BOARD_COL + 2*x, "  ");
    }

    // Draw the board border
    wattr_set (_win, A_REVERSE, color_Borders, NULL);
    mvwvline (_win, 0, BOARD_COL-1, ' ', B_ROWS+1);
    mvwvline (_win, 0, BOARD_COL-2, ' ', B_ROWS+1);
    mvwvline (_win, 0, BOARD_COL+B_COLS*2, ' ', B_ROWS+1);
    mvwvline (_win, 0, BOARD_COL+B_COLS*2+1, ' ', B_ROWS+1);
    mvwhline (_win, B_ROWS, BOARD_COL, ' ', B_COLS*2);
    wattr_off (_win, A_REVERSE, NULL);
    mvwvline (_win, 0, BOARD_COL-3, ACS_VLINE, B_ROWS+1);
    mvwvline (_win, 0, BOARD_COL+B_COLS*2+2, ACS_VLINE, B_ROWS+1);

    wattr_set (_win, A_NORMAL, color_Default, NULL);
    wrefresh (_win);
}

// Show current scores.
static void showscores (unsigned level)
{
    const char* username = player_name();
    werase (_win);
    mvwaddstr (_win, 0, 0, "Drop4 High Scores");
    mvwaddstr (_win, 1, 0, "Rank  Score   Name             (points/level)");
    // Copy each row so variadic calls receive values independent of stored scores.
    for (unsigned i = 0; i < MAXHISCORES && _scores[i].score; ++i) {
	struct highscore row = _scores[i];
	bool bMeOnThisRow = (row.level == level && row.score == _score && username && !strcmp(row.name, username));
	if (bMeOnThisRow)
	    wstandout (_win);
	mvwprintw (_win, 2+i, 0, "%3u  %6u   %-16s (%7u on %u)", i+1, row.score * row.level, row.name, row.score, row.level);
	if (bMeOnThisRow)
	    wstandend (_win);
    }
    mvwprintw (_win, 22, 0, "Your score is %u", _score * level);
    mvwaddstr (_win, 23, 0, "Hit RETURN to continue.");
    wrefresh (_win);
    wtimeout (_win, -1);
    wgetch (_win);
}

//----------------------------------------------------------------------
// Scorefile reading and writing

// Score comparison function for qsort.
//
// If two scores are equal, the person who had the score first is
// listed first in the highscore file.
static int cmpscores (const void *x, const void *y)
{
    const struct highscore* a = (const struct highscore*) x;
    const struct highscore* b = (const struct highscore*) y;
    int l = b->level * b->score - a->level * a->score;
    if (!l)
	l = a->time - b->time;
    return sign(l);
}

static void check_scores (void)
{
    time_t expiration = time(NULL)-EXPIRATION;
    // Check validity of each score. Zero out any that are bad.
    for (struct highscore *sc = _scores, *scend = &_scores[ArraySize(_scores)]; sc < scend; ++sc)
	if (sc->level < MINLEVEL || sc->level > MAXLEVEL
		|| sc->time < expiration
		|| !sc->name[0] || sc->name[sizeof(sc->name)-1])
	    memset (sc, 0, sizeof(*sc));
    qsort (_scores, ArraySize(_scores), sizeof(_scores[0]), cmpscores);
}

static void savescore (unsigned score, unsigned level)
{
    read_score_file (DROP4_SCOREFILE, SCOREFILE_MAGIC, _scores, sizeof(_scores));
    check_scores();
    struct highscore* nsc = &_scores[MAXHISCORES-1];
    if (nsc->score*nsc->level > score*level)
	return;	// The new score is too low to save
    const char* username = player_name();
    if (!username)
	return;
    strncpy (nsc->name, username, sizeof(nsc->name)-1);
    nsc->name[sizeof(nsc->name)-1] = 0;
    nsc->score = score;
    nsc->level = level;
    nsc->time = time (NULL);
    check_scores();
    write_score_file (DROP4_SCOREFILE, SCOREFILE_MAGIC, _scores, sizeof(_scores));
}
