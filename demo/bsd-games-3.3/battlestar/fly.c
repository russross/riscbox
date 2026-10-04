// This file is free software, distributed under the BSD license.

#include "battlestar.h"

//----------------------------------------------------------------------

enum {
    c_NStars = 64,
    c_DrenianMoveTime = 500
};

static int _drenianLine = 0;
static int _drenianCol = 0;
static int _dr = 0, _dc = 0;
static uint64_t _drenianNextMove = 0;
static bool _destroyed = false;
static struct { uint8_t l, c; } _stars [c_NStars] = {{0,0}};

static WINDOW* _w = NULL;

//----------------------------------------------------------------------

static void blast(void);
static void create_space_window (void);

//----------------------------------------------------------------------

static void create_space_window (void)
{
    if (_w)
	mvwin (_w, LINES-24, 0);
    else {
	_w = newwin (24, 80, LINES-24, 0);
	keypad (_w, true);
    }
}

static void draw_space_window (void)
{
    werase (_w);
    for (unsigned i = 0; i < ArraySize(_stars); ++i)
	mvwaddch (_w, _stars[i].l, _stars[i].c, '.');
    mvwaddstr (_w, getmaxy(_w)/2, getmaxx(_w)/2 - 10, "-------   +   -------");
    for (unsigned n = getmaxy(_w)/2-4; n < getmaxy(_w)/2-1u; ++n) {
	mvwaddch (_w, n, getmaxx(_w)/2, '|');
	mvwaddch (_w, n+6, getmaxx(_w)/2, '|');
    }
    mvwaddstr (_w, _drenianLine, _drenianCol - 1, "/-\\"); // Drenian ship
    mvwprintw (_w, getmaxy(_w)-1, 21, "TORPEDOES: %3u  FUEL: %3u", torps, fuel);
}

int visual (void)
{
    _destroyed = false;
    create_space_window();
    cbreak();
    noecho();
    curs_set (0);
    _drenianLine = nrand (getmaxy(_w)-3)+1;
    _drenianCol = nrand (getmaxx(_w)-2)+1;
    for (unsigned i = 0; i < ArraySize(_stars); ++i) {
	_stars[i].l = nrand(getmaxy(_w));
	_stars[i].c = nrand(getmaxx(_w));
    }
    for (;;) {
	uint64_t now = time_ms();
	if (now >= _drenianNextMove) {
	    if (_drenianLine+_dr <= getmaxy(_w)-3 && _drenianLine+_dr > 0)
		_drenianLine += _dr;
	    if (_drenianCol+_dc < getmaxx(_w)-1 && _drenianCol+_dc > 0)
		_drenianCol += _dc;
	    _drenianNextMove = now + c_DrenianMoveTime;
	}
	draw_space_window();
	wtimeout (_w, _drenianNextMove - now);
	int k = wgetch(_w);
	if (k == 'h' || k == KEY_LEFT) {
	    _dc = -1;
	    --fuel;
	} else if (k == 'l' || k == KEY_RIGHT) {
	    _dc = 1;
	    --fuel;
	} else if (k == 'j' || k == KEY_DOWN) {
	    _dr = 1;
	    --fuel;
	} else if (k == 'k' || k == KEY_UP) {
	    _dr = -1;
	    --fuel;
	} else if (k == ' ' || k == 'f') {
	    if (torps) {
		torps -= 2;
		blast();
		if (_drenianLine == getmaxy(_w)/2 && _drenianCol - getmaxx(_w)/2 < 2 && getmaxx(_w)/2 - _drenianCol < 2) {
		    _destroyed = true;
		    sleep (1);
		    break;
		}
	    } else {
		mvwaddstr (_w, 0, 0, "*** Out of torpedoes. ***");
		sleep (1);
		break;
	    }
	} else if (k == 'q') {
	    endwin();
	    return 0;
	} else if (k == KEY_RESIZE)
	    create_space_window();
	if (!fuel) {
	    mvwaddstr (_w, 0, 60, "*** Out of fuel ***");
	    sleep (1);
	    break;
	}
    }
    endwin();
    return _destroyed;
}

static void blast(void)
{
    // Torpedo animation, flying torpedoes
    mvwprintw (_w, getmaxy(_w)-1, 24, "%3d", torps);
    for (unsigned n = getmaxy(_w)-1-2; n >= getmaxy(_w)/2+1u; --n) {
	mvwaddch (_w, n, getmaxx(_w)/2 + getmaxy(_w)/2 - n, '/');
	mvwaddch (_w, n, getmaxx(_w)/2 - getmaxy(_w)/2 + n, '\\');
	wrefresh (_w);
	usleep (50000);
    }
    mvwaddch (_w, getmaxy(_w)/2, getmaxx(_w)/2, '*');
    // Erase torpedo trail
    for (unsigned n = getmaxy(_w)-1-2; n >= getmaxy(_w)/2+1u; --n) {
	mvwaddch (_w, n, getmaxx(_w)/2 + getmaxy(_w)/2 - n, ' ');
	mvwaddch (_w, n, getmaxx(_w)/2 - getmaxy(_w)/2 + n, ' ');
	wrefresh (_w);
	usleep (50000);
    }
}
