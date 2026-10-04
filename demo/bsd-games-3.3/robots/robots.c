// Copyright (c) 2016 by Mike Sharov <msharov@users.sourceforge.net>
// This file is free software, distributed under the BSD License.
//
// This game was rewritten from scratch, but the gameplay and the
// look of the playing field are derived from code contributed to
// The NetBSD Foundation by Christos Zoulas in 1999, and which was
// originally written at University of California in 1980.

#include "config.h"

//----------------------------------------------------------------------

#define	ROBOTS_SCOREFILE	_PATH_GAME_STATE "robots.scores"
#define SCOREFILE_MAGIC		"robots"

enum {
    MAXLEVELS		= 4,
    MAXSCORES		= 16,
    MAXSCORE		= INT_MAX,
    ROBOTS_PER_LEVEL	= 16,
    MAXROBOTS		= MAXLEVELS * ROBOTS_PER_LEVEL,
    MAXHEAPS		= MAXROBOTS/2,
    LEVEL_BONUS		= 60,
    SCREEN_WIDTH	= 80,
    SCREEN_HEIGHT	= 24,
    FIELD_HEIGHT	= SCREEN_HEIGHT,
    HELP_WIDTH		= 20,
    FIELD_WIDTH		= SCREEN_WIDTH-HELP_WIDTH,
};
enum {
    color_None,
    color_Field,
    color_Player,
    color_Teleport,
    color_Robots,
    color_Heaps
};
enum {
    dir_N,
    dir_NE,
    dir_E,
    dir_SE,
    dir_S,
    dir_SW,
    dir_W,
    dir_NW,
    NDIRS
};
enum {
    ROBOT	= A_BOLD| COLOR_PAIR(color_Robots)| '+',
    JUNKHEAP	= A_BOLD| COLOR_PAIR(color_Heaps )| '*',
    PLAYER	= A_BOLD| COLOR_PAIR(color_Player)| '@',
    TELEPORT	= A_BOLD| COLOR_PAIR(color_Teleport)|'=',
};

struct Point {
    unsigned short	x;
    unsigned short	y;
};

//----------------------------------------------------------------------

static unsigned _score = 0;
static unsigned _level = 0;
static unsigned _nRobots = 0;
static unsigned _nHeaps = 0;

static struct Point _robots [MAXROBOTS] = {{0,0}};
static struct Point _heaps [MAXHEAPS] = {{0,0}};
static struct Point _player = {0,0};

static WINDOW* _wgame = NULL;
static WINDOW* _whelp = NULL;

static const struct {
    int8_t dx:4;
    int8_t dy:4;
} c_DirOffsets [NDIRS+1] = {{0,-1},{1,-1},{1,0},{1,1},{0,1},{-1,1},{-1,0},{-1,-1},{0,0}};

//----------------------------------------------------------------------

static void initialize_windows (void);
static void initialize_level (void);

static void draw_field (void);
static void draw_help (void);
static void lose_game (void);
static void save_score (void);

static unsigned direction_from_key (char k);
static unsigned direction_toward (const struct Point* p1, const struct Point* p2);
static void move_robots (void);
static void move_player (char k);
static void move_teleport (void);

static int comparept (const void* v1, const void* v2);
static struct Point find_empty_spot (void);
static unsigned find_heap_at (const struct Point* p);
static unsigned find_robot_at (const struct Point* p);

static inline void delay (unsigned t)
    { usleep (1000000*t/120); }	// Make delay a fraction of 60Hz to minimize stutter
static inline bool equalpt (const struct Point* p1, const struct Point* p2)
    { return p1->x == p2->x && p1->y == p2->y; }
static inline void offset_in_direction (struct Point* p, unsigned d)
    { p->x += c_DirOffsets[d].dx; p->y += c_DirOffsets[d].dy; }

//----------------------------------------------------------------------

int main (void)
{
    initialize_curses();
    initialize_windows();
    for (;;) {
	if (!_nRobots)
	    initialize_level();
	draw_field();
	draw_help();
	int k = wgetch (_wgame);
	if (k == 'q' || k == KEY_F(10))
	    break;
	else if (k == 't')
	    move_teleport();
	else
	    move_player (k);
	move_robots();
	if (find_robot_at(&_player) < _nRobots || find_heap_at(&_player) < _nHeaps) {
	    lose_game();
	    break;
	}
    }
    save_score();
    return EXIT_SUCCESS;
}

static void initialize_windows (void)
{
    _whelp = newwin (FIELD_HEIGHT, HELP_WIDTH, LINES-FIELD_HEIGHT, FIELD_WIDTH);
    _wgame = newwin (FIELD_HEIGHT, FIELD_WIDTH, LINES-FIELD_HEIGHT, 0);
    scrollok (_wgame, false);
    keypad (_wgame, true);
    static const struct color_pair c_Pairs[] = {
	{ COLOR_YELLOW,	COLOR_BLACK	},	// color_Field
	{ COLOR_YELLOW,	COLOR_BLACK	},	// color_Player
	{ COLOR_CYAN,	COLOR_BLACK	},	// color_Teleport
	{ COLOR_RED,	COLOR_BLACK	},	// color_Robots
	{ COLOR_BLACK,	COLOR_BLACK	},	// color_Heaps
    };
    init_pairs (ArrayBlock (c_Pairs));
    wbkgdset (_wgame, COLOR_PAIR(color_Field));
}

static void initialize_level (void)
{
    _score += LEVEL_BONUS * _level;
    if (_level < MAXLEVELS)
	++_level;
    for (_nRobots = _nHeaps = 0; _nRobots < ROBOTS_PER_LEVEL*_level; ++_nRobots)
	_robots[_nRobots] = find_empty_spot();
    _player = find_empty_spot();
}

//----------------------------------------------------------------------
// Drawing

static void draw_field (void)
{
    werase (_wgame);
    box (_wgame, 0, 0);
    for (unsigned i = 0; i < _nHeaps; ++i)
	mvwaddch (_wgame, _heaps[i].y+1, _heaps[i].x+1, JUNKHEAP);
    for (unsigned i = 0; i < _nRobots; ++i)
	mvwaddch (_wgame, _robots[i].y+1, _robots[i].x+1, ROBOT);
    mvwaddch (_wgame, _player.y+1, _player.x+1, PLAYER);
    wrefresh (_wgame);
}

static void draw_help (void)
{
    werase (_whelp);
    mvwprintw (_whelp, 0, 0,
	" Directions:\n"
	" \n"
	" y k u\n"
	"  \\|/\n"
	" h- -l\n"
	"  /|\\\n"
	" b j n\n"
	" \n"
	" Commands:\n"
	" \n"
	" t:  teleport\n"
	" q:  quit\n"
	" \n"
	" Legend:\n"
	" \n"
	" +  robot\n"
	" *  junk heap\n"
	" @  you\n"
	" \n"
	" Score: %u", _score);
    wrefresh (_whelp);
}

static void lose_game (void)
{
    draw_field();
    static const char c_Scream[] = "Aaaaargghhhh!!!!";
    wattr_set (_wgame, A_BOLD, color_Robots, NULL);
    for (unsigned i = 1; i < strlen(c_Scream); ++i) {
	mvwaddnstr (_wgame, _player.y+1, _player.x+1, c_Scream, i);
	wrefresh (_wgame);
	delay(8);
    }
    delay(80);
}

//----------------------------------------------------------------------
// Data search

static int comparept (const void* v1, const void* v2)
{
    const struct Point* p1 = (const struct Point*) v1;
    const struct Point* p2 = (const struct Point*) v2;
    int sx = sign (p1->x-p2->x), sy = sign (p1->y-p2->y);
    return sx ? sx : sy;
}

static struct Point find_empty_spot (void)
{
    struct Point r;
    do {
	r.x = nrand (getmaxx(_wgame)-2);
	r.y = nrand (getmaxy(_wgame)-2);
    } while (equalpt (&r, &_player)
	    || find_robot_at(&r) != _nRobots
	    || find_heap_at(&r) != _nHeaps);
    return r;
}

static unsigned find_robot_at (const struct Point* p)
{
    unsigned i = 0;
    for (; i < _nRobots && !equalpt(p,&_robots[i]); ++i) {}
    return i;
}

static unsigned find_heap_at (const struct Point* p)
{
    unsigned i = 0;
    for (; i < _nHeaps && !equalpt(p,&_heaps[i]); ++i) {}
    return i;
}

//----------------------------------------------------------------------
// Direction and movement

static unsigned direction_toward (const struct Point* p1, const struct Point* p2)
{
    int dx = sign (p2->x-p1->x), dy = sign (p2->y-p1->y);
    for (unsigned d = 0; d < ArraySize(c_DirOffsets); ++d)
	if (c_DirOffsets[d].dx == dx && c_DirOffsets[d].dy == dy)
	    return d;
    return NDIRS;
}

static void offset_toward (struct Point* p, const struct Point* pdest)
    { offset_in_direction (p, direction_toward (p, pdest)); }

static void move_robots (void)
{
    // Move each robot toward player
    for (unsigned i = 0; i < _nRobots; ++i)
	offset_toward (&_robots[i], &_player);
    // Check for collisions and convert to heaps
    qsort (_robots, _nRobots, sizeof(_robots[0]), comparept);
    for (int i = _nRobots-1; i >= 0; --i) {
	bool heapHere = find_heap_at (&_robots[i]) < _nHeaps;
	if (i && equalpt (&_robots[i], &_robots[i-1]) && !heapHere) {
	    _heaps[_nHeaps++] = _robots[i];	// Collision here
	    ++_score;
	    heapHere = true;
	}
	if (heapHere) {		// Delete robot here
	    memmove (&_robots[i], &_robots[i+1], (--_nRobots-i)*sizeof(_robots[i]));
	    ++_score;
	}
    }
}

static unsigned direction_from_key (char k)
{
    static const char c_Keys [NDIRS+1] = "kulnjbhy";
    const char* d = strchr (c_Keys, k);
    return d ? d - c_Keys : NDIRS;
}

static void move_player (char k)
{
    struct Point p = _player;
    offset_in_direction (&p, direction_from_key (k));
    if (p.x < getmaxx(_wgame)-2 && p.y < getmaxy(_wgame)-2 && find_heap_at(&p) == _nHeaps)
	_player = p;
}

static void move_teleport (void)
{
    struct Point dest = find_empty_spot();
    while (!equalpt (&_player, &dest)) {
	mvwaddch (_wgame, _player.y+1, _player.x+1, TELEPORT);
	wrefresh (_wgame);
	offset_toward (&_player, &dest);
	delay(1);
    }
}

//----------------------------------------------------------------------
// Scores

struct Score {
    unsigned	score;
    char	name [16-sizeof(unsigned)];
};

static struct Score _scores [MAXSCORES] = {{0,""}};

static int compare_scores (const void* v1, const void* v2)
    { return sign (((const struct Score*)v2)->score - ((const struct Score*)v1)->score); }

static void save_score (void)
{
    read_score_file (ROBOTS_SCOREFILE, SCOREFILE_MAGIC, _scores, sizeof(_scores));
    // Check each score and zero if invalid
    for (struct Score *s = _scores, *send = &_scores[ArraySize(_scores)]; s < send; ++s)
	if (!s->name[0] || s->name[sizeof(s->name)-1] || s->score > MAXSCORE)
	    memset (s, 0, sizeof(*s));
    // Resort to account for the above zeroing
    qsort (_scores, ArraySize(_scores), sizeof(_scores[0]), compare_scores);
    // Add this game's score, if it is high enough
    const char* name = player_name();
    struct Score* ns = &_scores[ArraySize(_scores)-1];
    bool bScoreSaved = false;
    if (name && ns->score < _score) {
	bScoreSaved = true;
	ns->score = _score;
	strncpy (ns->name, name, sizeof(ns->name));
	ns->name[sizeof(ns->name)-1] = 0;
	// Resort the new score
	qsort (_scores, ArraySize(_scores), sizeof(_scores[0]), compare_scores);
	// And write the score file
	write_score_file (ROBOTS_SCOREFILE, SCOREFILE_MAGIC, _scores, sizeof(_scores));
    }
    // Now show the scores in the field window
    werase (_whelp);
    werase (_wgame);
    box (_wgame, 0, 0);
    mvwhline (_wgame, 2, 1, 0, FIELD_WIDTH-2);
    mvwhline (_wgame, FIELD_HEIGHT-4, 1, 0, FIELD_WIDTH-2);
    wattr_set (_wgame, A_BOLD, color_None, NULL);
    mvwaddstr (_wgame, 1, 2, "Top Scores");
    for (unsigned i = 0; i < ArraySize(_scores) && _scores[i].score; ++i) {
	if (_score == _scores[i].score && name && 0 == strcmp (name, _scores[i].name))
	    wattr_on (_wgame, A_REVERSE, NULL);
	mvwprintw (_wgame, 3+i, 2, "%2u: %6u %-14s ", i+1, _scores[i].score, _scores[i].name);
	wattr_off (_wgame, A_REVERSE, NULL);
    }
    mvwaddstr (_wgame, FIELD_HEIGHT-3, 2, "Congratulations! You have been eaten by the evil robots.");
    if (!_score)
	mvwaddstr (_wgame, FIELD_HEIGHT-2, 2, "Sadly, the robots were unharmed by the exprience.");
    else if (bScoreSaved)
	mvwprintw (_wgame, FIELD_HEIGHT-2, 2, "You managed to get a top score of %u. Lucky you!", _score);
    else
	mvwprintw (_wgame, FIELD_HEIGHT-2, 2, "If it is any consolation, your puny score was %u.", _score);
    wrefresh (_whelp);
    flushinp();
    wgetch (_wgame);
}
