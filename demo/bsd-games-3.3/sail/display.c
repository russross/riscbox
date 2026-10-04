// Copyright (c) 1983 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "sail.h"

//----------------------------------------------------------------------

enum {
    MAX_COLS	= 80,

    BOX_T	= 0,
    BOX_L	= 0,
    BOX_X	= 65,
    BOX_Y	= 17,
    BOX_B	= BOX_T+BOX_Y-1,
    BOX_R	= BOX_L+BOX_X-1,

    STAT_T	= 0,
    STAT_B	= BOX_B,
    STAT_L	= BOX_R+2,
    STAT_X	= 14,
    STAT_Y	= STAT_B-STAT_T+1,
    STAT_R	= STAT_L+STAT_X-1,
    STAT_1	= 0,
    STAT_2	= STAT_1+4,
    STAT_3	= STAT_2+7,

    VIEW_T	= BOX_T+1,
    VIEW_L	= BOX_L+1,
    VIEW_X	= BOX_X-5,
    VIEW_Y	= BOX_Y-2,
    VIEW_B	= VIEW_T+VIEW_Y-1,
    VIEW_R	= VIEW_L+VIEW_X-1,

    SLOT_T	= VIEW_T,
    SLOT_L	= VIEW_R+1,
    SLOT_X	= 3,
    SLOT_Y	= VIEW_Y,
    SLOT_B	= VIEW_B,
    SLOT_R	= SLOT_L+SLOT_X-1
};

enum {
    color_None,
    color_Sea,
    color_Player,
    color_Friend,
    color_Enemy
};

//----------------------------------------------------------------------

static void newturn (void);
static void fmtship (char* buf, size_t len, const char* fmt, const struct Ship* ship);
static void prompt (const char* p, struct Ship* ship);
static void endprompt (void);
static unsigned ms_to_next_turn (void);
static void draw_view (void);
static void draw_stat (void);
static void draw_slot (void);
static void draw_board (void);
static void adjustview (void);

//----------------------------------------------------------------------

static char _sc_hasprompt = 0;
static const char* _sc_prompt = NULL;
static const char* _sc_buf = NULL;

static WINDOW* _view_w = NULL;
static WINDOW* _view_box_w = NULL;
static WINDOW* _slot_w = NULL;
static WINDOW* _cmd_w = NULL;
static WINDOW* _scroll_w = NULL;
static WINDOW* _stat_w = NULL;

static int _viewrow = 0;
static int _viewcol = 0;
struct Ship* _ms = NULL;	// memorial structure, &cc->ship[player]

//----------------------------------------------------------------------

void initscreen(void)
{
    initialize_curses();
    static const struct color_pair c_Pairs[] = {
	{ COLOR_WHITE,		COLOR_BLUE	},	// color_Sea
	{ COLOR_YELLOW,		COLOR_BLUE	},	// color_Player
	{ COLOR_GREEN,		COLOR_BLUE	},	// color_Friend
	{ COLOR_MAGENTA,	COLOR_BLUE	}	// color_Enemy
    };
    init_pairs (ArrayBlock (c_Pairs));

    unsigned scrw = min_u (COLS, MAX_COLS);
    unsigned scrx = (COLS-scrw)/2;
    _stat_w = newwin (STAT_Y, STAT_X, LINES-STAT_Y, scrx+scrw-STAT_X);
    _view_box_w = newwin (getmaxy(_stat_w), scrw-getmaxx(_stat_w), getbegy(_stat_w), scrx);
    _view_w = newwin (getmaxy(_view_box_w)-2, getmaxx(_view_box_w)-2, getbegy(_view_box_w)+1, getbegx(_view_box_w)+1);
    _slot_w = newwin (getmaxy(_view_w), SLOT_X, getbegy(_view_w), getbegx(_view_w)+getmaxx(_view_w)-SLOT_X);
    _cmd_w = newwin (1, scrw, getbegy(_view_box_w)-1, scrx);
    _scroll_w = newwin (getbegy(_cmd_w), scrw, 0, scrx);

    wbkgdset (_view_box_w, COLOR_PAIR(color_Sea));
    wbkgdset (_view_w, COLOR_PAIR(color_Sea));
    wbkgdset (_slot_w, COLOR_PAIR(color_Sea));
    keypad (_cmd_w, true);
    scrollok (_scroll_w, true);
    wmove (_scroll_w, getmaxy(_scroll_w)-1, 0);
}

static void newturn (void)
{
    // Process AI ships first
    dr_turn();

    _repaired = _loaded = _fired = _changed = 0;
    _ms->status.movebuf[0] = 0;

    if (_ms->status.ready.l & R_LOADING) {
	if (_ms->status.ready.l & R_DOUBLE)
	    _ms->status.ready.l = R_LOADING;
	else
	    _ms->status.ready.l = R_LOADED;
    }
    if (_ms->status.ready.r & R_LOADING) {
	if (_ms->status.ready.r & R_DOUBLE)
	    _ms->status.ready.r = R_LOADING;
	else
	    _ms->status.ready.r = R_LOADED;
    }
    if (_sc_hasprompt) {
	werase (_cmd_w);
	wprintw(_cmd_w, "%s%s", _sc_prompt, _sc_buf);
    }

    if (_ms->status.FS && (!_ms->specs.rig[0] || _windspeed == 6))
	_ms->status.FS = 0;
    if (_ms->status.FS == 1)
	_ms->status.FS = 2;

    if (_ms->status.struck)
	leave(LEAVE_QUIT);
    if (_ms->status.captured)
	leave(LEAVE_CAPTURED);
    if (_windspeed == 7)
	leave(LEAVE_HURRICAN);

    adjustview();
    draw_screen();
}

static void fmtship (char *buf, size_t len, const char *fmt, const struct Ship* ship)
{
    while (*fmt) {
	if (len-- == 0) {
	    *buf = '\0';
	    return;
	}
	if (*fmt == '$' && fmt[1] == '$') {
	    size_t l = snprintf (buf, len, "%s (%c%c)", ship->shipname, colours(ship), sterncolour(ship));
	    buf += l;
	    len -= l - 1;
	    fmt += 2;
	} else
	    *buf++ = *fmt++;
    }

    if (len > 0)
	*buf = '\0';
}

void makesignal (const struct Ship* from, const char* fmt, const struct Ship* ship, ...)
{
    char format [128];
    fmtship (format, sizeof(format), fmt, ship);
    va_list ap;
    va_start (ap, ship);
    char message [128];
    vsnprintf (ArrayBlock(message), format, ap);
    va_end (ap);
    Signal ("$$: %s", from, message);
}

void makemsg (const struct Ship* from, const char* fmt, ...)
{
    va_list ap;
    va_start (ap, fmt);
    char message [128];
    vsnprintf (ArrayBlock(message), fmt, ap);
    va_end (ap);
    Signal ("$$: %s", from, message);
}

void Signal (const char* fmt, const struct Ship* ship, ...)
{
    if (!_scroll_w)
	return;
    va_list ap;
    va_start(ap, ship);
    char msgbuf [256];
    fmtship (ArrayBlock(msgbuf), fmt, ship);
    vw_printw (_scroll_w, msgbuf, ap);
    waddch (_scroll_w, '\n');
    va_end (ap);
}

void Msg (const char* fmt, ...)
{
    va_list ap;
    va_start (ap, fmt);
    char message [128];
    vsnprintf (ArrayBlock(message), fmt, ap);
    va_end (ap);
    Signal ("%s", _ms, message);
}

static void prompt(const char *p, struct Ship *ship)
{
    static char buf [64];
    fmtship (buf, sizeof(buf), p, ship);
    _sc_prompt = buf;
    _sc_buf = "";
    _sc_hasprompt = 1;
    waddstr (_cmd_w, buf);
}

static void endprompt (void)
{
    werase (_cmd_w);
    _sc_hasprompt = 0;
}

static unsigned ms_to_next_turn (void)
{
    // Setup wait time to have turns occuring every 7 seconds
    enum { TURN_TIME = 7000 };
    static uint64_t s_NextTurnTime = 0;
    uint64_t now = time_ms();
    if (s_NextTurnTime <= now) {
	s_NextTurnTime = Align (now+1, TURN_TIME);
	newturn();
    }
    return s_NextTurnTime - now;
}

int sgetch (const char *p, struct Ship *ship, int flag)
{
    prompt (p, ship);
    int c = ERR;
    do {
	wtimeout (_cmd_w, ms_to_next_turn());
	c = wgetch (_cmd_w);
	if (flag && c >= ' ' && c <= '~')
	    waddch (_cmd_w, c);
	ms_to_next_turn();
    } while (c == ERR);
    endprompt();
    return c;
}

void sgetstr (const char *pr, char *buf, int n)
{
    int c;
    char *p = buf;

    prompt(pr, NULL);
    _sc_buf = buf;
    for (;;) {
	*p = 0;
	wtimeout (_cmd_w, ms_to_next_turn());
	c = wgetch (_cmd_w);
	ms_to_next_turn();
	switch (c) {
	    case '\n':
	    case '\r':
		endprompt();
		return;
	    case '\b':
		if (p > buf) {
		    waddstr(_cmd_w, "\b \b");
		    --p;
		}
		break;
	    default:
		if (c >= ' ' && c < 0x7f && p < buf + n - 1) {
		    *p++ = c;
		    waddch(_cmd_w, c);
		} else
		    putchar('\a');
	}
    }
}

_Noreturn void leave (int conditions)
{
    Msg("It looks like you've had it!");
    switch (conditions) {
	case LEAVE_QUIT:	break;
	case LEAVE_CAPTURED:	Msg("Your ship was captured."); break;
	case LEAVE_HURRICAN:	Msg("Hurricane!  All ships destroyed."); break;
	default:		Msg("A funny thing happened (%d).", conditions);
    }
    draw_screen();
    wtimeout (_cmd_w, 5000);
    wgetch (_cmd_w);
    exit (EXIT_SUCCESS);
}

void draw_screen(void)
{
    draw_board();
    draw_view();
    draw_stat();
    draw_slot();
    wnoutrefresh(_scroll_w);
    wrefresh (_cmd_w);
}

static void draw_view(void)
{
    werase (_view_w);
    foreachship(sp) {
	if (sp->status.dir
		&& sp->status.row > _viewrow
		&& sp->status.row < _viewrow + VIEW_Y
		&& sp->status.col > _viewcol
		&& sp->status.col < _viewcol + VIEW_X) {
	    if (sp->status.index == _player)
		wattr_set (_view_w, A_BOLD, color_Player, NULL);
	    else if (shipnationality(sp) == _ms->nationality)
		wattr_set (_view_w, A_BOLD, color_Friend, NULL);
	    else
		wattr_set (_view_w, A_BOLD, color_Enemy, NULL);
	    mvwaddch (_view_w, sp->status.row - _viewrow, sp->status.col - _viewcol, colours(sp));
	    mvwaddch (_view_w, sternrow(sp) - _viewrow, sterncol(sp) - _viewcol, sterncolour(sp));
	    wattr_set (_view_w, A_NORMAL, color_None, NULL);
	}
    }
    wnoutrefresh (_view_w);
}

static void draw_stat(void)
{
    werase (_stat_w);
    wmove(_stat_w, STAT_1, 0);
    wprintw(_stat_w, "Turn    %c%2d\n", _dont_adjust ? '*' : '-', _turn);
    wprintw(_stat_w, "Fouls    %2d\n", fouled(_ms));
    wprintw(_stat_w, "Grapples %2d\n", grappled(_ms));

    wmove(_stat_w, STAT_2, 0);
    wprintw(_stat_w, "    0 %c(%c)\n", maxmove(_ms, _winddir + 3, -1) + '0', maxmove(_ms, _winddir + 3, 1) + '0');
    waddstr(_stat_w, "   \\|/\n");
    wprintw(_stat_w, "   -^-%c(%c)\n", maxmove(_ms, _winddir + 2, -1) + '0', maxmove(_ms, _winddir + 2, 1) + '0');
    waddstr(_stat_w, "   /|\\\n");
    wprintw(_stat_w, "    | %c(%c)\n", maxmove(_ms, _winddir + 1, -1) + '0', maxmove(_ms, _winddir + 1, 1) + '0');
    wprintw(_stat_w, "   %c(%c)\n", maxmove(_ms, _winddir, -1) + '0', maxmove(_ms, _winddir, 1) + '0');

    wmove(_stat_w, STAT_3, 0);
    wprintw(_stat_w, "Load  %c%c %c%c\n", c_LoadLetter[_ms->status.load.l], readyname(_ms->status.ready.l), c_LoadLetter[_ms->status.load.r], readyname(_ms->status.ready.r));
    wprintw(_stat_w, "Hull %2d\n", _ms->specs.hull);
    wprintw(_stat_w, "Crew %2d %2d %2d\n", _ms->specs.crew[0], _ms->specs.crew[1], _ms->specs.crew[2]);
    wprintw(_stat_w, "Guns %2d %2d\n", _ms->specs.gun.l, _ms->specs.gun.r);
    wprintw(_stat_w, "Carr %2d %2d\n", _ms->specs.car.l, _ms->specs.car.r);
    wprintw(_stat_w, "Rigg %d %d %d ", _ms->specs.rig[0], _ms->specs.rig[1], _ms->specs.rig[2]);
    if (_ms->specs.rig[3] < 0)
	waddch(_stat_w, '-');
    else
	wprintw(_stat_w, "%d", _ms->specs.rig[3]);
    wnoutrefresh (_stat_w);
}

static void draw_slot(void)
{
    werase (_slot_w);
    if (!boarding(_ms, 0)) {
	mvwaddstr(_slot_w, 0, 0, "   ");
	mvwaddstr(_slot_w, 1, 0, "   ");
    } else
	mvwaddstr(_slot_w, 1, 0, "OBP");
    if (!boarding(_ms, 1)) {
	mvwaddstr(_slot_w, 2, 0, "   ");
	mvwaddstr(_slot_w, 3, 0, "   ");
    } else
	mvwaddstr(_slot_w, 3, 0, "DBP");

    wmove(_slot_w, SLOT_Y - 4, 0);
    if (_ms->status.RH)
	wprintw(_slot_w, "%dRH", _ms->status.RH);
    else
	waddstr(_slot_w, "   ");
    wmove(_slot_w, SLOT_Y - 3, 0);
    if (_ms->status.RG)
	wprintw(_slot_w, "%dRG", _ms->status.RG);
    else
	waddstr(_slot_w, "   ");
    wmove(_slot_w, SLOT_Y - 2, 0);
    if (_ms->status.RR)
	wprintw(_slot_w, "%dRR", _ms->status.RR);
    else
	waddstr(_slot_w, "   ");

    enum { Y = SLOT_Y/2 };
    wmove(_slot_w, 7, 1);
    wprintw(_slot_w, "%d", _windspeed);
    mvwaddch(_slot_w, Y, 0, ' ');
    mvwaddch(_slot_w, Y, 2, ' ');
    mvwaddch(_slot_w, Y - 1, 0, ' ');
    mvwaddch(_slot_w, Y - 1, 1, ' ');
    mvwaddch(_slot_w, Y - 1, 2, ' ');
    mvwaddch(_slot_w, Y + 1, 0, ' ');
    mvwaddch(_slot_w, Y + 1, 1, ' ');
    mvwaddch(_slot_w, Y + 1, 2, ' ');
    wmove(_slot_w, Y - c_dr[_winddir], 1 - c_dc[_winddir]);
    switch (_winddir) {
	case 1:
	case 5:
	    waddch(_slot_w, '|');
	    break;
	case 2:
	case 6:
	    waddch(_slot_w, '/');
	    break;
	case 3:
	case 7:
	    waddch(_slot_w, '-');
	    break;
	case 4:
	case 8:
	    waddch(_slot_w, '\\');
	    break;
    }
    mvwaddch(_slot_w, Y + c_dr[_winddir], 1 + c_dc[_winddir], '+');
    wnoutrefresh(_slot_w);
}

static void draw_board(void)
{
    werase (_view_box_w);
    box (_view_box_w, 0, 0);
    mvwprintw (_view_box_w, 0, 2, "Class %d %s (%d guns) '%s' (%c%c)", _ms->specs.sclass, c_ClassName[_ms->specs.sclass], _ms->specs.guns, _ms->shipname, colours(_ms), sterncolour(_ms));
    mvwaddstr (_view_box_w, getmaxy(_view_box_w)-1, (getmaxx(_view_box_w) - strlen(_cc->name)) / 2, _cc->name);
    wnoutrefresh (_view_box_w);
}

void centerview (void)
{
    _viewrow = _ms->status.row - VIEW_Y / 2;
    _viewcol = _ms->status.col - VIEW_X / 2;
}

void upview (void)
{
    _viewrow -= VIEW_Y / 3;
}

void downview (void)
{
    _viewrow += VIEW_Y / 3;
}

void leftview (void)
{
    _viewcol -= VIEW_X / 5;
}

void rightview (void)
{
    _viewcol += VIEW_X / 5;
}

static void adjustview (void)
{
    if (_dont_adjust)
	return;
    if (_ms->status.row < _viewrow + VIEW_Y / 4)
	_viewrow = _ms->status.row - (VIEW_Y - VIEW_Y / 4);
    else if (_ms->status.row > _viewrow + (VIEW_Y - VIEW_Y / 4))
	_viewrow = _ms->status.row - VIEW_Y / 4;
    if (_ms->status.col < _viewcol + VIEW_X / 8)
	_viewcol = _ms->status.col - (VIEW_X - VIEW_X / 8);
    else if (_ms->status.col > _viewcol + (VIEW_X - VIEW_X / 8))
	_viewcol = _ms->status.col - VIEW_X / 8;
}

void parties(struct Ship *to, int *crew, int isdefense, int buf)
{
    int k, j, men;
    struct BP *ptr;
    int temp[3];

    for (k = 0; k < 3; ++k)
	temp[k] = crew[k];
    if (isdigit(buf)) {
	ptr = isdefense ? to->status.DBP : to->status.OBP;
	for (j = 0; j < NBP && ptr[j].turnsent; ++j);
	if (!ptr[j].turnsent && buf > '0') {
	    men = 0;
	    for (k = 0; k < 3 && buf > '0'; ++k) {
		men += crew[k] * (k == 0 ? 100 : (k == 1 ? 10 : 1));
		crew[k] = 0;
		if (men)
		    --buf;
	    }
	    if (buf > '0')
		Msg("Sending all crew sections.");
	    if (isdefense) {
		ship_set_defensive_parties (_ms, j, _turn, to->status.index, men);
		wmove(_slot_w, 2, 0);
		for (k = 0; k < NBP; ++k)
		    if (temp[k] && !crew[k])
			waddch(_slot_w, k + '1');
		    else
			wmove(_slot_w, 2, 1 + k);
		mvwaddstr(_slot_w, 3, 0, "DBP");
		makemsg(_ms, "repelling boarders");
	    } else {
		ship_set_offensive_parties (_ms, j, _turn, to->status.index, men);
		wmove(_slot_w, 0, 0);
		for (k = 0; k < NBP; ++k)
		    if (temp[k] && !crew[k])
			waddch(_slot_w, k + '1');
		    else
			wmove(_slot_w, 0, 1 + k);
		mvwaddstr(_slot_w, 1, 0, "OBP");
		makesignal(_ms, "boarding the $$", to);
	    }
	} else
	    Msg("Sending no crew sections.");
    }
}

struct ScenarioSelector select_scenario (void)
{
    struct ScenarioSelector r = { 0, 0, {L_ROUND, L_ROUND}};

    unsigned ww = min_u (COLS, 64);
    unsigned wh = min_u (LINES, 24);

    // Layout selector boxes
    //
    //            SAIL
    // +--------------------------+
    // | scen | ship list         |
    // | list +---------+---------|
    // |      |  lload  |  rload  |
    // +--------------------------+
    //
    WINDOW* wtitle = newwin (9, ww, LINES-wh, (COLS-ww)/2);
    WINDOW* wscen = newwin (wh-getmaxy(wtitle), 40, getbegy(wtitle)+getmaxy(wtitle), getbegx(wtitle));
    WINDOW* wlload = newwin (3, (ww-getmaxx(wscen))/2, LINES-3, getbegx(wscen)+getmaxx(wscen));
    WINDOW* wrload = newwin (getmaxy(wlload), getmaxx(wlload), getbegy(wlload), getbegx(wlload)+getmaxx(wlload));
    WINDOW* wship = newwin (getmaxy(wscen)-getmaxy(wlload), ww-getmaxx(wscen), getbegy(wscen), getbegx(wlload));
    wbkgdset (wtitle, COLOR_PAIR(1));
    wbkgdset (wscen, COLOR_PAIR(1));
    wbkgdset (wship, COLOR_PAIR(1));
    wbkgdset (wlload, COLOR_PAIR(1));
    wbkgdset (wrload, COLOR_PAIR(1));

    keypad (wtitle, true);

    enum {
	FOCUS_SCENARIOS,
	FOCUS_SHIPS,
	FOCUS_LEFTLOAD,
	FOCUS_RIGHTLOAD,
	FOCUS_N
    };
    unsigned focus = FOCUS_SCENARIOS, scentop = 0, scenpage = getmaxy(wscen)-2;
    for (;;) {
	// Draw selection screen
	werase (wtitle);
	werase (wscen);
	werase (wship);
	werase (wlload);
	werase (wrload);

	static const char c_Title[7][32] = {
	    " ####    ####   ######  ##    ",
	    "##  ##  ##  ##    ##    ##    ",
	    "##      ##  ##    ##    ##    ",
	    " ####   ######    ##    ##    ",
	    "    ##  ##  ##    ##    ##    ",
	    "##  ##  ##  ##    ##    ##    ",
	    " ####   ##  ##  ######  ######"
	};
	unsigned tbx = (getmaxx(wtitle)-strlen(c_Title[0]))/2;
	unsigned tby = (getmaxy(wtitle)-ArraySize(c_Title))/2;
	for (unsigned y = 0; y < ArraySize(c_Title); ++y)
	    for (unsigned x = 0; x < strlen(c_Title[0]); ++x)
		if (c_Title[y][x] != ' ')
		    mvwaddch (wtitle, y+tby, x+tbx, ACS_CKBOARD);

	box (wscen, 0, 0);
	for (unsigned i = scentop; i < min_u (NSCENE, scentop+scenpage); ++i) {
	    if (i == r.scenario)
		wattr_on (wscen, focus == FOCUS_SCENARIOS ? A_REVERSE : A_BOLD, NULL);
	    mvwhline (wscen, i-scentop+1,1, ' ', getmaxx(wscen)-2);
	    mvwaddstr (wscen, i-scentop+1,2, _scenarios[i].name);
	    wattr_set (wscen, A_NORMAL, color_None, NULL);
	}

	box (wship, 0, 0);
	for (unsigned i = 0; i < min_u (_scenarios[r.scenario].vessels, getmaxy(wship)-2); ++i) {
	    if (i == r.ship)
		wattr_on (wship, focus == FOCUS_SHIPS ? A_REVERSE : A_BOLD, NULL);
	    mvwhline (wship, i+1,1, ' ', getmaxx(wship)-2);
	    mvwprintw (wship, i+1,2, "%c %s", c_CountryName[_scenarios[r.scenario].ship[i].nationality][0], _scenarios[r.scenario].ship[i].shipname);
	    wattr_set (wship, A_NORMAL, color_None, NULL);
	}

	wattr_set (wlload, A_NORMAL, color_None, NULL);
	box (wlload, 0, 0);
	static const char c_LoadName[4][8] = { "Grape", "Chain", "Round", "Double" };
	if (focus == FOCUS_LEFTLOAD)
	    wattr_on (wlload, A_REVERSE, NULL);
	mvwaddstr (wlload, 1,2, c_LoadName[r.iniload.l-1]);

	wattr_set (wrload, A_NORMAL, color_None, NULL);
	box (wrload, 0, 0);
	if (focus == FOCUS_RIGHTLOAD)
	    wattr_on (wrload, A_REVERSE, NULL);
	mvwaddstr (wrload, 1,2, c_LoadName[r.iniload.r-1]);

	wnoutrefresh (wtitle);
	wnoutrefresh (wscen);
	wnoutrefresh (wship);
	wnoutrefresh (wlload);
	wrefresh (wrload);

	// Process selection commands
	int k = wgetch (wtitle);
	if (k == 'k' || k == KEY_UP) {
	    if (focus == FOCUS_SCENARIOS && r.scenario > 0)
		--r.scenario;
	    else if (focus == FOCUS_SHIPS && r.ship > 0)
		--r.ship;
	    else if (focus == FOCUS_LEFTLOAD && r.iniload.l > L_GRAPE)
		--r.iniload.l;
	    else if (focus == FOCUS_RIGHTLOAD && r.iniload.r > L_GRAPE)
		--r.iniload.r;
	} else if (k == 'j' || k == KEY_DOWN) {
	    if (focus == FOCUS_SCENARIOS && r.scenario < NSCENE-1)
		++r.scenario;
	    else if (focus == FOCUS_SHIPS && r.ship < _scenarios[r.scenario].vessels-1)
		++r.ship;
	    else if (focus == FOCUS_LEFTLOAD && r.iniload.l < L_DOUBLE)
		++r.iniload.l;
	    else if (focus == FOCUS_RIGHTLOAD && r.iniload.r < L_DOUBLE)
		++r.iniload.r;
	} else if (k == KEY_HOME) {
	    if (focus == FOCUS_SCENARIOS)
		r.scenario = 0;
	    else if (focus == FOCUS_SHIPS)
		r.ship = 0;
	    else if (focus == FOCUS_LEFTLOAD)
		r.iniload.l = L_ROUND;
	    else if (focus == FOCUS_RIGHTLOAD)
		r.iniload.r = L_ROUND;
	} else if (k == KEY_END) {
	    if (focus == FOCUS_SCENARIOS)
		r.scenario = NSCENE-1;
	    else if (focus == FOCUS_SHIPS)
		r.ship = _scenarios[r.scenario].vessels-1;
	} else if (k == KEY_NPAGE && focus == FOCUS_SCENARIOS) {
	    r.scenario = min_u (r.scenario + scenpage, NSCENE-1);
	} else if (k == KEY_PPAGE && focus == FOCUS_SCENARIOS) {
	    if (r.scenario > scenpage)
		r.scenario -= scenpage;
	    else
		r.scenario = 0;
	} else if (k == '\t' || k == ' ')
	    focus = (focus+1) % FOCUS_N;
	else if (k == KEY_BACKSPACE)
	    focus = (focus-1) % FOCUS_N;
	else if (k == 'q' || k == KEY_ESCAPE) {
	    r.scenario = NSCENE;
	    break;
	} else if (k == KEY_ENTER || k == '\n')
	    break;

	if (r.ship > _scenarios[r.scenario].vessels-1)
	    r.ship = 0;	// when switching to a scenario with fewer ships
	if (r.scenario < scentop)
	    scentop = r.scenario;
	if (r.scenario >= scentop + scenpage - 1)
	    scentop = r.scenario - (scenpage - 1);
    }
    delwin (wtitle);
    delwin (wscen);
    delwin (wship);
    delwin (wlload);
    delwin (wrload);
    erase();
    refresh();
    return r;
}
