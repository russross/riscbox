// Copyright (c) 1987 by Ed James <edjames@berkeley.edu>
// This file is free software, distributed under the BSD license.

#include "atc.h"

//{{{ Constants and types ----------------------------------------------

enum {
    MAXRULES	= 6,
    MAXDEPTH	= 15,
    HELPTOKEN	= '?',
    ALPHATOKEN	= 250,
    NUMTOKEN,
    BACKTOKEN
};

enum ERelDir {
    D_LEFT,
    D_RIGHT,
    D_UP,
    D_DOWN
};

struct Rule {
    uint16_t	token;
    int8_t	to_state;
    uint8_t	handler;
    const char	str [12];
};

struct State {
    size_t		num_rules;
    const struct Rule*	rule;
};

struct Stack {
    char	str [16];
    int		state;
    unsigned	rule;
    unsigned	ch;
    unsigned	pos;
};

enum EFHandler {
    HF_NONE,
    HF_SETPLANE,
    HF_TURN,
    HF_CIRCLE,
    HF_LEFT,
    HF_RIGHT,
    HF_HARD_LEFT,
    HF_HARD_RIGHT,
    HF_DELAYB,
    HF_BEACON,
    HF_EXIT,
    HF_AIRPORT,
    HF_CLIMB,
    HF_DESCEND,
    HF_SETALT,
    HF_SETRELALT,
    HF_BENUM,
    HF_TO_DIR,
    HF_REL_DIR,
    HF_MARK,
    HF_UNMARK,
    HF_IGNORE,
    HF_NHANDLERS,
    HF_QUIT
};

#define T_RULE		_stack[_level].rule
#define T_STATE		_stack[_level].state
#define T_STR		_stack[_level].str
#define T_POS		_stack[_level].pos
#define	T_CH		_stack[_level].ch

//}}}-------------------------------------------------------------------
//{{{ Local variables

static struct Plane _p = {};
static struct Stack _stack [MAXDEPTH] = {{}};
static unsigned _level = 0;
static unsigned _tval = 0;
static unsigned _dest_type = 0;
static unsigned _dest_no = 0;
static enum ERelDir _dir = D_LEFT;

char _cmdtext [64] = "";
char _errtext [64] = "";

//}}}-------------------------------------------------------------------
//{{{ Local functions

static void rezero (void);
static void draw_command_stack (void);
static void push (unsigned ruleno, unsigned ch);
static unsigned gettoken (void);
static unsigned plane_number (char l);
static unsigned dir_from_dxdy (int dx, int dy);
static unsigned dir_no (char c);
static const char* call_handler (enum EFHandler fhi, char c);

static const char* cmd_setplane(char c);
static const char* cmd_turn(void);
static const char* cmd_circle(void);
static const char* cmd_left(void);
static const char* cmd_right(void);
static const char* cmd_Left(void);
static const char* cmd_Right(void);
static const char* cmd_delayb(char c);
static const char* cmd_beacon(void);
static const char* cmd_exit(void);
static const char* cmd_airport(void);
static const char* cmd_climb(void);
static const char* cmd_descend(void);
static const char* cmd_setalt(char c);
static const char* cmd_setrelalt(char c);
static const char* cmd_benum(char c);
static const char* cmd_to_dir(char c);
static const char* cmd_rel_dir(char c);
static const char* cmd_mark(void);
static const char* cmd_unmark(void);
static const char* cmd_ignore(void);

//}}}-------------------------------------------------------------------
//{{{ Rule table
//
// This table maps keys to handler functions (see call_handler) for each
// state. The states are the stages in building a complex command.
//
static const struct Rule
_state0[] = {	{ ALPHATOKEN,	1,	HF_SETPLANE,	"%c:"		},
		{ 'Q',		0,	HF_QUIT,	"quit"		},
		{ ' ',		-1,	HF_NONE,	""		},
		{ '\n',		-1,	HF_NONE,	""		},
		{ HELPTOKEN,	12,	HF_NONE,	" [a-z]Q\\n"	}},
_state1[] = {	{ 't',		2,	HF_TURN,	" turn"		},
		{ 'a',		3,	HF_NONE,	" altitude:"	},
		{ 'c',		4,	HF_CIRCLE,	" circle"	},
		{ 'm',		7,	HF_MARK,	" mark"		},
		{ 'u',		7,	HF_UNMARK,	" unmark"	},
		{ 'i',		7,	HF_IGNORE,	" ignore"	},
		{ HELPTOKEN,	12,	HF_NONE,	" tacmui"	}},
_state2[] = {	{ 'l',		6,	HF_LEFT,	" left"		},
		{ 'r',		6,	HF_RIGHT,	" right"	},
		{ 'L',		4,	HF_HARD_LEFT,	" left 90"	},
		{ 'R',		4,	HF_HARD_RIGHT,	" right 90"	},
		{ 't',		11,	HF_NONE,	" towards"	},
		{ 'w',		4,	HF_TO_DIR,	" to 0"		},
		{ 'e',		4,	HF_TO_DIR,	" to 45"	},
		{ 'd',		4,	HF_TO_DIR,	" to 90"	},
		{ 'c',		4,	HF_TO_DIR,	" to 135"	},
		{ 'x',		4,	HF_TO_DIR,	" to 180"	},
		{ 'z',		4,	HF_TO_DIR,	" to 225"	},
		{ 'a',		4,	HF_TO_DIR,	" to 270"	},
		{ 'q',		4,	HF_TO_DIR,	" to 315"	},
		{ HELPTOKEN,	12,	HF_NONE,	" lrLRt\\n"	}},
_state3[] = {	{ '+',		10,	HF_CLIMB,	" climb"	},
		{ 'c',		10,	HF_CLIMB,	" climb"	},
		{ '-',		10,	HF_DESCEND,	" descend"	},
		{ 'd',		10,	HF_DESCEND,	" descend"	},
		{ NUMTOKEN,	7,	HF_SETALT,	" %c000 feet"	},
		{ HELPTOKEN,	12,	HF_NONE,	" +-cd[0-9]"	}},
_state4[] = {	{ '@',		9,	HF_NONE,	" at"		},
		{ 'a',		9,	HF_NONE,	" at"		},
		{ '\n',		-1,	HF_NONE,	""		},
		{ HELPTOKEN,	12,	HF_NONE,	" @a\\n"	}},
_state5[] = {	{ NUMTOKEN,	7,	HF_DELAYB,	"%c"		},
		{ HELPTOKEN,	12,	HF_NONE,	" [0-9]"	}},
_state6[] = {	{ '@',		9,	HF_NONE,	" at"		},
		{ 'a',		9,	HF_NONE,	" at"		},
		{ 'w',		4,	HF_REL_DIR,	" 0"		},
		{ 'e',		4,	HF_REL_DIR,	" 45"		},
		{ 'd',		4,	HF_REL_DIR,	" 90"		},
		{ 'c',		4,	HF_REL_DIR,	" 135"		},
		{ 'x',		4,	HF_REL_DIR,	" 180"		},
		{ 'z',		4,	HF_REL_DIR,	" 225"		},
		{ 'a',		4,	HF_REL_DIR,	" 270"		},
		{ 'q',		4,	HF_REL_DIR,	" 315"		},
		{ '\n',		-1,	HF_NONE,	""		},
		{ HELPTOKEN,	12,	HF_NONE,	" @a<dir>\\n"	}},
_state7[] = {	{ '\n',		-1,	HF_NONE,	""		},
		{ HELPTOKEN,	12,	HF_NONE,	" \\n"		}},
_state8[] = {	{ NUMTOKEN,	4,	HF_BENUM,	"%c"		},
		{ HELPTOKEN,	12,	HF_NONE,	" [0-9]"	}},
_state9[] = {	{ 'b',		5,	HF_NONE,	" beacon #"	},
		{ '*',		5,	HF_NONE,	" beacon #"	},
		{ HELPTOKEN,	12,	HF_NONE,	" b*"		}},
_state10[] = {	{ NUMTOKEN,	7,	HF_SETRELALT,	" %c000 ft"	},
		{ HELPTOKEN,	12,	HF_NONE,	" [0-9]"	}},
_state11[] = {	{ 'b',		8,	HF_BEACON,	" beacon #"	},
		{ '*',		8,	HF_BEACON,	" beacon #"	},
		{ 'e',		8,	HF_EXIT,	" exit #"	},
		{ 'a',		8,	HF_AIRPORT,	" airport #"	},
		{ HELPTOKEN,	12,	HF_NONE,	" b*ea"		}},
_state12[] = {	{ -1,		-1,	HF_NONE,	""		}};

#define DEF_STATE(s)	{ ArraySize(s),	(s)	}
static const struct State _st[] = {
    DEF_STATE(_state0), DEF_STATE(_state1), DEF_STATE(_state2),
    DEF_STATE(_state3), DEF_STATE(_state4), DEF_STATE(_state5),
    DEF_STATE(_state6), DEF_STATE(_state7), DEF_STATE(_state8),
    DEF_STATE(_state9), DEF_STATE(_state10), DEF_STATE(_state11),
    DEF_STATE(_state12)
};

//}}}-------------------------------------------------------------------

void getcommand (void)
{
    rezero();
    do {
	draw_command_stack();
	unsigned c = gettoken();
	if (c == BACKTOKEN && _level)
	    --_level;
	else {
	    for (size_t i = 0; i < _st[T_STATE].num_rules; ++i) {
		if (_st[T_STATE].rule[i].token == c || _st[T_STATE].rule[i].token == _tval) {
		    push (i, c >= ALPHATOKEN ? _tval : c);
		    break;
		}
	    }
	}
    } while (T_STATE != -1);

    if (_level == 1) {
	update_planes();
	return;
    }

    _dest_type = T_NODEST;
    for (unsigned i = 0; i < _level; ++i) {
	enum EFHandler fhi = _st[_stack[i].state].rule[_stack[i].rule].handler;
	if (fhi != HF_NONE) {
	    const char* errmsg = call_handler (fhi, _stack[i].ch);
	    if (errmsg) {
		snprintf (ArrayBlock(_errtext), "%s", errmsg);
		return;
	    }
	}
    }

    struct Plane* pp = findplane(_p.plane_no);
    if (pp->new_altitude != _p.new_altitude)
	pp->new_altitude = _p.new_altitude;
    else if (pp->status != _p.status)
	pp->status = _p.status;
    else {
	pp->new_dir = _p.new_dir;
	pp->delayd = _p.delayd;
	pp->delayd_no = _p.delayd_no;
    }
}

static unsigned gettoken(void)
{
    _tval = getAChar();
    if (_tval >= '0' && _tval <= '9')
	return NUMTOKEN;
    else if (_tval >= 'a' && _tval <= 'a'+MAXPLANES)
	return ALPHATOKEN;
    else if (_tval == KEY_ESCAPE || _tval == KEY_BACKSPACE)
	return BACKTOKEN;
    else
	return _tval;
}

static void rezero(void)
{
    _level = 0;
    memset (_stack, 0, sizeof(_stack));
    T_RULE = -1;
    T_CH = -1;
}

static void draw_command_stack (void)
{
    memset (_cmdtext, 0, sizeof(_cmdtext));
    char* cmdb = _cmdtext;
    int cmdbsz = ArraySize(_cmdtext);
    for (unsigned i = 0; i < _level && cmdbsz > 1; ++i) {
	ssize_t ncp = snprintf (cmdb, cmdbsz, "%s", _stack[i].str);
	if (ncp <= 0)
	    break;
	cmdb += ncp;
	cmdbsz -= ncp;
    }
}

static void push (unsigned ruleno, unsigned ch)
{
    snprintf (T_STR, sizeof(T_STR), _st[T_STATE].rule[ruleno].str, _tval);
    T_RULE = ruleno;
    T_CH = ch;
    int newstate = _st[T_STATE].rule[ruleno].to_state;
    unsigned newpos = T_POS + strlen(T_STR);
    ++_level;	// this changes T_ macro address
    T_STATE = newstate;
    T_POS = newpos;
    T_RULE = -1;
    strncpy (T_STR, "", sizeof(T_STR));
    memset (_errtext, 0, sizeof(_errtext));
}

static unsigned plane_number (char l)
{
    if (l < 'a' && l > 'z' && l < 'A' && l > 'Z')
	return MAXPLANES;
    else if (l >= 'a' && l <= 'z')
	return l - 'a';
    else
	return l - 'A';
}

static unsigned dir_from_dxdy (int dx, int dy)
{
    unsigned mind = UINT_MAX, mdir = DIR_NORTH;
    for (unsigned i = 0; i < MAXDIR; ++i) {
	unsigned d = square(dx - _displacement[i].dx) + square(dy - _displacement[i].dy);
	if (mind > d) {
	    mind = d;
	    mdir = i;
	}
    }
    return mdir;
}

static unsigned dir_no (char c)
{
    static const char c_DirChars [MAXDIR+1] = "wedcxzaq";
    const char* f = strchr (c_DirChars, c);
    return f ? f - c_DirChars : MAXDIR;
}

//{{{ State handlers ---------------------------------------------------

static const char* call_handler (enum EFHandler fhi, char c)
{
    switch (fhi) {
	default:		return NULL;
	case HF_SETPLANE:	return cmd_setplane (c);
	case HF_TURN:		return cmd_turn();
	case HF_CIRCLE:		return cmd_circle();
	case HF_LEFT:		return cmd_left();
	case HF_RIGHT:		return cmd_right();
	case HF_HARD_LEFT:	return cmd_Left();
	case HF_HARD_RIGHT:	return cmd_Right();
	case HF_DELAYB:		return cmd_delayb (c);
	case HF_BEACON:		return cmd_beacon();
	case HF_EXIT:		return cmd_exit();
	case HF_AIRPORT:	return cmd_airport();
	case HF_CLIMB:		return cmd_climb();
	case HF_DESCEND:	return cmd_descend();
	case HF_SETALT:		return cmd_setalt (c);
	case HF_SETRELALT:	return cmd_setrelalt (c);
	case HF_BENUM:		return cmd_benum (c);
	case HF_TO_DIR:		return cmd_to_dir (c);
	case HF_REL_DIR:	return cmd_rel_dir (c);
	case HF_MARK:		return cmd_mark();
	case HF_UNMARK:		return cmd_unmark();
	case HF_IGNORE:		return cmd_ignore();
	case HF_QUIT:		quitter();
    };
}

static const char* cmd_setplane (char c)
{
    const struct Plane* pp = findplane (plane_number(c));
    if (!pp)
	return "Unknown plane";
    memcpy (&_p, pp, sizeof(_p));
    _p.delayd = false;
    return NULL;
}

static const char* cmd_turn (void)
{
    if (_p.altitude == 0)
	return "Planes at airports may not change direction";
    return NULL;
}

static const char* cmd_circle (void)
{
    if (_p.altitude == 0)
	return "Planes cannot circle on the ground";
    _p.new_dir = MAXDIR;
    return NULL;
}

static const char* cmd_left (void)
{
    _dir = D_LEFT;
    _p.new_dir = (_p.dir-1) % MAXDIR;
    return NULL;
}

static const char* cmd_right (void)
{
    _dir = D_RIGHT;
    _p.new_dir = (_p.dir+1) % MAXDIR;
    return NULL;
}

static const char* cmd_Left (void)
{
    _p.new_dir = (_p.dir-2) % MAXDIR;
    return NULL;
}

static const char* cmd_Right (void)
{
    _p.new_dir = (_p.dir+2) % MAXDIR;
    return NULL;
}

static const char* cmd_delayb (char c)
{
    unsigned bi = c-'0';
    if (bi >= _sp->num_beacons)
	return "Unknown beacon";
    if (sign(_sp->beacon[bi].x-_p.xpos) != _displacement[_p.dir].dx
	    || sign(_sp->beacon[bi].y-_p.ypos) != _displacement[_p.dir].dy)
	return "Beacon is not in flight path";
    _p.delayd = true;
    _p.delayd_no = bi;

    if (_dest_type != T_NODEST) {
	struct ScreenPos destpt = _sp->beacon[bi];
	switch (_dest_type) {
	    case T_BEACON:	destpt = _sp->beacon[_dest_no];	break;
	    case T_EXIT:	destpt = _sp->exit[_dest_no];	break;
	    case T_AIRPORT:	destpt = _sp->airport[_dest_no];break;
	}
	int xdiff = destpt.x - _sp->beacon[bi].x;
	int ydiff = destpt.y - _sp->beacon[bi].y;
	if (!xdiff && !ydiff)
	    return "Would already be there";
	_p.new_dir = dir_from_dxdy (xdiff, ydiff);
	if (_p.new_dir == _p.dir)
	    return "Already going in that direction";
    }
    return NULL;
}

static const char* cmd_beacon (void)
{
    _dest_type = T_BEACON;
    return NULL;
}

static const char* cmd_exit (void)
{
    _dest_type = T_EXIT;
    return NULL;
}

static const char* cmd_airport (void)
{
    _dest_type = T_AIRPORT;
    return NULL;
}

static const char* cmd_climb (void)
{
    _dir = D_UP;
    return NULL;
}

static const char* cmd_descend (void)
{
    _dir = D_DOWN;
    return NULL;
}

static const char* cmd_setalt (char c)
{
    unsigned newalt = c-'0';
    if (newalt > 9)
	return "Altitude would be too high";
    if (_p.altitude == newalt && _p.new_altitude == _p.altitude)
	return "Already at that altitude";
    _p.new_altitude = newalt;
    return NULL;
}

static const char* cmd_setrelalt (char c)
{
    unsigned newalt = c-'0';
    if (_dir == D_UP)
	newalt += _p.altitude;
    else
	newalt -= _p.altitude;
    if (newalt > 9)
	return "Altitude would be too high";
    _p.new_altitude = newalt;
    return NULL;
}

static const char* cmd_benum (char c)
{
    unsigned bi = c - '0';
    struct ScreenPos destpt;
    switch (_dest_type) {
	case T_BEACON:
	    if (bi >= _sp->num_beacons)
		return "Unknown beacon";
	    destpt = _sp->beacon[bi];
	    break;
	case T_EXIT:
	    if (bi >= _sp->num_exits)
		return "Unknown exit";
	    destpt = _sp->exit[bi];
	    break;
	case T_AIRPORT:
	    if (bi >= _sp->num_airports)
		return "Unknown airport";
	    destpt = _sp->airport[bi];
	    break;
	default:
	    return "Unknown destination";
    }
    _p.new_dir = dir_from_dxdy (destpt.x - _p.xpos, destpt.y - _p.ypos);
    _dest_no = bi;
    return NULL;
}

static const char* cmd_to_dir (char c)
{
    _p.new_dir = dir_no(c);
    return NULL;
}

static const char* cmd_rel_dir (char c)
{
    unsigned angle = dir_no(c);
    switch (_dir) {
	case D_LEFT:
	    _p.new_dir = (_p.dir-angle) % MAXDIR;
	    break;
	case D_RIGHT:
	    _p.new_dir = (_p.dir+angle) % MAXDIR;
	    break;
	default:
	    return "Bizarre direction in rel_dir!";
    }
    return NULL;
}

static const char* cmd_mark (void)
{
    if (!_p.altitude)
	return "Cannot mark planes on the ground";
    if (_p.status == S_MARKED)
	return "Already marked";
    _p.status = S_MARKED;
    return NULL;
}

static const char* cmd_unmark (void)
{
    if (!_p.altitude)
	return "Cannot unmark planes on the ground";
    if (_p.status == S_UNMARKED)
	return "Already unmarked";
    _p.status = S_UNMARKED;
    return NULL;
}

static const char* cmd_ignore (void)
{
    if (!_p.altitude)
	return "Cannot ignore planes on the ground";
    if (_p.status == S_IGNORED)
	return "Already ignored";
    _p.status = S_IGNORED;
    return NULL;
}

//}}}-------------------------------------------------------------------
