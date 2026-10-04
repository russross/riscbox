// Copyright (c) 1983 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "sail.h"

//{{{ Local functions --------------------------------------------------

static void acceptcombat (void);
static void grapungrap (void);
static void unfoulplayer (void);
static void changesail (void);
static void acceptsignal (void);
static void lookout (void);
static const char *saywhat (struct Ship *sp, int flag);
static void eyeball (struct Ship *ship);
static void acceptmove (void);
static void acceptboard (void);
static void repair (void);
static int turned (void);
static void loadplayer (void);
static void initialize(void);

//}}}-------------------------------------------------------------------
//{{{ Global variables

// the current scenario
const struct Scenario* _cc = NULL;
VECTOR (struct ShipList, _ships);

int _winddir = 0;
int _windspeed = 0;
int _turn = 0;
int _loaded = 0;
int _fired = 0;
int _changed = 0;
int _repaired = 0;
int _dont_adjust = 0;
int _player = 0;

//}}}-------------------------------------------------------------------

int main (void)
{
    initialize();
    Msg("Aye aye, Sir");
    play();
    return EXIT_SUCCESS;
}

static void initialize(void)
{
    initscreen();

    struct ScenarioSelector selscen = select_scenario();
    if (selscen.scenario >= NSCENE)
	exit (EXIT_SUCCESS);	// cancelled

    _cc = &_scenarios[selscen.scenario];
    _player = selscen.ship;
    _windspeed = _cc->windspeed;
    _winddir = _cc->winddir;

    vector_reserve (&_ships, _cc->vessels);
    int nat[NNATION];
    memset (nat, 0, sizeof(nat));
    for (unsigned s = 0; s < _cc->vessels; ++s) {
	struct Ship* sp = (struct Ship*) vector_emplace_back (&_ships);
	const struct ScenarioShip* ssp = &_cc->ship[s];
	sp->shipname = ssp->shipname;
	sp->nationality = ssp->nationality;
	sp->shiprow = ssp->shiprow;
	sp->shipcol = ssp->shipcol;
	sp->shipdir = ssp->shipdir;
	sp->specs = ssp->specs;
	sp->status.index = s;
	sp->status.load.l = L_ROUND;
	sp->status.load.r = L_ROUND;
	sp->status.ready.r = R_LOADED | R_INITIAL;
	sp->status.ready.l = R_LOADED | R_INITIAL;
	sp->status.stern = nat[sp->nationality]++;
	sp->status.dir = ssp->shipdir;
	sp->status.row = ssp->shiprow;
	sp->status.col = ssp->shipcol;
    }
    _ms = SHIP(_player);
    _ms->status.load = selscen.iniload;
    _ms->status.ready.r = _ms->status.ready.l = R_LOADED| R_INITIAL;

    draw_screen();
}

_Noreturn void play(void)
{
    for (;;) {
	draw_screen();
	switch (sgetch ("~\b", NULL, 0)) {
	    case 'm':	acceptmove(); break;
	    case 's':	acceptsignal(); break;
	    case 'g':	grapungrap(); break;
	    case 'u':	unfoulplayer(); break;
	    case 'v':	Msg("sail " BSDGAMES_VERSTRING); break;
	    case 'b':	acceptboard(); break;
	    case 'f':	acceptcombat(); break;
	    case 'l':	loadplayer(); break;
	    case 'c':	changesail(); break;
	    case 'r':	repair(); break;
	    case 'C':	centerview(); break;
	    case 'U':	upview(); break;
	    case 'D':
	    case 'N':	downview(); break;
	    case 'H':	leftview(); break;
	    case 'J':	rightview(); break;
	    case 'F':	lookout(); break;
	    case 'q':
	    case 'Q':	leave(LEAVE_QUIT); break;
	    case 'B':
		Msg("'Hands to stations!'");
		unboard(_ms, _ms, 1);	// cancel DBP's
		unboard(_ms, _ms, 0);	// cancel offense
		break;
	    case '\f':
		centerview();
		break;
	    case 'L':
		_ms->status.load.l = L_EMPTY;
		_ms->status.load.r = L_EMPTY;
		_ms->status.ready.l = R_EMPTY;
		_ms->status.ready.r = R_EMPTY;
		Msg("Broadsides unloaded");
		break;
	    case 'I':
		foreachship(sp)
		    if (sp != _ms)
			eyeball(sp);
		break;
	    case 'i': {
		struct Ship *sp = closestenemy (_ms, 0, 1);
		if (!sp)
		    Msg("No more ships left.");
		else
		    eyeball(sp);
		break; }
	    case 'S':
		_dont_adjust = !_dont_adjust;
		break;
	}
    }
}

static void acceptcombat(void)
{
    int men = 0;
    int target, temp;
    int n, r;
    int index, rakehim, sternrake;
    int hhits = 0, ghits = 0, rhits = 0, chits = 0;
    int crew[3];
    int load;
    int guns, car, ready, shootat, hit;
    int roll;
    struct Ship *closest;

    crew[0] = _ms->specs.crew[0];
    crew[1] = _ms->specs.crew[1];
    crew[2] = _ms->specs.crew[2];
    for (n = 0; n < 3; ++n) {
	if (_ms->status.OBP[n].turnsent)
	    men += _ms->status.OBP[n].mensent;
    }
    for (n = 0; n < 3; ++n) {
	if (_ms->status.DBP[n].turnsent)
	    men += _ms->status.DBP[n].mensent;
    }
    if (men) {
	crew[0] = men / 100 ? 0 : crew[0] != 0;
	crew[1] = (men % 100) / 10 ? 0 : crew[1] != 0;
	crew[2] = men % 10 ? 0 : crew[2] != 0;
    }
    for (r = 0; r < 2; ++r) {
	if (r) {
	    ready = _ms->status.ready.r;
	    load = _ms->status.load.r;
	    guns = _ms->specs.gun.r;
	    car = _ms->specs.car.r;
	} else {
	    ready = _ms->status.ready.l;
	    load = _ms->status.load.l;
	    guns = _ms->specs.gun.l;
	    car = _ms->specs.car.l;
	}
	if ((!guns && !car) || load == L_EMPTY || (ready & R_LOADED) == 0)
	    goto cant;
	if (_ms->status.struck || !crew[2])
	    goto cant;
	closest = closestenemy(_ms, (r ? 'r' : 'l'), 1);
	if (closest == 0)
	    goto cant;
	if (closest->status.struck)
	    goto cant;
	target = range(_ms, closest);
	if (target > c_ShotRange[load] || (!guns && target >= 3))
	    goto cant;
	Signal("$$ within range of %s broadside.", closest, r ? "right" : "left");
	if (load > L_CHAIN && target < 6) {
	    switch (sgetch("Aim for hull or rigging? ", (struct Ship *) 0, 1)) {
		case 'r':
		    shootat = RIGGING;
		    break;
		case 'h':
		    shootat = HULL;
		    break;
		default:
		    shootat = -1;
		    Msg("'Avast there! Hold your fire.'");
	    }
	} else {
	    if (sgetch("Fire? ", NULL, 1) == 'n') {
		shootat = -1;
		Msg("Belay that! Hold your fire.");
	    } else
		shootat = RIGGING;
	}
	if (shootat == -1)
	    continue;
	_fired = 1;
	rakehim = gunsbear(_ms, closest) && !gunsbear(closest, _ms);
	temp = portside(closest, _ms, 1) - closest->status.dir + 1;
	if (temp < 1)
	    temp += 8;
	else if (temp > 8)
	    temp -= 8;
	sternrake = temp > 4 && temp < 6;
	if (rakehim) {
	    if (!sternrake)
		Msg("Raking the %s!", closest->shipname);
	    else
		Msg("Stern Rake! %s splintering!", closest->shipname);
	}
	index = guns;
	if (target < 3)
	    index += car;
	index = (index - 1) / 3;
	index = index > 8 ? 8 : index;
	if (!rakehim)
	    hit = c_HDT[index][target - 1];
	else
	    hit = c_HDTrake[index][target - 1];
	if (rakehim && sternrake)
	    ++hit;
	hit += c_QUAL[index][_ms->specs.qual - 1];
	for (n = 0; n < 3 && !_ms->status.captured; ++n)
	    if (!crew[n]) {
		if (index <= 5)
		    --hit;
		else
		    hit -= 2;
	    }
	if (ready & R_INITIAL) {
	    if (index <= 3)
		++hit;
	    else
		hit += 2;
	}
	if (_ms->status.captured) {
	    if (index <= 1)
		--hit;
	    else
		hit -= 2;
	}
	hit += c_AMMO[index][load - 1];
	if (((temp = _ms->specs.sclass) >= 5 || temp == 1) && _windspeed == 5)
	    --hit;
	if (_windspeed == 6 && temp == 4)
	    hit -= 2;
	if (_windspeed == 6 && temp <= 3)
	    --hit;
	if (hit >= 0) {
	    roll = dieroll();
	    if (load == L_GRAPE)
		chits = hit;
	    else {
		const struct Tables *t;
		if (hit > 10)
		    hit = 10;
		t = &(shootat == RIGGING ? c_RigTable : c_HullTable) [hit][roll - 1];
		chits = t->C;
		rhits = t->R;
		hhits = t->H;
		ghits = t->G;
		if (closest->status.FS)
		    rhits *= 2;
		if (load == L_CHAIN) {
		    ghits = 0;
		    hhits = 0;
		}
	    }
	    table(_ms, closest, shootat, load, hit, roll);
	}
	Msg("Damage inflicted on the %s:", closest->shipname);
	Msg("\t%d HULL, %d GUNS, %d CREW, %d RIGGING", hhits, ghits, chits, rhits);
	if (!r) {
	    _ms->status.load.l = L_EMPTY;
	    _ms->status.ready.l = R_EMPTY;
	} else {
	    _ms->status.load.r = L_EMPTY;
	    _ms->status.ready.r = R_EMPTY;
	}
	continue;
      cant:
	Msg("Unable to fire %s broadside", r ? "right" : "left");
    }
}

static void grapungrap(void)
{
    foreachship (sp) {
	if (sp == _ms || sp->status.dir == 0)
	    continue;
	if (range(_ms, sp) > 1 && !grappled2(_ms, sp))
	    continue;
	switch (sgetch("Attempt to grapple or ungrapple $$: ", sp, 1)) {
	    case 'g':
		if (dieroll() < 3 || _ms->nationality == shipnationality(sp)) {
		    grapple_ship_with (_ms, sp->status.index);
		    grapple_ship_with (sp, _player);
		    Msg("Attempt succeeds!");
		    makesignal(_ms, "grappled with $$", sp);
		} else
		    Msg("Attempt fails.");
		break;
	    case 'u':
		for (int i = grappled2(_ms, sp); --i >= 0;) {
		    if (_ms->nationality == shipnationality(sp) || dieroll() < 3) {
			cleangrapple(_ms, sp, 0);
			Msg("Attempt succeeds!");
			makesignal(_ms, "ungrappling with $$", sp);
		    } else
			Msg("Attempt fails.");
		}
		break;
	}
    }
}

static void unfoulplayer(void)
{
    foreachship (to) {
	if (fouled2(_ms, to) == 0)
	    continue;
	if (sgetch("Attempt to unfoul with the $$? ", to, 1) != 'y')
	    continue;
	for (int i = fouled2(_ms, to); --i >= 0;) {
	    if (dieroll() <= 2) {
		cleanfoul(_ms, to, 0);
		Msg("Attempt succeeds!");
		makesignal(_ms, "Unfouling $$", to);
	    } else
		Msg("Attempt fails.");
	}
    }
}

static void changesail(void)
{
    int rig = _ms->specs.rig[0];
    int full = _ms->status.FS;
    if (_windspeed == 6 || (_windspeed == 5 && _ms->specs.sclass > 4))
	rig = 0;
    if (_ms->specs.crew[2] && rig) {
	if (!full) {
	    if (sgetch("Increase to Full sails? ", NULL, 1) == 'y') {
		_ms->status.FS = 1;
		_changed = 1;
	    }
	} else {
	    if (sgetch("Reduce to Battle sails? ", NULL, 1) == 'y') {
		_ms->status.FS = 0;
		_changed = 1;
	    }
	}
    } else if (!rig)
	Msg("Sails rent to pieces");
}

static void acceptsignal(void)
{
    char buf[60];
    char *p = buf;

    *p++ = '"';
    sgetstr("Message? ", p, sizeof buf - 2);
    while (*p++) {}
    p[-1] = '"';
    *p = 0;
    makemsg (_ms, "%s", buf);
}

static void lookout(void)
{
    char buf[3];
    sgetstr("What ship? ", buf, sizeof buf);
    foreachship (sp) {
	char c = *c_CountryName[sp->nationality];
	if ((tolower(c) == *buf || colours(sp) == *buf)
		&& (sp->status.stern == buf[1]
		    || sterncolour(sp) == buf[1]
		    || buf[1] == '?'))
	    eyeball(sp);
    }
}

static const char* saywhat(struct Ship *sp, int flag)
{
    if (sp == _ms)
	return "";
    else if (sp->status.struck)
	return "(struck)";
    else if (sp->status.captured)
	return "(captured)";
    else if (flag)
	return "(available)";
    else
	return "(computer)";
}

static void eyeball(struct Ship *ship)
{
    if (ship->status.dir != 0) {
	Msg("Sail ho! (range %d, %s)", range(_ms, ship), saywhat(ship, 0));
	int i = portside(_ms, ship, 1) - _ms->status.dir;
	if (i <= 0)
	    i += 8;
	Signal("$$ %s %s %s.", ship, c_CountryName[ship->nationality], c_ClassName[ship->specs.sclass], c_DirectionName[i]);
    }
}

static inline bool turnfirst (const char* x)
    { return *x == 'r' || *x == 'l'; }

static void acceptmove(void)
{
    if (!_ms->specs.crew[2] || snagged(_ms) || !_windspeed) {
	Msg("Unable to move");
	return;
    }
    char af;
    short ta = maxturns(_ms, &af);
    short ma = maxmove(_ms, _ms->status.dir, 0);
    char prompt[32];
    snprintf (ArrayBlock(prompt), "move (%hd,%c%hd): ", ma, af ? '\'' : ' ', ta);
    char buf[sizeof(_ms->status.movebuf)];
    sgetstr (prompt, buf, sizeof(buf));
    int dir = _ms->status.dir;
    int vma = ma;
    char last = '\0';
    bool moved = false;
    for (char* p = buf; *p; ++p)
	switch (*p) {
	    case 'l':
		dir -= 2;
		// fallthrough
	    case 'r':
		if (++dir == 0)
		    dir = 8;
		else if (dir == 9)
		    dir = 1;
		if (last == 't') {
		    Msg("Ship can't turn that fast.");
		    *p-- = '\0';
		}
		last = 't';
		--ma;
		--ta;
		vma = min_i (ma, maxmove(_ms, dir, 0));
		if (moved && (ta < 0 || vma < 0))
		    *p-- = '\0';
		break;
	    case 'b':
		--ma;
		--vma;
		last = 'b';
		if (moved && (ta < 0 || vma < 0))
		    *p-- = '\0';
		break;
	    case '0':
	    case 'd':
		*p-- = '\0';
		break;
	    case '\n':
		*p-- = '\0';
		break;
	    case '1':
	    case '2':
	    case '3':
	    case '4':
	    case '5':
	    case '6':
	    case '7':
		if (last == '0') {
		    Msg("Can't move that fast.");
		    *p-- = '\0';
		}
		last = '0';
		moved = true;
		ma -= *p - '0';
		vma -= *p - '0';
		if (moved && (ta < 0 || vma < 0))
		    *p-- = '\0';
		break;
	    default:
		if (!isspace(*p)) {
		    Msg("Input error.");
		    *p-- = '\0';
		}
	}
    if (moved && (ta < 0 || vma < 0 || (af && turnfirst(buf)))) {
	Msg("Movement error.");
	if (ta < 0) {
	    if (_ms->status.FS == 1) {
		_ms->status.FS = 0;
		Msg("No hands to set full sails.");
	    }
	} else if (ma >= 0)
	    buf[1] = '\0';
    }
    if (af && !moved) {
	if (_ms->status.FS == 1) {
	    _ms->status.FS = 0;
	    Msg("No hands to set full sails.");
	}
    }
    if (*buf)
	snprintf (ArrayBlock (_ms->status.movebuf), "%s", buf);
    else
	snprintf (ArrayBlock (_ms->status.movebuf), "d");
    Msg("Helm: %s.", _ms->status.movebuf);
}

static void acceptboard(void)
{
    int n;
    int crew[3];
    int men = 0;
    char c;

    crew[0] = _ms->specs.crew[0];
    crew[1] = _ms->specs.crew[1];
    crew[2] = _ms->specs.crew[2];
    for (n = 0; n < NBP; ++n) {
	if (_ms->status.OBP[n].turnsent)
	    men += _ms->status.OBP[n].mensent;
    }
    for (n = 0; n < NBP; ++n) {
	if (_ms->status.DBP[n].turnsent)
	    men += _ms->status.DBP[n].mensent;
    }
    if (men) {
	crew[0] = men / 100 ? 0 : crew[0] != 0;
	crew[1] = (men % 100) / 10 ? 0 : crew[1] != 0;
	crew[2] = men % 10 ? 0 : crew[2] != 0;
    } else {
	crew[0] = crew[0] != 0;
	crew[1] = crew[1] != 0;
	crew[2] = crew[2] != 0;
    }
    foreachship(sp) {
	if (sp == _ms || sp->status.dir == 0 || range(_ms, sp) > 1)
	    continue;
	if (_ms->nationality == shipnationality(sp))
	    continue;
	if (meleeing(_ms, sp) && crew[2]) {
	    c = sgetch("How many more to board the $$? ", sp, 1);
	    parties(sp, crew, 0, c);
	} else if ((fouled2(_ms, sp) || grappled2(_ms, sp)) && crew[2]) {
	    c = sgetch("Crew sections to board the $$ (3 max) ?", sp, 1);
	    parties(sp, crew, 0, c);
	}
    }
    if (crew[2]) {
	c = sgetch("How many sections to repel boarders? ", NULL, 1);
	parties(_ms, crew, 1, c);
    }
}

static void repair (void)
{
    char c;
    char *repairs;
    struct ShipSpecs *ptr = &_ms->specs;
    int count;

#define FIX(x, m) (m - ptr->x > count \
	? (ptr->x += count, count = 0) : (count -= m - ptr->x, ptr->x = m))

    if (_repaired || _loaded || _fired || _changed || turned()) {
	Msg("No hands free to repair");
	return;
    }
    c = sgetch("Repair (hull, guns, rigging)? ", NULL, 1);
    switch (c) {
	case 'h': repairs = &_ms->status.RH; break;
	case 'g': repairs = &_ms->status.RG; break;
	case 'r': repairs = &_ms->status.RR; break;
	default: Msg("Avast heaving!"); return;
    }
    if (++*repairs >= 3) {
	count = 2;
	switch (c) {
	    case 'h':{
		    int maxr = ptr->guns / 4;
		    if (ptr->hull < maxr) {
			FIX(hull, maxr);
			_ms->specs.hull = ptr->hull;
		    }
		    break;
		}
	    case 'g':
		if (ptr->gun.l < ptr->gun.r) {
		    int maxr = ptr->guns / 5 - ptr->car.l;
		    if (ptr->gun.l < maxr) {
			FIX(gun.l, maxr);
			_ms->specs.gun.l = ptr->gun.l;
			_ms->specs.car.l = ptr->car.l;
		    }
		} else {
		    int maxr = ptr->guns / 5 - ptr->car.r;
		    if (ptr->gun.r < maxr) {
			FIX(gun.r, maxr);
			_ms->specs.gun.r = ptr->gun.r;
			_ms->specs.car.r = ptr->car.r;
		    }
		}
		break;
	    case 'r': {
		enum { X = 2 };
		if (ptr->rig[3] >= 0 && ptr->rig[3] < X) {
		    FIX(rig[3], X);
		    _ms->specs.rig[3] = ptr->rig[3];
		}
		if (count && ptr->rig[2] < X) {
		    FIX(rig[2], X);
		    _ms->specs.rig[2] = ptr->rig[2];
		}
		if (count && ptr->rig[1] < X) {
		    FIX(rig[1], X);
		    _ms->specs.rig[1] = ptr->rig[1];
		}
		if (count && ptr->rig[0] < X) {
		    FIX(rig[0], X);
		    _ms->specs.rig[0] = ptr->rig[0];
		}
		break; }
	}
	if (count == 2) {
	    Msg("Repairs completed.");
	    *repairs = 2;
	} else
	    *repairs = 0;
    }
    _repaired = 1;
}

static int turned(void)
{
    for (const char* p = _ms->status.movebuf; *p; ++p)
	if (*p == 'r' || *p == 'l')
	    return 1;
    return 0;
}

static void loadplayer(void)
{
    if (!_ms->specs.crew[2]) {
	Msg("Out of crew");
	return;
    }
    char sc = sgetch ("Load which broadside (left or right)? ", NULL, 1);
    unsigned lside = 0;
    if (sc == 'r')
	lside = 1;
    else if (sc != 'l')
	return;

    sc = sgetch ("Reload with (round, double, chain, grape)? ", NULL, 1);
    unsigned ltype, lready = 0;
    switch (sc) {
	case 'r':	ltype = L_ROUND; break;
	case 'd':	ltype = L_DOUBLE; lready = R_DOUBLE; break;
	case 'c':	ltype = L_CHAIN; break;
	case 'g':	ltype = L_GRAPE; break;
	default:	Msg("Broadside not loaded."); return;
    }
    if (lside == 0) {
	_ms->status.load.r = ltype;
	_ms->status.ready.r = lready | R_LOADING;
    } else {
	_ms->status.load.l = ltype;
	_ms->status.ready.l = lready | R_LOADING;
    }
    _loaded = 1;
}

//{{{ Data lookup tables -----------------------------------------------

const struct windeffects c_WindEffectTable [7][6] = {
    {{9, 9, 9, 9}, {9, 9, 9, 9}, {9, 9, 9, 9}, {9, 9, 9, 9}, {9, 9, 9, 9}, {9, 9, 9, 9}},
    {{3, 2, 2, 0}, {3, 2, 1, 0}, {3, 2, 1, 0}, {3, 2, 1, 0}, {2, 1, 0, 0}, {2, 1, 0, 0}},
    {{1, 1, 1, 0}, {1, 1, 0, 0}, {1, 0, 0, 0}, {1, 0, 0, 0}, {1, 0, 0, 0}, {1, 0, 0, 0}},
    {{0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 0}},
    {{0, 0, 0, 0}, {1, 0, 0, 0}, {1, 1, 0, 0}, {1, 1, 0, 0}, {2, 2, 1, 0}, {2, 2, 1, 0}},
    {{1, 0, 0, 0}, {1, 1, 0, 0}, {1, 1, 1, 0}, {1, 1, 1, 0}, {3, 2, 2, 0}, {3, 2, 2, 0}},
    {{2, 1, 1, 0}, {3, 2, 1, 0}, {3, 2, 1, 0}, {3, 2, 1, 0}, {3, 3, 2, 0}, {3, 3, 2, 0}}
};

const struct Tables c_RigTable[11][6] = {
    {{0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 1}, {0, 0, 1, 0}},
    {{0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 1}, {0, 0, 1, 0}, {1, 0, 0, 1}, {0, 1, 1, 1}},
    {{0, 0, 0, 0}, {0, 0, 0, 1}, {0, 0, 1, 1}, {0, 1, 0, 1}, {0, 1, 0, 1}, {1, 0, 1, 2}},
    {{0, 0, 0, 0}, {0, 0, 1, 1}, {0, 1, 0, 1}, {0, 0, 0, 2}, {0, 1, 0, 2}, {1, 0, 1, 2}},
    {{0, 1, 0, 1}, {1, 0, 0, 1}, {0, 1, 1, 2}, {0, 1, 0, 2}, {0, 0, 1, 3}, {1, 0, 1, 4}},
    {{0, 0, 1, 1}, {0, 1, 0, 2}, {1, 0, 0, 3}, {0, 1, 1, 3}, {1, 0, 0, 4}, {1, 1, 1, 4}},
    {{0, 0, 1, 2}, {0, 1, 1, 2}, {1, 1, 0, 3}, {0, 1, 0, 4}, {1, 0, 0, 4}, {1, 0, 1, 5}},
    {{0, 0, 1, 2}, {0, 1, 0, 3}, {1, 1, 0, 3}, {1, 0, 2, 4}, {0, 2, 1, 5}, {2, 1, 0, 5}},
    {{0, 2, 1, 3}, {1, 0, 0, 3}, {2, 1, 0, 4}, {0, 1, 1, 4}, {0, 1, 0, 5}, {1, 0, 2, 6}},
    {{1, 1, 0, 4}, {1, 0, 1, 4}, {2, 0, 0, 5}, {0, 2, 1, 5}, {0, 1, 2, 6}, {0, 2, 0, 7}},
    {{1, 0, 1, 5}, {0, 2, 0, 6}, {1, 2, 0, 6}, {1, 1, 1, 6}, {2, 0, 2, 6}, {1, 1, 2, 7}}
};

const struct Tables c_HullTable[11][6] = {
    {{0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 0}, {0, 0, 0, 0}, {1, 0, 0, 0}, {0, 1, 0, 0}},
    {{0, 0, 0, 0}, {0, 0, 0, 0}, {0, 1, 0, 0}, {1, 1, 0, 0}, {1, 0, 1, 0}, {1, 0, 1, 1}},
    {{0, 1, 0, 0}, {1, 0, 0, 0}, {1, 1, 0, 0}, {1, 0, 1, 0}, {1, 0, 1, 1}, {2, 1, 0, 0}},
    {{0, 1, 1, 0}, {1, 0, 0, 0}, {1, 1, 1, 0}, {2, 0, 0, 1}, {2, 0, 1, 0}, {2, 2, 0, 0}},
    {{0, 1, 1, 0}, {1, 0, 0, 1}, {2, 1, 0, 1}, {2, 2, 1, 0}, {3, 0, 1, 0}, {3, 1, 0, 0}},
    {{1, 1, 1, 0}, {2, 0, 2, 1}, {2, 1, 1, 0}, {2, 2, 0, 0}, {3, 1, 0, 1}, {3, 1, 1, 0}},
    {{1, 2, 2, 0}, {2, 0, 2, 1}, {2, 1, 0, 1}, {2, 2, 0, 0}, {3, 1, 1, 0}, {4, 2, 1, 0}},
    {{2, 1, 1, 0}, {2, 0, 1, 1}, {3, 2, 2, 0}, {3, 2, 0, 0}, {4, 2, 1, 0}, {4, 2, 1, 1}},
    {{2, 1, 2, 0}, {3, 1, 1, 1}, {3, 2, 2, 0}, {4, 2, 1, 0}, {4, 1, 0, 2}, {4, 2, 2, 0}},
    {{2, 3, 1, 0}, {3, 2, 2, 0}, {3, 2, 2, 1}, {4, 2, 2, 0}, {4, 1, 0, 3}, {5, 1, 2, 0}},
    {{2, 2, 4, 0}, {3, 3, 1, 1}, {4, 2, 1, 1}, {5, 1, 0, 2}, {5, 1, 2, 1}, {6, 2, 2, 0}},
};

const char c_AMMO[9][4] = {
    {-1, 1, 0, 1},
    {-1, 1, 0, 1},
    {-1, 1, 0, 1},
    {-2, 1, 0, 2},
    {-2, 2, 0, 2},
    {-2, 2, 0, 2},
    {-3, 2, 0, 2},
    {-3, 2, 0, 3},
    {-3, 2, 0, 3}
};

const char c_HDT[9][10] = {
    {1, 0, -1, -2, -3, -3, -4, -4, -4, -4},
    {1, 1, 0, -1, -2, -2, -3, -3, -3, -3},
    {2, 1, 0, -1, -2, -2, -3, -3, -3, -3},
    {2, 2, 1, 0, -1, -1, -2, -2, -2, -2},
    {3, 2, 1, 0, -1, -1, -2, -2, -2, -2},
    {3, 3, 2, 1, 0, 0, -1, -1, -1, -1},
    {4, 3, 2, 1, 0, 0, -1, -1, -1, -1},
    {4, 4, 3, 2, 1, 1, 0, 0, 0, 0},
    {5, 4, 3, 2, 1, 1, 0, 0, 0, 0}
};

const char c_HDTrake[9][10] = {
    {2, 1, 0, -1, -2, -2, -3, -3, -3, -3},
    {2, 2, 1, 0, -1, -1, -2, -2, -2, -2},
    {3, 2, 1, 0, -1, -1, -2, -2, -2, -2},
    {4, 3, 2, 1, 0, 0, -1, -1, -1, -1},
    {5, 4, 3, 2, 1, 1, 0, 0, 0, 0},
    {6, 5, 4, 3, 2, 2, 1, 1, 1, 1},
    {7, 6, 5, 4, 3, 3, 2, 2, 2, 2},
    {8, 7, 6, 5, 4, 4, 3, 3, 3, 3},
    {9, 8, 7, 6, 5, 5, 4, 4, 4, 4}
};

const char c_QUAL[9][5] = {
    {-1, 0, 0, 1, 1},
    {-1, 0, 0, 1, 1},
    {-1, 0, 0, 1, 2},
    {-1, 0, 0, 1, 2},
    {-1, 0, 0, 2, 2},
    {-1, -1, 0, 2, 2},
    {-2, -1, 0, 2, 2},
    {-2, -1, 0, 2, 2},
    {-2, -1, 0, 2, 3}
};

const char c_MT[9][3] = {
    {1, 0, 0},
    {1, 1, 0},
    {2, 1, 0},
    {2, 1, 1},
    {2, 2, 1},
    {3, 2, 1},
    {3, 2, 2},
    {4, 3, 2},
    {4, 4, 2}
};

const char c_ShotRange[] = {
    0,
    1,			       // grape
    3,			       // chain
    10,			       // round
    1			       // double
};

const char c_CountryName [NNATION][9] = {
    "American", "British", "Spanish", "French", "Japanese"
};

const char* const c_ClassName[7] = {
    "Drift wood",
    "Ship of the Line",
    "Ship of the Line",
    "Frigate",
    "Corvette",
    "Sloop",
    "Brig"
};

const char* const c_DirectionName[9] = {
    "dead ahead",
    "off the starboard bow",
    "off the starboard beam",
    "off the starboard quarter",
    "dead astern",
    "off the port quarter",
    "off the port beam",
    "off the port bow",
    "dead ahead"
};

const char c_QualityName[6][9] = { "dead", "mutinous", "green", "mundane", "crack", "elite" };

const char c_LoadLetter[] = "-GCRDE";

const char c_dc[] = { 0, 0,-1,-1,-1, 0, 1, 1, 1 };
const char c_dr[] = { 0, 1, 1, 0,-1,-1,-1, 0, 1 };

//}}}-------------------------------------------------------------------
