// Copyright (c) 1980 The Regents of the University of California. // This file is free software, distributed under the BSD license.

#include "spirhunt.h"

//{{{ Local constants --------------------------------------------------

enum {
    color_None,
    color_Space,
    color_Panel,
    color_PanelGreen,
    color_PanelYellow,
    color_PanelRed,
    color_Ship,
    color_Shielded,
    color_Pirate,
    color_Star,
    color_Base,
    color_Planet,
    color_Plaser,
    color_Torpedo
};

enum {
    QUADRANT_VIEW_COLS = NSECTS*2+1,
    QUADRANT_VIEW_LINES = NSECTS,
    GALAXY_VIEW_COLS = NQUADS*3+1,
    GALAXY_VIEW_LINES = NQUADS,
    PANEL_COLS	= 77,
    PANEL_LINES	= QUADRANT_VIEW_LINES+2,
};

//}}}-------------------------------------------------------------------
//{{{ Local variables

static WINDOW*	_wmsg = NULL;
static WINDOW*	_wpanel = NULL;
static WINDOW*	_wquadrant = NULL;
static WINDOW*	_wgalaxy = NULL;
static WINDOW*	_winfo = NULL;
static WINDOW*	_wcmd = NULL;

//}}}-------------------------------------------------------------------
//{{{ Prototypes

static void draw_panel (void);
static void draw_quadrant_scan (void);
static void draw_galaxy_scan (void);
static void draw_ship_info (void);

//}}}-------------------------------------------------------------------
//{{{ Windows layout

void create_windows (void)
{
    static const struct color_pair c_Pairs[] = {
	{ COLOR_BLACK,	COLOR_BLACK	},	// color_Space
	{ COLOR_WHITE,	COLOR_BLUE	},	// color_Panel
	{ COLOR_GREEN,	COLOR_BLUE	},	// color_PanelGreen
	{ COLOR_YELLOW,	COLOR_BLUE	},	// color_PanelYellow
	{ COLOR_MAGENTA,COLOR_BLUE	},	// color_PanelRed
	{ COLOR_WHITE,	COLOR_BLACK	},	// color_Ship
	{ COLOR_CYAN,	COLOR_BLACK	},	// color_Shielded
	{ COLOR_RED,	COLOR_BLACK	},	// color_Pirate
	{ COLOR_YELLOW,	COLOR_BLACK	},	// color_Star
	{ COLOR_CYAN,	COLOR_BLACK	},	// color_Base
	{ COLOR_GREEN,	COLOR_BLACK	},	// color_Planet
	{ COLOR_GREEN,	COLOR_BLACK	},	// color_Plaser
	{ COLOR_WHITE,	COLOR_BLACK	}	// color_Torpedo
    };
    init_pairs (ArrayBlock (c_Pairs));

    // Clear the screen first; necessary on window resize
    clear();
    wnoutrefresh (stdscr);

    // The panel encloses the scans and the info
    if (_wpanel)
	delwin (_wpanel);
    _wpanel = newwin (PANEL_LINES, PANEL_COLS, LINES-PANEL_LINES, (COLS-PANEL_COLS)/2);
    leaveok (_wpanel, true);

    // Scrolling window for messages
    if (_wmsg)
	delwin (_wmsg);
    _wmsg = newwin (getbegy(_wpanel), PANEL_COLS, 0, getbegx(_wpanel));
    leaveok (_wmsg, true);
    scrollok (_wmsg, true);
    keypad (_wmsg, true);
    wmove (_wmsg, getmaxy(_wmsg)-1, 0);

    // The short-range scan of the current quadrant
    if (_wquadrant)
	delwin (_wquadrant);
    _wquadrant = newwin (QUADRANT_VIEW_LINES, QUADRANT_VIEW_COLS, getbegy(_wpanel)+1, getbegx(_wpanel)+3);
    leaveok (_wquadrant, true);

    // The map of the galaxy
    if (_wgalaxy)
	delwin (_wgalaxy);
    _wgalaxy = newwin (GALAXY_VIEW_LINES, GALAXY_VIEW_COLS, getbegy(_wquadrant), getbegx(_wquadrant)+getmaxx(_wquadrant)+1);
    leaveok (_wgalaxy, true);

    // Ship status
    if (_winfo)
	delwin (_winfo);
    unsigned infox = getbegx(_wgalaxy)+getmaxx(_wgalaxy)+4;
    unsigned infow = getbegx(_wpanel)+getmaxx(_wpanel)-infox-1;
    _winfo = newwin (QUADRANT_VIEW_LINES, infow, getbegy(_wquadrant), infox);

    // Command input window
    if (_wcmd)
	delwin (_wcmd);
    _wcmd = newwin (1, GALAXY_VIEW_COLS, getbegy(_wgalaxy)+getmaxy(_wgalaxy)+1, getbegx(_wgalaxy));
    leaveok (_wcmd, false);
}

//}}}-------------------------------------------------------------------
//{{{ Screen drawing

void draw_screen (void)
{
    erase_window (_wpanel, color_Panel);
    draw_panel();
    wnoutrefresh (_wpanel);

    erase_window (_wquadrant, color_Space);
    draw_quadrant_scan();
    wnoutrefresh (_wquadrant);

    erase_window (_wgalaxy, color_Space);
    draw_galaxy_scan();
    wnoutrefresh (_wgalaxy);

    erase_window (_winfo, color_Panel);
    draw_ship_info();
    wnoutrefresh (_winfo);

    wnoutrefresh (_wmsg);

    doupdate();
}

void draw_and_sleep (unsigned seconds)
{
    draw_screen();
    sleep (seconds);
}

//{{{2 Panel -----------------------------------------------------------

static void draw_panel (void)
{
    // Panel separator lines
    mvwvline (_wpanel, 0, 0, 0, getmaxy(_wpanel));
    mvwvline (_wpanel, 0, getbegx(_winfo)-getbegx(_wpanel)-2, 0, getmaxy(_wpanel));
    mvwvline (_wpanel, 0, getbegx(_winfo)+getmaxx(_winfo)-getbegx(_wpanel), 0, getmaxy(_wpanel));

    // y-rulers for the scans
    for (unsigned y = 0; y < NSECTS; ++y) {
	// Left ruler is for quadrant map
	if (Ship.sect.y == y)
	    wattr_on (_wpanel, A_BOLD, NULL);
	mvwaddch (_wpanel, y+1, getbegx(_wquadrant)-getbegx(_wpanel)-1, '0'+y);
	wattr_off (_wpanel, A_BOLD, NULL);
	// Middle ruler is shared by quadrant and galaxy
	if (Ship.sect.y == y || Ship.quad.y == y)
	    wattr_on (_wpanel, A_BOLD, NULL);
	mvwaddch (_wpanel, y+1, getbegx(_wquadrant)-getbegx(_wpanel)+getmaxx(_wquadrant), '0'+y);
	// Right ruler is for galaxy
	if (y < NQUADS)
	    mvwaddch (_wpanel, y+1, getbegx(_wgalaxy)-getbegx(_wpanel)+getmaxx(_wgalaxy), '0'+y);
	wattr_off (_wpanel, A_BOLD, NULL);
    }
    // x-rulers for the quadrant view
    for (unsigned x = 0; x < NSECTS; ++x) {
	unsigned nx = getbegx(_wquadrant)-getbegx(_wpanel) + x*2 + 1;
	if (Ship.sect.x == x)
	    wattr_on (_wpanel, A_BOLD, NULL);
	else
	    wattr_off (_wpanel, A_BOLD, NULL);
	mvwaddch (_wpanel, getbegy(_wquadrant)-1-getbegy(_wpanel), nx, '0'+x);
	mvwaddch (_wpanel, getbegy(_wquadrant)+getmaxy(_wquadrant)-getbegy(_wpanel), nx, '0'+x);
    }
    // x-rulers for the galaxy view
    for (unsigned x = 0; x < NQUADS; ++x) {
	unsigned nx = getbegx(_wgalaxy)-getbegx(_wpanel) + x*3 + 1;
	if (Ship.quad.x == x)
	    wattr_on (_wpanel, A_BOLD, NULL);
	else
	    wattr_off (_wpanel, A_BOLD, NULL);
	mvwaddch (_wpanel, getbegy(_wgalaxy)-1-getbegy(_wpanel), nx, '0'+x);
	mvwaddch (_wpanel, getbegy(_wgalaxy)+getmaxy(_wgalaxy)-getbegy(_wpanel), nx, '0'+x);
    }
}

//}}}2------------------------------------------------------------------
//{{{2 Quadrant scan

static void draw_sector_char (struct xy p, chtype c)
    { mvwaddch (_wquadrant, p.y, p.x*2+1, c); }

static void draw_quadrant_scan (void)
{
    // With short range scanners out, the view is reduced.
    // You still see stars and know where the starbase is,
    // but the pirates become invisible past VISCAN_RANGE.
    bool havesrs = !device_damaged (SRSCAN);
    const struct quad* q = current_quad();

    // Space markers for empty space
    char sc;
    if (q->stars == SUPERNOVA) {
	sc = '*';
	wattr_set (_wquadrant, A_BOLD, color_Star, NULL);
    } else {
	sc = '.';
	wattr_set (_wquadrant, A_BOLD, color_Space, NULL);
    }
    struct xy spi;
    for (spi.y = 0; spi.y < NSECTS; ++spi.y)
	for (spi.x = 0; spi.x < NSECTS; ++spi.x)
	    if (havesrs || VISCAN_RANGE >= sector_distance (Ship.sect, spi))
		draw_sector_char (spi, sc);

    if (q->stars == SUPERNOVA)
	return; // Everything is dead in a supernova

    // Sector contents; pirates, stars, bases, etc.
    wattr_set (_wquadrant, A_BOLD, color_Base, NULL);
    if (q->bases)
	draw_sector_char (Etc.starbase, BASE);

    wattr_set (_wquadrant, A_BOLD, color_Planet, NULL);
    if (q->systemname)
	draw_sector_char (Etc.inhabited, INHABIT);

    wattr_set (_wquadrant, A_BOLD, color_Star, NULL);
    for (unsigned i = 0; i < q->stars; ++i)
	draw_sector_char (Etc.stars[i], STAR);

    wattr_set (_wquadrant, A_BOLD, color_Space, NULL);
    for (unsigned i = 0; i < q->holes; ++i)
	draw_sector_char (Etc.blackholes[i], HOLE);

    wattr_set (_wquadrant, A_BOLD, color_Pirate, NULL);
    for (unsigned i = 0; i < q->pirates; ++i)
	if (havesrs || VISCAN_RANGE >= sector_distance (Ship.sect, Etc.pirate[i].sect))
	    draw_sector_char (Etc.pirate[i].sect, PIRATE);

    // Your ship
    if (Ship.cloaked)
	wattr_set (_wquadrant, A_BOLD, color_Space, NULL);
    else if (Ship.shldup)
	wattr_set (_wquadrant, A_BOLD, color_Shielded, NULL);
    else
	wattr_set (_wquadrant, A_NORMAL, color_Ship, NULL);
    draw_sector_char (Ship.sect, YOURSHIP);
}

//}}}2------------------------------------------------------------------
//{{{2 Galaxy scan

static void draw_galaxy_scan (void)
{
    bool havelrs = !device_damaged (LRSCAN);
    bool havesrs = !device_damaged (SRSCAN);
    for (unsigned y = 0; y < NQUADS; ++y) {
	for (unsigned x = 0; x < NQUADS; ++x) {
	    const struct quad* q = &Quad[y][x];
	    wmove (_wgalaxy, y, x*3+1);

	    // Mark current quadrant with reverse
	    struct xy cq = {x,y};
	    unsigned dist = sector_distance (Ship.quad, cq);
	    attr_t a = dist ? A_BOLD : A_REVERSE;

	    if (q->stars == SUPERNOVA) {
		wattr_set (_wgalaxy, a, color_Star, NULL);
		waddch (_wgalaxy, '*');
		continue;
	    }

	    // First char is starbase indicator, B or D for distressed
	    wattr_set (_wgalaxy, a, q->distressed ? color_Pirate : color_Base, NULL);
	    char starbase = ' ';
	    if (q->distressed && q->bases)
		starbase = 'A';
	    else if (q->distressed)
		starbase = 'D';
	    else if (q->bases)
		starbase = 'B';
	    else if (havelrs) {
		starbase = '.';
		if (dist)
		    wattr_set (_wgalaxy, a, color_Space, NULL);
	    }
	    waddch (_wgalaxy, starbase);

	    // Second char is the pirates count, out to scan range only
	    wattr_set (_wgalaxy, A_BOLD, color_Space, NULL);
	    char pirates = ' ';
	    if (dist <= LRSCAN_RANGE) {
		wattr_set (_wgalaxy, A_BOLD, color_Pirate, NULL);
		if (havelrs || (!dist && havesrs))
		    pirates = q->pirates ? '0'+q->pirates : '.';
		else
		    pirates = '?';
	    }
	    waddch (_wgalaxy, pirates);
	}
    }
}

//}}}2------------------------------------------------------------------
//{{{2 Ship stats

static void draw_ship_info (void)
{
    unsigned y = 0;
    mvwaddstr (_winfo, y++, 0, "Condition     ");
    static const char c_condname[5][8] = { "GREEN", "DOCKED", "YELLOW", "RED", "CLOAKED" };
    static const uint8_t c_color[5] = { color_PanelGreen, color_Panel, color_PanelYellow, color_PanelRed, color_Space };
    unsigned ci = Ship.cloaked ? 4 : Ship.cond;
    wattr_set (_winfo, A_BOLD, c_color[ci], NULL);
    wprintw (_winfo, "%7s", c_condname[ci]);

    wattr_set (_winfo, A_NORMAL, color_Panel, NULL);
    mvwaddstr (_winfo, y++, 0, "Shields       ");
    unsigned percent = 100 * Ship.shield / Param.shield;
    if (device_damaged (SHIELD) || percent < 25)
	wattr_set (_winfo, A_BOLD, color_PanelRed, NULL);
    else if (percent < 50)
	wattr_set (_winfo, A_BOLD, color_PanelYellow, NULL);
    if (device_damaged (SHIELD))
	waddstr (_winfo, "damaged");
    else if (!Ship.shldup)
	waddstr (_winfo, "   down");
    else if (percent == 100)
	waddstr (_winfo, "     up");
    else
	wprintw (_winfo, "up, %2u%%", percent);

    wattr_set (_winfo, A_NORMAL, color_Panel, NULL);
    mvwaddstr (_winfo, y++, 0, "Warp factor       ");
    if (Ship.warp >= 90)
	wattr_set (_winfo, A_BOLD, color_PanelRed, NULL);
    else if (Ship.warp >= 60)
	wattr_set (_winfo, A_BOLD, color_PanelYellow, NULL);
    wprintw (_winfo, "%u.%u", Ship.warp/10, Ship.warp%10);

    if (Ship.energy <= Param.energylow)
	wattr_set (_winfo, A_BOLD, c_color[RED], NULL);
    else if (Ship.energy <= 2*Param.energylow)
	wattr_set (_winfo, A_BOLD, c_color[YELLOW], NULL);
    else
	wattr_set (_winfo, A_NORMAL, color_Panel, NULL);
    mvwprintw (_winfo, y++, 0, "Total energy     %4u", Ship.energy);

    wattr_set (_winfo, A_NORMAL, color_Panel, NULL);
    mvwprintw (_winfo, y++, 0, "Torpedoes          %2u", Ship.torped);
    unsigned npirates = pirates_remaining();
    if (npirates) {
	mvwprintw (_winfo, y++, 0, "Pirates left       %2u", npirates);
	mvwprintw (_winfo, y++, 0, "Time left       %2.2f", Now.time);
    }
    mvwprintw (_winfo, y++, 0, "Date          %4.2f", Now.date);
    mvwprintw (_winfo, y++, 0, "Position        " FULLCOORD_FMT "", Ship.quad.x, Ship.quad.y, Ship.sect.x, Ship.sect.y);

    // Name of the system, if inhabited, under the quadrant scan
    const char* sysname = systemname (current_quad());
    if (sysname) {
	wattr_set (_winfo, A_BOLD, current_quad()->distressed ? color_PanelRed : color_Panel, NULL);
	mvwaddstr (_winfo, y++, 0, sysname);
    }

    if (device_damaged (LIFESUP)) {
	wattr_set (_winfo, A_BOLD, color_PanelRed, NULL);
	mvwprintw (_winfo, y++, 0, "Life support     %.2f", Ship.reserves);
    }
}
//}}}2
//}}}-------------------------------------------------------------------
//{{{ Animations

void animate_plaser (struct xy from, struct xy to)
{
    short pfg,pbg;
    pair_content (color_Panel, &pfg, &pbg);
    if (to.x == Ship.sect.x && to.y == Ship.sect.y)
	init_pair (color_Panel, COLOR_YELLOW, COLOR_RED);
    draw_screen();
    for (unsigned i = 0; i < 8; ++i) {
	erase_window (_wquadrant, color_Space);
	draw_quadrant_scan();
	wattr_set (_wquadrant, i%2 ? A_BOLD : A_NORMAL, color_Plaser, NULL);
	struct line_iterator li = make_line_iterator (from, to);
	for (;;) {
	    struct xy lastp = li.p;
	    advance_line_iterator (&li);
	    if (li.p.x >= NSECTS || li.p.y >= NSECTS || EMPTY != sector_contents (li.p.x, li.p.y))
		break;
	    const chtype c_lc[3][9] = {
		{'\\',ACS_VLINE,'/'},
		{ACS_HLINE,'+',ACS_HLINE},
		{'/',ACS_VLINE,'\\'}
	    };
	    draw_sector_char (li.p, c_lc[li.p.y-lastp.y+1][li.p.x-lastp.x+1]);
	}
	wrefresh (_wquadrant);
	delay (8);
    }
    if (to.x == Ship.sect.x && to.y == Ship.sect.y)
	init_pair (color_Panel, pfg, pbg);
    draw_screen();
}

void animate_torpedo (struct xy from, struct xy to)
{
    draw_screen();
    struct line_iterator li = make_line_iterator (from, to);
    for (;;) {
	advance_line_iterator (&li);
	if (li.p.x >= NSECTS || li.p.y >= NSECTS || EMPTY != sector_contents (li.p.x, li.p.y))
	    break;
	wattr_set (_wquadrant, A_BOLD, color_Torpedo, NULL);
	draw_sector_char (li.p, '*');
	wrefresh (_wquadrant);
	delay (12);
	wattr_set (_wquadrant, A_BOLD, color_Space, NULL);
	draw_sector_char (li.p, EMPTY);
    }
    draw_screen();
}

void animate_nova (struct xy nl)
{
    draw_screen();
    const char c_stage[] = { '.','*','*','#','#','@' };
    int stage = 1, ds = 1;
    do {
	// Nova affects neighbor sectors at stage-1
	wattr_set (_wquadrant, ds > 0 ? A_BOLD : A_NORMAL, color_Star, NULL);
	for (int dy = -1; dy <= 1; ++dy) {
	    for (int dx = -1; dx <= 1; ++dx) {
		struct xy halo = { nl.x+dx, nl.y+dy };
		if (halo.x >= NSECTS || halo.y >= NSECTS)
		    continue;
		unsigned ss = stage;
		if (dx || dy)
		    ss -= ds;	// halo is 1 stage behind
		mvwaddch (_wquadrant, halo.y, halo.x*2+1, c_stage[ss]);
		mvwaddch (_wquadrant, halo.y, halo.x*2+2, c_stage[ss]);
	    }
	}
	wrefresh (_wquadrant);
	delay (12);
	if ((stage += ds) >= (int) ArraySize(c_stage)-2)
	    ds = -1;
    } while (stage >= 0);
}

//}}}-------------------------------------------------------------------
//{{{ Message output

void print_msg (const char* msg, ...)
{
    va_list args;
    va_start (args, msg);
    vw_printw (_wmsg, msg, args);
    va_end (args);
}

static void print_help (void)
{
    print_msg (
	"Available commands:\n"
	"m: move (ss or qqss)\tr: rest\n"
	"p: fire plasers\t\tI: move under impulse\n"
	"t: fire torpedoes\tR: ram something\n"
	"c: cloak on/off\t\tC: ask pirate to surrender\n"
	"s: shield up/down\tH: call starbase for help\n"
	"w: set warp factor\tD: self-destruct\n"
	"d: dock/undock\t\tS: save and quit\n"
	"i: damage report\tQ: quick quit\n");
}

//}}}-------------------------------------------------------------------
//{{{ Score

int print_score (void)
{
    print_msg ("\n*** Your score:\n");
    int u = Game.pirates_killed;
    int t = u * Param.piratepwr / 4;
    int s = t;
    if (t)
	print_msg ("%d pirates killed\t\t\t%6d\n", u, t);
    float r = Now.date - Param.date;
    if (r < 1.0)
	r = 1.0;
    r = Game.pirates_killed / r;
    s += (t = 400 * r);
    if (t != 0)
	print_msg ("Kill rate %.2f pirates/day\t\t%6d\n", r, t);
    r = pirates_remaining();
    r /= Game.pirates_killed + 1;
    s += (t = -400 * r);
    if (t != 0)
	print_msg ("Penalty for %d pirates remaining\t%6d\n", pirates_remaining(), t);
    if (Move.endgame) {
	s += (t = 100 * (u = 2));
	print_msg ("Bonus for winning the game\t\t%6d\n", t);
    }
    if (Game.killed) {
	s -= 500;
	print_msg ("Penalty for getting killed\t\t  -500\n");
    }
    s += (t = -100 * (u = Game.bases_killed));
    if (t != 0)
	print_msg ("%d starbases killed\t\t\t%6d\n", u, t);
    s += (t = -100 * (u = Game.helps));
    if (t != 0)
	print_msg ("%d calls for help\t\t\t%6d\n", u, t);
    s += (t = -5 * (u = Game.stars_killed));
    if (t != 0)
	print_msg ("%d stars destroyed\t\t\t%6d\n", u, t);
    s += (t = -150 * (u = Game.killinhab));
    if (t != 0)
	print_msg ("%d inhabited starsystems destroyed\t%6d\n", u, t);
    s += (t = 3 * (u = Game.captives));
    if (t != 0)
	print_msg ("%d pirates captured\t\t\t%6d\n", u, t);
    s += (t = -(u = Game.deaths));
    if (t != 0)
	print_msg ("%d casualties\t\t\t\t%6d\n", u, t);
    print_msg ("\n***  TOTAL\t\t\t%14d\n", s);
    draw_screen();
    wgetch (_wmsg);
    return s;
}

//}}}-------------------------------------------------------------------
//{{{ User input

// Main command input
void main_command (void)
{
    // Main commands are read from the message window because it is always
    // visible. _wcmd is used for string input and usually is not.
    int k = wgetch (_wmsg);
    switch (k) {
	case 'm':	dowarp(0); break;
	case 'p':	plasers(); break;
	case 't':	torped(); break;
	case 'c':	cloak(); break;
	case 'w':	setwarp(); break;
	case 'd':	Ship.cond == DOCKED ? undock() : dock(); break;
	case 'i':	damage_report(); break;
	case 'I':	impulse(); break;
	case 'R':	dowarp(1); break;
	case 'r':	rest(); break;
	case 's':	shield(); break;
	case 'C':	capture(); break;
	case 'H':	help(); break;
	case 'D':	destruct(); break;
	case 'S':	save_game(); break;
	case 'Q':
	case KEY_F(10):	myreset(); break;
	case '?':
	case 'h':
	case KEY_F(1):	print_help(); break;
	case KEY_RESIZE: create_windows(); break;
    };
}

// String parameter input
void getstrpar (const char* s, char* r, unsigned l)
{
    draw_screen();
    r[0] = 0;

    curs_set (1);
    echo();

    erase_window (_wcmd, color_Space);
    wmove (_wcmd, 0, 0);

    wattr_set (_wcmd, A_BOLD, color_Ship, NULL);
    wprintw (_wcmd, " %s: ", s);

    wattr_set (_wcmd, A_NORMAL, color_Ship, NULL);
    if (KEY_RESIZE == wgetnstr (_wcmd, r, l))
	create_windows();

    noecho();
    curs_set (0);

    for (unsigned br = strlen (r); br && r[--br] == '\n';)
	r[br] = 0;

    draw_screen();
}

// Get integer parameter
int getintpar (const char* s)
{
    char buf [16];
    getstrpar (s, ArrayBlock(buf));
    return atoi (buf);
}

// Get floating parameter
float getfltpar (const char* s)
{
    char buf [16];
    getstrpar (s, ArrayBlock(buf));
    return atof (buf);
}

// get yes/no parameter
bool getynpar (const char* s)
{
    char buf [8];
    getstrpar (s, ArrayBlock(buf));
    return buf[0] == 'y';
}

// Get coded parameter
const struct cvntab* getcodpar (const char* s, const struct cvntab* tab)
{
    for (;;) {
	char buf [16] = {};
	getstrpar (s, ArrayBlock(buf));
	if (!buf[0])
	    return NULL;	// cancel command

	// Print help, if requested
	if (buf[0] == '?') {
	    int c = 4;
	    for (const struct cvntab* r = tab; r->abrev; ++r) {
		char helpbuf [32];
		strcpy (helpbuf, r->abrev);
		strcat (helpbuf, r->full);
		print_msg ("%14.14s", helpbuf);
		if (--c > 0)
		    continue;
		c = 4;
		print_msg ("\n");
	    }
	    if (c != 4)
		print_msg ("\n");
	    continue;
	}

	// Search for command in the table
	const struct cvntab* r = tab;
	for (; r->abrev; ++r) {
	    const char* p = buf;
	    const char* q = r->abrev;
	    for (; *q; ++q)
		if (*p++ != *q)
		    break;
	    if (!*q) {
		for (q = r->full; *p && *q; ++q, ++p)
		    if (*p != *q)
			break;
		if (!*p || !*q)
		    break;
	    }
	}

	// check for not found
	if (r->abrev)
	    return r;
	else
	    print_msg ("Command not recognized. Try ? for help.\n");
    }
}

// Get destination
//
// The user is asked for the destination coordinate SS or QQSS
// Return values with quad > NQUADS indicate error
//
struct xy getdest (const char* prompt)
{
    struct xy dest = { UINT8_MAX, UINT8_MAX };
    char dbuf[8] = {};
    getstrpar (prompt, ArrayBlock(dbuf)-1);
    if (!dbuf[0])
	return dest;

    unsigned dlen = 0;
    while (dlen < 5 && ((dbuf[dlen] >= '0' && dbuf[dlen] <= '9') || dbuf[dlen] == '.'))
	++dlen;
    if (dlen < 2)
	return dest;

    struct xy quad = Ship.quad, sect = {};
    const char* d = dbuf;
    if (dlen >= 4) {
	quad.x = *d++ - '0';
	quad.y = *d++ - '0';
	if (quad.x >= NQUADS || quad.y >= NQUADS)
	    return dest;
    }
    if (*d == '.')
	++d;
    sect.x = *d++ - '0';
    sect.y = *d++ - '0';
    if (sect.x >= NSECTS || sect.y >= NSECTS)
	return dest;
    return dest = absolute_sector_coord (quad, sect);
}

//}}}-------------------------------------------------------------------
