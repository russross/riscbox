// Copyright (c) 1980 The Regents of the University of California.
// This file is free software, distributed under the BSD license.
//
// snake - crt hack game.
//
// You move around the screen with arrow keys trying to pick up money
// without getting eaten by the snake. hjkl work as in vi in place of
// arrow keys. You can leave at the exit any time.

#include "config.h"

#define SNAKE_SCOREFILE	_PATH_GAME_STATE "snake.scores"
#define SCOREFILE_MAGIC	"snake"

enum {
    SNAKE_LENGTH	= 16,
    SPACEWARP_PENALTY	= 10,
    MAXSCORES		= 10,
    MAXSCORE		= INT_MAX
};
enum {
    color_None,
    color_Field,
    color_Snake,
    color_Me,
    color_Money,
    color_Goal
};
enum {
    ME		= A_BOLD| COLOR_PAIR(color_Me)| '@',
    SNAKEHEAD	= A_BOLD| COLOR_PAIR(color_Snake)| '@',
    SNAKETAIL	= A_BOLD| COLOR_PAIR(color_Snake)| 's',
    TREASURE	= A_BOLD| COLOR_PAIR(color_Money)| '$',
    GOAL	= A_BOLD| COLOR_PAIR(color_Goal)| '#'
};
enum EDir {
    dir_North,
    dir_East,
    dir_South,
    dir_West,
    dir_N,
    dir_Opposite = 2
};

struct point {
    unsigned short	col;
    unsigned short	line;
};

struct Score {
    unsigned	score;
    char	name [16-sizeof(unsigned)];
};

//----------------------------------------------------------------------

static struct point _you = {0,0};
static struct point _money = {0,0};
static struct point _finish = {0,0};
static struct point _snake [SNAKE_LENGTH] = {{0,0}};

static unsigned _loot = 0;

static unsigned short _penalty = 0;	// current spacewarp penalty
static unsigned short _chunk = 0;	// amount of money given at a time

static WINDOW* _wscore = NULL;		// top line with money displayed
static WINDOW* _wgame = NULL;		// main game field

static struct Score _scores [MAXSCORES] = {{0,""}};

//----------------------------------------------------------------------

static void calculate_screen_size (unsigned short rcols, unsigned short rlines);
static struct point find_empty_spot (void);
static void setup (void);

static inline bool stepped_on_snake (void);
static enum EDir segment_direction (const struct point* p1, const struct point* p2);
static void pushsnake (void);
static void spacewarp (bool bonus);
static void surround (struct point *);
static void be_eaten (void);
static void win (const struct point *);
static void winnings (unsigned won);

static int compare_scores (const void *x, const void *y);
static bool read_scores (void);
static void show_scores (void);
static bool save_score (unsigned score, bool won);

//----------------------------------------------------------------------

static inline bool same (const struct point* p1, const struct point* p2)
    { return p1->line == p2->line && p1->col == p2->col; }
static void pchar (const struct point* p, int c)
    { mvwaddch (_wgame, p->line+1, p->col+1, c); }
static inline void delay (unsigned t)
    { usleep (t*50000); }
static inline int cashvalue (void)
    { return _chunk*(_loot-_penalty)/25; }

//----------------------------------------------------------------------

int main (int argc, const char* const* argv)
{
    unsigned short rlines = 0, rcols = 0;	// user requested field size
    if (argc == 3) {
	rcols = atoi (argv[1]);
	rlines = atoi (argv[2]);
    } else if (argc == 2) {
	show_scores();
	return EXIT_SUCCESS;
    } else if (argc != 1) {
	puts ("Usage: snake [width height] [-s]");
	return EXIT_SUCCESS;
    }

    initialize_curses();
    static const struct color_pair c_Pairs[] = {
	{ COLOR_YELLOW,	COLOR_BLACK	},	// color_Field
	{ COLOR_GREEN,	COLOR_BLACK	},	// color_Snake
	{ COLOR_RED,	COLOR_BLACK	},	// color_Me
	{ COLOR_YELLOW,	COLOR_BLACK	},	// color_Money
	{ COLOR_CYAN,	COLOR_BLACK	}	// color_Goal
    };
    init_pairs (ArrayBlock (c_Pairs));
    calculate_screen_size (rcols, rlines);

    box (_wgame, 0, 0);
    _finish = find_empty_spot();
    _you = find_empty_spot();
    _money = find_empty_spot();
    _snake[0] = find_empty_spot();
    for (unsigned i = 1; i < ArraySize(_snake); ++i) {
	_snake[i] = _snake[i-1];
	if (_snake[i].col > 1)
	    --_snake[i].col;
    }
    setup();

    wtimeout (_wgame, 100);
    for (int lastc = 0;;) {
	wrefresh (_wgame);
	int c = wgetch (_wgame);
	if (c == ERR)
	    c = lastc;
	if (c == 'q' || c == KEY_F(10))
	    break;
	else if (c == 'w' || c == ';') {
	    spacewarp (false);
	    continue;
	}
	pchar (&_you, ' ');
	if ((c == 'h' || c == KEY_LEFT) && _you.col > 0)
	    --_you.col;
	else if ((c == 'l' || c == KEY_RIGHT) && _you.col < getmaxx(_wgame)-3)
	    ++_you.col;
	else if ((c == 'k' || c == KEY_UP) && _you.line > 0)
	    --_you.line;
	else if ((c == 'j' || c == KEY_DOWN) && _you.line < getmaxy(_wgame)-3)
	    ++_you.line;
	pchar (&_you, ME);
	lastc = c;
	pushsnake();
	if (same (&_you, &_money)) {
	    _loot += 25;
	    _money = find_empty_spot();
	    pchar (&_money, TREASURE);
	    winnings (cashvalue());
	} else if (same (&_you, &_finish)) {
	    win (&_finish);
	    cleanup_curses();
	    printf ("You have won with $%d.\n", cashvalue());
	    save_score (cashvalue(), true);
	    break;
	} else if (stepped_on_snake()) {
	    be_eaten();
	    cleanup_curses();
	    if (_loot >= _penalty)
		printf("You and your $%d have been eaten\n", cashvalue());
	    else
		printf("The snake ate you. You owe $%d.\n", -cashvalue());
	    break;
	}
    }
    return EXIT_SUCCESS;
}

static inline bool stepped_on_snake (void)
{
    for (unsigned i = 0; i < ArraySize(_snake); ++i)
	if (same(&_snake[i], &_you))
	    return true;
    return false;
}

static void calculate_screen_size (unsigned short rcols, unsigned short rlines)
{
    enum { c_MinFairFieldSize = 12 };
    if (!rlines || rlines < c_MinFairFieldSize || rlines > LINES-1)
	rlines = LINES-1;
    if (!rcols || rcols < c_MinFairFieldSize || rcols > COLS)
	rcols = COLS;
    unsigned mindim = rlines;
    if (rcols < mindim)
	mindim = rcols;
    // chunk is the amount of money the user gets for each $.
    // The formula below tries to be fair for various screen sizes.
    // We only pay attention to the smaller of the 2 edges, since
    // that seems to be the bottleneck.
    // This formula is a hyperbola which includes the following points:
    //      (24, $25)       (original scoring algorithm)
    //      (12, $40)       (experimentally derived by the "feel")
    //      (48, $15)       (a guess)
    // This will give a 4x4 screen $99/shot.  We don't allow anything
    // smaller than 4x4 because there is a 3x3 game where you can win
    // an infinite amount of money.
    //
    // Compensate for border.  This really changes the game since
    // the screen is two squares smaller but we want the default
    // to be $25, and the high scores on small screens were a bit
    // much anyway.
    mindim += 2;
    _chunk = (675.0 / (mindim + 6)) + 2.5;	// min screen edge

    // Create the score and game field windows
    _wscore = newwin (1, rcols, LINES-rlines-1, 0);
    _wgame = newwin (rlines, rcols, LINES-rlines, 0);
    keypad (_wgame, true);
    wbkgdset (_wgame, COLOR_PAIR(color_Field));
}

// setup the board
static void setup(void)
{
    werase (_wgame);
    pchar(&_you, ME);
    pchar(&_finish, GOAL);
    pchar(&_money, TREASURE);
    for (unsigned i = 1; i < ArraySize(_snake); ++i)
	pchar(&_snake[i], SNAKETAIL);
    pchar(&_snake[0], SNAKEHEAD);
    box (_wgame, 0, 0);
    wrefresh (_wgame);
}

static struct point find_empty_spot (void)
{
    struct point p;
    do {
	p.col = nrand (getmaxx(_wgame)-2);
	p.line = nrand (getmaxy(_wgame)-2);
    } while ((char) mvwinch (_wgame, p.line+1, p.col+1) != ' ');
    return p;
}

static enum EDir segment_direction (const struct point* p1, const struct point* p2)
{
    int dl = p2->line - p1->line, dc = p2->col - p1->col;
    if (absv(dl) > absv(dc))
	return dl > 0 ? dir_South : dir_North;
    else
	return dc > 0 ? dir_East : dir_West;
}

static void pushsnake (void)
{
    // Draw the shifted tail
    if (!same (&_snake[ArraySize(_snake)-1], &_you))	// If the player did not step on the tail
	pchar(&_snake[ArraySize(_snake)-1], ' ');	// Erase tail
    for (unsigned i = ArraySize(_snake)-1; i > 0; --i)
	_snake[i] = _snake[i-1];		// Segments roll over in sequence
    pchar(&_snake[1], SNAKETAIL);		// This was the head

    // Find previous direction
    unsigned d = segment_direction (&_snake[2], &_snake[1]);
    enum EDir playerDir = segment_direction (&_snake[1], &_you);
    if (playerDir == (d+dir_Opposite)%dir_N)
	playerDir = d;	// snake can't see behind it

    // There are three directions available
    // Pick one at random, favoring the current direction, and toward player
    unsigned choice = nrand (16);
    if (choice < 1)		// turn left
	d = (d-1)%dir_N;
    else if (choice < 2)	// turn right
	d = (d+1)%dir_N;
    else if (choice < 4)	// turn toward player
	d = playerDir;

    do {	// Create the new head point
	static const struct { int8_t dl,dc; } c_DirDPts [dir_N] =
	    {{-1,0},{0,1},{1,0},{0,-1}};
	_snake[0].line = _snake[1].line + c_DirDPts[d].dl;
	_snake[0].col = _snake[1].col + c_DirDPts[d].dc;
	d = (d+1)%dir_N;	// if hit something, keep turning until free
    } while (_snake[0].line >= getmaxy(_wgame)-2
	    || _snake[0].col >= getmaxx(_wgame)-2
	    || same (&_snake[0], &_finish));

    // Draw the new head
    pchar (&_snake[0], SNAKEHEAD);

    // The snake can eat money
    if (same (&_snake[0], &_money)) {
	_money = find_empty_spot();
	pchar (&_money, TREASURE);
    }
}

static void spacewarp (bool bonus)
{
    _you = find_empty_spot();
    const char* str = "SPACE WARP!!!";
    if (bonus) {
	str = "BONUS!!!";
	_loot -= _penalty;
	_penalty = 0;
    } else
	_penalty += _loot / SPACEWARP_PENALTY;
    // Flash the message in the center of the empty screen
    wattr_set (_wgame, A_BOLD, color_Goal, NULL);
    for (unsigned j = 0; j < 3; ++j) {
	werase (_wgame);
	box (_wgame, 0, 0);
	wrefresh (_wgame);
	delay(5);
	mvwaddstr (_wgame, getmaxy(_wgame)/2, (getmaxx(_wgame)-strlen(str))/2, str);
	wrefresh (_wgame);
	delay(10);
    }
    wattr_set (_wgame, A_NORMAL, color_Field, NULL);
    setup();
    winnings (cashvalue());
}

static void draw_snake_head_frame (struct point* ps, const char (*f)[4])
{
    for (unsigned i = 0; i < 3; ++i)
	mvwaddstr (_wgame, ps->line+i,   ps->col, f[i]);
}

static void surround(struct point *ps)
{
    // Adjust snake to not leave the window
    if (ps->col == 0)
	++ps->col;
    if (ps->line == 0)
	++ps->line;
    if (ps->line == LINES-1)
	--ps->line;
    if (ps->col == COLS-1)
	--ps->col;

    // Eating animation frames
    enum { frame_BigMouth, frame_ClosedMouth, frame_Wink, frame_NFrames};
    static const char c_SnakeHeadFrames [frame_NFrames][3][4] = {
	{"/*\\",
	 "* *",
	 "\\*/"},	// big mouth
	{"   ",
	 "o.o",
	 "\\_/"},	// closed mouth, eyes
	{"   ",
	 "o.-",
	 "\\_/"}	// wink
    };

    draw_snake_head_frame (ps, c_SnakeHeadFrames[frame_BigMouth]);
    for (unsigned j = 0; j < 20; ++j) {		// chewing :)
	pchar (ps, '@');
	wrefresh (_wgame);
	delay(1);
	pchar (ps, ' ');
	wrefresh (_wgame);
	delay(1);
    }
    if (save_score (cashvalue(), false)) {	// wink if better score
	draw_snake_head_frame (ps, c_SnakeHeadFrames[frame_ClosedMouth]);
	wrefresh (_wgame);
	delay(6);
	draw_snake_head_frame (ps, c_SnakeHeadFrames[frame_Wink]);
	wrefresh (_wgame);
	delay(6);
    }
    draw_snake_head_frame (ps, c_SnakeHeadFrames[frame_ClosedMouth]);
    wrefresh (_wgame);
    delay(6);
}

static void be_eaten (void)
{
    surround(&_you);
    unsigned bonus = nrand(10);
    if (cashvalue()%10u == bonus) {
	mvwprintw (_wgame, getmaxy(_wgame)-2, 2, "Bonus %u!\n", bonus);
	wrefresh (_wgame);
	delay(30);
	return spacewarp (true);
    }
}

static void win (const struct point* ps)
{
    enum { c_BoxSize = 5 };	// The size of box at full expansion
    for (unsigned j = 1; j < c_BoxSize; ++j) {
	for (unsigned l = ps->line+1-j; l <= ps->line+1+j; ++l)
	    mvwhline (_wgame, l, ps->col+1-j, GOAL, 2*j+1);
	wrefresh (_wgame);
	delay(1);
    }
}

static void winnings (unsigned won)
{
    if (won > 0) {
	mvwprintw (_wscore, 0, 1, "$%u ", won);
	wrefresh (_wscore);
    }
}

//----------------------------------------------------------------------
// Scorefile reading and writing

// Score comparator for qsort, in reverse order
static int compare_scores (const void *x, const void *y)
{
    const struct Score* a = (const struct Score*) x;
    const struct Score* b = (const struct Score*) y;
    return sign (b->score - a->score);
}

static bool read_scores (void)
{
    if (!read_score_file (SNAKE_SCOREFILE, SCOREFILE_MAGIC, _scores, sizeof(_scores)))
	return false;
    // Check each score and zero if invalid
    for (unsigned i = 0; i < ArraySize(_scores); ++i)
	if (!_scores[i].name[0] || _scores[i].name[sizeof(_scores[i].name)-1] || _scores[i].score > MAXSCORE)
	    memset (&_scores[i], 0, sizeof(_scores[i]));
    // Resort to account for the above zeroing
    qsort (_scores, ArraySize(_scores), sizeof(_scores[0]), compare_scores);
    return true;
}

static void show_scores (void)
{
    if (read_scores()) {
	printf("Snake players scores to date:\n");
	for (unsigned i = 0; i < ArraySize(_scores) && _scores[i].score; ++i)
	    printf("%u:\t$%u\t%s\n", i+1, _scores[i].score, _scores[i].name);
    } else
	puts ("No scores recorded yet!");
}

static bool save_score (unsigned score, bool won)
{
    read_scores();
    // Save the scores only if the new score is better than the lowest
    struct Score* ns = &_scores[ArraySize(_scores)-1];
    if (ns->score >= score)
	return false;
    else if (!won)
	return true;	// Return true to wink if the score would have been saved
    // Get user name
    const char* name = player_name();
    ns->score = score;
    strncpy (ns->name, name, sizeof(ns->name)-1);
    ns->name[sizeof(ns->name)-1] = 0;
    // Find user's previous best score
    unsigned prevbest = 0;
    for (const struct Score* os = _scores; os < ns && os->score > prevbest; ++os)
	if (0 == strcmp (os->name, ns->name))
	    prevbest = os->score;
    // Tell him how good he is
    if (prevbest) {
	if (prevbest < score)
	    printf ("You bettered your previous best of $%u\n", prevbest);
	else if (prevbest > score)
	    printf ("Your best to date is $%d\n", prevbest);
    }
    if (_scores[0].score) {
	if (_scores[0].score > score && !prevbest)
	    printf ("The highest is %s with $%u\n", _scores[0].name, _scores[0].score);
	else if (_scores[0].score < score) {
	    if (0 == strcmp (_scores[0].name, ns->name))
		printf ("You set a new record!\n");
	    else
		printf ("You beat %s's old record of $%u!\n", _scores[0].name, _scores[0].score);
	}
    }
    // Save the scores if the new score is better than the lowest
    qsort (_scores, ArraySize(_scores), sizeof(_scores[0]), compare_scores);
    write_score_file (SNAKE_SCOREFILE, SCOREFILE_MAGIC, _scores, sizeof(_scores));
    return true;
}
