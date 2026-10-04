// Copyright (c) 1987 by Ed James <edjames@berkeley.edu>
// This file is free software, distributed under the BSD license.

#include "atc.h"

//----------------------------------------------------------------------

enum {
    C_AIRPORT	= '=',
    C_LINE	= '+',
    C_BACKROUND	= '.',
    C_BEACON	= '*',
    C_CREDIT	= '*'
};
enum {
    color_None,
    color_RadarBackground,
    color_Lines,
    color_Planes,
    color_Airports,
    color_Beacons,
    color_Exits
};

//----------------------------------------------------------------------

static WINDOW* _wradar = NULL;
static WINDOW* _wcredit = NULL;
static WINDOW* _winput = NULL;
static WINDOW* _wplanes = NULL;

//----------------------------------------------------------------------

static void draw_line (WINDOW *w, int x, int y, int lx, int ly, char c);
static void draw_radar (void);
static void draw_plane_list (void);
static void draw_credit_box (void);
static void draw_command_box (void);

//----------------------------------------------------------------------

void init_gr (void)
{
    initialize_curses();
    unsigned radarh = _sp->height, radarw = _sp->width*2-1,
	scry = LINES - (MAXPLANES+1 + INPUT_LINES),
	scrx = (COLS - (radarw + PLANE_COLS))/2;
    _wradar = newwin (radarh, radarw, scry, scrx);
    _wplanes = newwin (MAXPLANES+1, PLANE_COLS, scry, scrx+radarw+1);
    _winput = newwin (INPUT_LINES, radarw, scry+radarh, scrx);
    _wcredit = newwin (INPUT_LINES, PLANE_COLS, scry+radarh, scrx+radarw+1);
    keypad (_winput, true);
    static const struct color_pair c_Pairs[] = {
	{ COLOR_BLACK,	COLOR_BLACK	},	// color_RadarBackground
	{ COLOR_BLUE,	COLOR_BLACK	},	// color_Lines
	{ COLOR_WHITE,	COLOR_BLACK	},	// color_Planes
	{ COLOR_YELLOW,	COLOR_BLACK	},	// color_Airports
	{ COLOR_RED,	COLOR_BLACK	},	// color_Beacons
	{ COLOR_GREEN,	COLOR_BLACK	}	// color_Exits
    };
    init_pairs (ArrayBlock (c_Pairs));
    wbkgdset (_wradar, A_BOLD| COLOR_PAIR(color_RadarBackground));
}

//----------------------------------------------------------------------
// Screen drawing

static void draw_line (WINDOW * w, int x, int y, int lx, int ly, char c)
{
    int dx = sign (lx - x);
    int dy = sign (ly - y);
    for (;;) {
	mvwaddch (w, y, x*2, c);
	if (x == lx && y == ly)
	    break;
	x += dx;
	y += dy;
    }
}

static void draw_radar (void)
{
    // Draw radar screen with map
    werase (_wradar);
    wattr_set (_wradar, A_NORMAL, color_RadarBackground, NULL);
    box (_wradar, 0, 0);
    for (unsigned i = 1; i < _sp->height-1u; ++i)
	for (unsigned j = 1; j < _sp->width-1u; ++j)
	    mvwaddch (_wradar, i, j*2, C_BACKROUND);
    wcolor_set (_wradar, color_Lines, NULL);
    for (unsigned i = 0; i < _sp->num_lines; ++i)
	draw_line(_wradar, _sp->line[i].p1.x, _sp->line[i].p1.y, _sp->line[i].p2.x, _sp->line[i].p2.y, C_LINE);
    wcolor_set (_wradar, color_Beacons, NULL);
    for (unsigned i = 0; i < _sp->num_beacons; ++i)
	mvwprintw (_wradar, _sp->beacon[i].y, _sp->beacon[i].x*2, "%c%u", C_BEACON, i);
    wcolor_set (_wradar, color_Exits, NULL);
    for (unsigned i = 0; i < _sp->num_exits; ++i)
	mvwprintw (_wradar, _sp->exit[i].y, _sp->exit[i].x*2, "%u", i);
    wcolor_set (_wradar, color_Airports, NULL);
    for (unsigned i = 0; i < _sp->num_airports; ++i)
	mvwprintw (_wradar, _sp->airport[i].y, _sp->airport[i].x * 2, "%c%u", "^?>?v?<?"[_sp->airport[i].dir], i);

    // Draw planes on radar
    wcolor_set (_wradar, color_Planes, NULL);
    vector_foreach (const struct Plane, p, _air) {
	if (p->status == S_MARKED)
	    wstandout (_wradar);
	char planetagbuf[8];
	ssize_t ptbs = snprintf (planetagbuf, sizeof(planetagbuf), "%c%hu", plane_name(p), p->altitude);
	mvwaddnstr (_wradar, p->ypos, p->xpos*2, planetagbuf, min_i (ptbs, getmaxx(_wradar)-p->xpos*2));
	if (p->status == S_MARKED)
	    wstandend (_wradar);
    }
    wnoutrefresh (_wradar);
}

static void draw_plane_list (void)
{
    werase (_wplanes);
    wprintw (_wplanes, "Time: %-4d Safe: %d", _clck, _safe_planes);
    mvwaddstr (_wplanes, 2, 0, "pl dt  comm");
    vector_foreach (const struct Plane, p, _air) {
	waddch (_wplanes, '\n');
	waddstr (_wplanes, command(p));
    }
    waddch (_wplanes, '\n');
    vector_foreach (const struct Plane, p, _ground) {
	waddch(_wplanes, '\n');
	waddstr(_wplanes, command(p));
    }
    wnoutrefresh (_wplanes);
}

static void draw_credit_box (void)
{
    mvwhline (_wcredit, 0, 0, '*', getmaxx(_wcredit)-1);
    mvwhline (_wcredit, getmaxy(_wcredit)-1, 0, '*', getmaxx(_wcredit)-1);
    wmove(_wcredit, INPUT_LINES / 2, 1);
    waddstr(_wcredit, AUTHOR_STR);
    wnoutrefresh (_wcredit);
}

static void draw_command_box (void)
{
    werase (_winput);
    mvwaddstr (_winput, getmaxy(_winput)/2, 0, _cmdtext);
    mvwaddstr (_winput, getmaxy(_winput)/2+1, 0, _errtext);
    wnoutrefresh (_winput);
}

void draw_all(void)
{
    draw_radar();
    draw_plane_list();
    draw_credit_box();
    draw_command_box();
    wrefresh (_winput);
}

//----------------------------------------------------------------------
// User input

int getAChar(void)
{
    static uint64_t s_NextUpdate = 0;
    for (;;) {
	draw_all();
	uint64_t now = time_ms();
	int k = 0;
	if (now >= s_NextUpdate)
	    s_NextUpdate = now + _sp->update_secs*1000;
	else {
	    wtimeout (_winput, s_NextUpdate - now);
	    k = wgetch (_winput);
	}
	if (!k)
	    update_planes();
	else
	    return k;
    }
}

_Noreturn void loser (const struct Plane* p, const char* msg, ...)
{
    werase (_winput);
    wprintw (_winput, "Plane %c ", plane_name(p));
    va_list args;
    va_start (args, msg);
    vw_printw (_winput, msg, args);
    va_end (args);
    waddstr (_winput, "\n\nHit space for top players list...");
    wtimeout (_winput, -1);
    wgetch (_winput);
    quitter();
}

_Noreturn void quitter (void)
{
    cleanup_curses();
    save_score (true);
    exit (EXIT_SUCCESS);
}
