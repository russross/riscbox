// Copyright (c) 1983 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "sail.h"

//{{{ Local functions --------------------------------------------------

static void unfoul (void);
static void boardcomp (void);
static int fightitout (struct Ship* from, struct Ship* to, int key);
static void resolve (void);
static void compcombat (void);
static int next (void);
static void thinkofgrapples (void);
static void checkup (void);
static void prizecheck (void);
static int str_end (const char* str);
static void closeon (struct Ship* from, struct Ship* to, char* command, int ta, int ma, int af);
static int score (struct Ship* ship, struct Ship* to, char* movement, int onlytemp);
static void move_ship (struct Ship* ship, const char* p, unsigned char* dir, short *row, short *col, char* drift);
static void try_ship_cmd (struct Ship* f, struct Ship* t, char* command, char* temp, int ma, int ta, int af, int vma, int dir, int* high, int rakeme);
static void rmend (char* str);
static void moveall (void);
static bool stillmoving (unsigned k);
static bool is_isolated (const struct Ship* ship);
static int push (const struct Ship* from, const struct Ship* to);
static void step (struct Ship* sp, int com, bool* moved);
static void sendbp (struct Ship* from, struct Ship* to, int sections, int isdefense);
static unsigned is_toughmelee (const struct Ship* ship, const struct Ship* to, bool isdefense, int count);
static void reload (void);
static void checksails (void);
static void ungrap (struct Ship* from, struct Ship* to);
static void grap (struct Ship* from, struct Ship* to);
static void subtract (struct Ship* from, int totalfrom, int pcfrom);
static unsigned mensent (const struct Ship* from, const struct Ship* to, bool isdefense);

//}}}-------------------------------------------------------------------
//{{{ Local variables

static const int8_t c_Diagonals[] = { 0, 1, 1, 2, 3, 4, 4, 5 };	// diagonal distances in x==y

//}}}-------------------------------------------------------------------

int dr_turn (void)
{
    if (next() < 0)
	return 0;
    unfoul();
    checkup();
    prizecheck();
    moveall();
    thinkofgrapples();
    boardcomp();
    compcombat();
    resolve();
    reload();
    checksails();
    return 0;
}

static void unfoul(void)
{
    foreachship (sp) {
	if (sp == _ms)
	    continue;
	uint8_t nat = shipnationality(sp);
	foreachship (to) {
	    if (nat != shipnationality(to) && !is_toughmelee(sp, to, 0, 0))
		continue;
	    for (int i = fouled2(sp, to); --i >= 0;)
		if (dieroll() <= 2)
		    cleanfoul(sp, to, 0);
	}
    }
}

static void boardcomp(void)
{
    int crew[3];
    foreachship (sp) {
	if (sp == _ms)
	    continue;
	if (sp->status.dir == 0)
	    continue;
	if (sp->status.struck || sp->status.captured)
	    continue;
	if (!snagged(sp))
	    continue;
	crew[0] = sp->specs.crew[0] != 0;
	crew[1] = sp->specs.crew[1] != 0;
	crew[2] = sp->specs.crew[2] != 0;
	foreachship (sq) {
	    if (!Xsnagged2(sp, sq))
		continue;
	    if (meleeing(sp, sq))
		continue;
	    if (!sq->status.dir || sp->nationality == shipnationality(sq))
		continue;
	    switch (sp->specs.sclass - sq->specs.sclass) {
		case -3:
		case -4:
		case -5:
		    if (crew[0]) { // OBP
			sendbp (sp, sq, crew[0] * 100, 0);
			crew[0] = 0;
		    } else if (crew[1]) { // OBP
			sendbp (sp, sq, crew[1] * 10, 0);
			crew[1] = 0;
		    }
		    break;
		case -2:
		    if (crew[0] || crew[1]) { // OBP
			sendbp (sp, sq, crew[0] * 100 + crew[1] * 10, 0);
			crew[0] = crew[1] = 0;
		    }
		    break;
		case -1:
		case 0:
		case 1:
		    if (crew[0]) { // OBP
			sendbp (sp, sq, crew[0] * 100 + crew[1] * 10, 0);
			crew[0] = crew[1] = 0;
		    }
		    break;
		case 2:
		case 3:
		case 4:
		case 5: // OBP
		    sendbp (sp, sq, crew[0] * 100 + crew[1] * 10 + crew[2], 0);
		    crew[0] = crew[1] = crew[2] = 0;
		    break;
	    }
	}
    }
}

static int fightitout (struct Ship* from, struct Ship* to, int key)
{
    int frominjured, toinjured;
    int index, totalfrom = 0, totalto = 0;

    unsigned menfrom = mensent(from, to, key);
    unsigned mento = mensent(to, from, 0);

    int pcto = to->status.pcrew, pcfrom = from->status.pcrew;
    if (key) {
	if (!menfrom) {	       // if crew surprised
	    if (from->status.captured)
		menfrom = from->status.pcrew;
	    else
		menfrom = from->specs.crew[0] + from->specs.crew[1] + from->specs.crew[2];
	} else
	    menfrom *= 2;      // defenders have advantage
    }
    uint8_t fromqual = shipcrewqual(from);
    uint8_t toqual = shipcrewqual(to);
    int fromstrength = menfrom * fromqual;
    int strengthto = mento * toqual;
    unsigned count = 0;
    for (; fromstrength < strengthto * 3 && strengthto < fromstrength * 3 && count < 4; ++count) {
	index = fromstrength / 10;
	if (index > 8)
	    index = 8;
	toinjured = c_MT[index][2 - dieroll() / 3];
	totalto += toinjured;
	index = strengthto / 10;
	if (index > 8)
	    index = 8;
	frominjured = c_MT[index][2 - dieroll() / 3];
	totalfrom += frominjured;
	menfrom -= frominjured;
	mento -= toinjured;
	fromstrength = menfrom * fromqual;
	strengthto = mento * toqual;
    }
    if (fromstrength >= strengthto * 3 || count == 4) {
	unboard (to, from, 0);
	subtract (from, totalfrom, pcfrom);
	subtract (to, totalto, pcto);
	makemsg (from, "boarders from %s repelled", to->shipname);
	makemsg (to, "killed in melee: %d.  %s: %d", totalto, from->shipname, totalfrom);
	if (key)
	    return 1;
    } else if (strengthto >= fromstrength * 3) {
	unboard (from, to, 0);
	subtract (from, totalfrom, pcfrom);
	subtract (to, totalto, pcto);
	if (key) {		// ship captured
	    from->status.captured = true;
	    from->status.pcrewqual = to->specs.qual;
	    from->status.capnation = to->nationality;
	    mento = to->specs.crew[!to->specs.crew[0]];
	    if (mento) {	// install sent men as prize crew
		subtract(to, mento, pcto);
		subtract(from, -mento, 0);
	    }
	    makemsg (from, "captured by the %s!", to->shipname);
	    makemsg (to, "killed in melee: %d.  %s: %d", totalto, from->shipname, totalfrom);
	    mento = 0;
	    return 0;
	}
    }
    return 0;
}

static void resolve (void)
{
    foreachship(sp) {
	if (sp->status.dir == 0)
	    continue;
	for (struct Ship* sq = sp + 1, *sqend = vector_end(&_ships); sq < sqend; ++sq)
	    if (sq->status.dir && meleeing(sp, sq) && meleeing(sq, sp))
		fightitout(sp, sq, 0);
	int thwart = 2;
	foreachship(sq) {
	    if (sq->status.dir && meleeing(sq, sp))
		thwart = fightitout(sp, sq, 1);
	    if (!thwart)
		break;
	}
	if (!thwart) {
	    foreachship(sq) {
		if (sq->status.dir && meleeing(sq, sp))
		    unboard(sq, sp, 0);
		unboard(sp, sq, 0);
	    }
	    unboard(sp, sp, 1);
	} else if (thwart == 2)
	    unboard(sp, sp, 1);
    }
}

static void compcombat (void)
{
    int n;
    struct Ship* closest;
    int crew[3], men = 0, target, temp;
    int r, guns, ready, load, car;
    int index, rakehim, sternrake;
    int shootat, hit;

    foreachship(sp) {
	if (sp == _ms || sp->status.dir == 0)
	    continue;
	crew[0] = sp->specs.crew[0];
	crew[1] = sp->specs.crew[1];
	crew[2] = sp->specs.crew[2];
	for (n = 0; n < 3; ++n) {
	    if (sp->status.OBP[n].turnsent)
		men += sp->status.OBP[n].mensent;
	}
	for (n = 0; n < 3; ++n) {
	    if (sp->status.DBP[n].turnsent)
		men += sp->status.DBP[n].mensent;
	}
	if (men) {
	    crew[0] = men / 100 ? 0 : crew[0] != 0;
	    crew[1] = (men % 100) / 10 ? 0 : crew[1] != 0;
	    crew[2] = men % 10 ? 0 : crew[2] != 0;
	}
	for (r = 0; r < 2; ++r) {
	    if (!crew[2])
		continue;
	    if (sp->status.struck)
		continue;
	    if (r) {
		ready = sp->status.ready.r;
		guns = sp->specs.gun.r;
		car = sp->specs.car.r;
	    } else {
		ready = sp->status.ready.l;
		guns = sp->specs.gun.l;
		car = sp->specs.car.l;
	    }
	    if (!guns && !car)
		continue;
	    if ((ready & R_LOADED) == 0)
		continue;
	    closest = closestenemy(sp, r ? 'r' : 'l', 0);
	    if (closest == 0)
		continue;
	    if (range(closest, sp) > range(sp, closestenemy(sp, r ? 'r' : 'l', 1)))
		continue;
	    if (closest->status.struck)
		continue;
	    target = range(sp, closest);
	    if (target > 10)
		continue;
	    if (!guns && target >= 3)
		continue;
	    load = L_ROUND;
	    if (target == 1 && sp->status.loadwith == L_GRAPE)
		load = L_GRAPE;
	    if (target <= 3 && closest->status.FS)
		load = L_CHAIN;
	    if (target == 1 && load != L_GRAPE)
		load = L_DOUBLE;
	    if (load > L_CHAIN && target < 6)
		shootat = HULL;
	    else
		shootat = RIGGING;
	    rakehim = gunsbear(sp, closest)
		&& !gunsbear(closest, sp);
	    temp = portside(closest, sp, 1)
		- closest->status.dir + 1;
	    if (temp < 1)
		temp += 8;
	    if (temp > 8)
		temp -= 8;
	    sternrake = temp > 4 && temp < 6;
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
	    hit += c_QUAL[index][shipcrewqual(sp) - 1];
	    for (n = 0; n < 3 && !sp->status.captured; ++n)
		if (!crew[n])
		    hit -= 1 + (index > 5);
	    if (ready & R_INITIAL) {
		if (!r)
		    sp->status.ready.l &= ~R_INITIAL;
		else
		    sp->status.ready.r &= ~R_INITIAL;
		if (index <= 3)
		    ++hit;
		else
		    hit += 2;
	    }
	    if (sp->status.captured)
		hit -= 1 + (index > 1);
	    hit += c_AMMO[index][load - 1];
	    temp = sp->specs.sclass;
	    if ((temp >= 5 || temp == 1) && _windspeed == 5)
		--hit;
	    if (_windspeed == 6 && temp == 4)
		hit -= 2;
	    if (_windspeed == 6 && temp <= 3)
		--hit;
	    if (hit >= 0) {
		if (load != L_GRAPE)
		    hit = hit > 10 ? 10 : hit;
		table(sp, closest, shootat, load, hit, dieroll());
	    }
	}
    }
}

static int next(void)
{
    ++_turn;
    if (_turn % 7 == 0 && (dieroll() >= _cc->windchange || !_windspeed)) {
	switch (dieroll()) {
	    case 1:	_winddir = 1; break;
	    case 2:	break;
	    case 3:	++_winddir; break;
	    case 4:	--_winddir; break;
	    case 5:	_winddir += 2; break;
	    case 6:	_winddir -= 2; break;
	}
	if (_winddir > 8)
	    _winddir -= 8;
	if (_winddir < 1)
	    _winddir += 8;
	if (_windspeed) {
	    switch (dieroll()) {
		case 1:
		case 2:	--_windspeed; break;
		case 5:
		case 6:	++_windspeed; break;
	    }
	} else
	    ++_windspeed;
    }
    return 0;
}

static inline bool couldwin (const struct Ship* f, const struct Ship* t)
    { return f->specs.crew[1] > t->specs.crew[1] * 1.5; }

static void thinkofgrapples (void)
{
    foreachship(sp) {
	if (sp == _ms || sp->status.dir == 0)
	    continue;
	foreachship(sq) {
	    if (sp->nationality != shipnationality(sq)) {
		if (sp->status.struck || sp->status.captured)
		    continue;
		if (range(sp, sq) != 1)
		    continue;
		if (grappled2(sp, sq))
		    if (is_toughmelee(sp, sq, 0, 0))
			ungrap(sp, sq);
		    else
			grap(sp, sq);
		else if (couldwin(sp, sq)) {
		    grap(sp, sq);
		    sp->status.loadwith = L_GRAPE;
		}
	    } else
		ungrap(sp, sq);
	}
    }
}

static void checkup (void)
{
    foreachship(sp) {
	if (sp->status.dir == 0)
	    continue;
	int8_t explode = sp->status.explode;
	int8_t sink = sp->status.sink;
	if (explode != 1 && sink != 1)
	    continue;
	if (dieroll() < 5)
	    continue;
	if (sink == 1)
	    sp->status.sink = 2;
	else
	    sp->status.explode = 2;
	sp->status.dir = 0;
	if (snagged (sp))
	    foreachship (sq)
		cleansnag (sp, sq, 1);
	if (sink != 1) {
	    makemsg (sp, "exploding!");
	    foreachship (sq)
		if (sp != sq && sq->status.dir && range(sp, sq) < 4)
		    table (sp, sq, RIGGING, L_EXPLODE, sp->specs.guns / 13, 6);
	} else
	    makemsg (sp, "sinking!");
    }
}

static void prizecheck (void)
{
    foreachship(sp) {
	if (!sp->status.captured || sp->status.struck || !sp->status.dir)
	    continue;
	if (sp->specs.crew[0] + sp->specs.crew[1] + sp->specs.crew[2] > sp->status.pcrew * 6) {
	    makemsg (sp, "prize crew overthrown");
	    sp->status.captured = false;
	}
    }
}

static int str_end (const char* str)
{
    size_t sl = strlen (str);
    return sl ? str[sl-1] : 0;
}

static void closeon (struct Ship* from, struct Ship* to, char* command, int ta, int ma, int af)
{
    char temp[10] = "";
    command[0] = '\0';
    int high = -30000;
    try_ship_cmd (from, to, command, temp, ma, ta, af, ma, from->status.dir, &high, 0);
}

static int score (struct Ship* ship, struct Ship* to, char* movement, int onlytemp)
{
    struct ShipStatus* fp = &ship->status;
    uint8_t dir = fp->dir;
    if (!dir)
	return 0;
    int row = fp->row;
    int col = fp->col;
    char drift = fp->drift;
    move_ship(ship, movement, &fp->dir, &fp->row, &fp->col, &drift);
    if (!*movement)
	strcpy(movement, "d");

    int ran = range(ship, to);
    int total = -50 * ran;
    if (ran < 4 && gunsbear(ship, to))
	total += 60;
    if ((ran = portside(ship, to, 1) - fp->dir) == 4 || ran == -4)
	total = -30000;

    if (!onlytemp) {
	fp->row = row;
	fp->col = col;
	fp->dir = dir;
    }
    return total;
}

static void move_ship (struct Ship* ship, const char* p, unsigned char* dir, short *row, short *col, char* drift)
{
    bool moved = false;
    for (; *p; ++p) {
	if (*p == 'r') {
	    if (++*dir == 9)
		*dir = 1;
	} else if (*p == 'l') {
	    if (--*dir == 0)
		*dir = 8;
	} else if (*p >= '1' && *p <= '7') {
	    moved = true;
	    unsigned dist = *p - '0';
	    if (*dir % 2 == 0)
		dist = c_Diagonals[dist];
	    *row -= c_dr[*dir] * dist;
	    *col -= c_dc[*dir] * dist;
	}
    }
    if (!moved) {
	if (_windspeed != 0 && ++*drift > 2) {
	    if ((ship->specs.sclass >= 3 && !snagged(ship))
		|| (_turn & 1) == 0) {
		*row -= c_dr[_winddir];
		*col -= c_dc[_winddir];
	    }
	}
    } else
	*drift = 0;
}

static void try_ship_cmd (struct Ship* f, struct Ship* t, char* command, char* temp, int ma, int ta, int af, int vma, int dir, int* high, int rakeme)
{
    short n;
    if ((n = str_end (temp)) < '1' || n > '9') {
	for (n = 1; vma - n >= 0; ++n) {
	    char st[8];
	    snprintf (ArrayBlock(st), "%hd", n);
	    strcat (temp, st);
	    int new = score (f, t, temp, rakeme);
	    if (new > *high && (!rakeme || (gunsbear(f, t) && !gunsbear(t, f)))) {
		*high = new;
		strcpy(command, temp);
	    }
	    try_ship_cmd (f, t, command, temp, ma - n, ta, af, vma - n, dir, high, rakeme);
	    rmend (temp);
	}
    }
    if ((ma > 0 && ta > 0 && (n = str_end(temp)) != 'l' && n != 'r') || !strlen(temp)) {
	strcat (temp, "r");
	int new = score (f, t, temp, rakeme);
	if (new > *high && (!rakeme || (gunsbear(f, t) && !gunsbear(t, f)))) {
	    *high = new;
	    strcpy (command, temp);
	}
	try_ship_cmd (f, t, command, temp, ma - 1, ta - 1, af, min_i (ma - 1, maxmove(f, (dir == 8 ? 1 : dir + 1), 0)), (dir == 8 ? 1 : dir + 1), high, rakeme);
	rmend (temp);
    }
    if ((ma > 0 && ta > 0 && (n = str_end(temp)) != 'l' && n != 'r') || !strlen(temp)) {
	strcat (temp, "l");
	int new = score (f, t, temp, rakeme);
	if (new > *high && (!rakeme || (gunsbear(f, t) && !gunsbear(t, f)))) {
	    *high = new;
	    strcpy (command, temp);
	}
	try_ship_cmd (f, t, command, temp, ma - 1, ta - 1, af, (min_i (ma - 1, maxmove(f, (dir - 1 ? dir - 1 : 8), 0))), (dir - 1 ? dir - 1 : 8), high, rakeme);
	rmend (temp);
    }
}

static void rmend (char* str)
{
    char* p;
    for (p = str; *p; ++p);
    if (p != str)
	*--p = 0;
}

// move all comp ships
static void moveall (void)
{
    int n;
    int k, l;

    // first try to create moves for OUR ships
    foreachship(sp) {
	struct Ship* closest;
	int ma, ta;
	char af;

	if (sp == _ms || sp->status.dir == 0)
	    continue;
	if (!sp->status.struck && _windspeed && !snagged(sp) && sp->specs.crew[2]) {
	    ta = maxturns(sp, &af);
	    ma = maxmove(sp, sp->status.dir, 0);
	    closest = closestenemy(sp, 0, 0);
	    if (closest == 0)
		*sp->status.movebuf = '\0';
	    else
		closeon(sp, closest, sp->status.movebuf, ta, ma, af);
	} else
	    *sp->status.movebuf = '\0';
    }
    // Then execute the moves for ALL ships (dead ones too),
    // checking for collisions and snags at each step.
    // The old positions are saved in row[], col[], dir[].
    // At the end, we compare and write out the changes.
    n = 0;
    foreachship(sp) {
	if (snagged(sp))
	    strcpy(sp->status.movebuf, "d");
	else if (*sp->status.movebuf != 'd')
	    strcat(sp->status.movebuf, "d");
	++n;
    }
    // Now resolve collisions.
    // This is the tough part.
    bool moved[NSHIP] = {false};
    for (k = 0; stillmoving(k); ++k) {
	// Step once.
	// And propagate the nulls at the end of sp->status.movebuf.
	n = 0;
	foreachship(sp) {
	    if (!sp->status.movebuf[k])
		sp->status.movebuf[k + 1] = '\0';
	    else if (sp->status.dir)
		step(sp, sp->status.movebuf[k], &moved[n]);
	    ++n;
	}
	// The real stuff.
	n = 0;
	foreachship(sp) {
	    if (sp->status.dir == 0 || is_isolated(sp))
		goto cont1;
	    l = 0;
	    foreachship(sq) {
		char snap = 0;

		if (sp == sq)
		    goto cont2;
		if (sq->status.dir == 0)
		    goto cont2;
		if (!push(sp, sq))
		    goto cont2;
		if (snagged2(sp, sq) && range(sp, sq) > 1)
		    ++snap;
		if (!range(sp, sq) && !fouled2(sp, sq)) {
		    makesignal(sp, "collision with $$", sq);
		    if (dieroll() < 4) {
			makesignal(sp, "fouled with $$", sq);
			foul_ship_with (sp, l);
			foul_ship_with (sq, n);
		    }
		    ++snap;
		}
		if (snap) {
		    sp->status.movebuf[k + 1] = 0;
		    sq->status.movebuf[k + 1] = 0;
		    sq->status.row = sp->status.row - 1;
		    if (sp->status.dir == 1 || sp->status.dir == 5)
			sq->status.col = sp->status.col - 1;
		    else
			sq->status.col = sp->status.col;
		    sq->status.dir = sp->status.dir;
		}
	      cont2:
		++l;
	    }
	  cont1:
	    ++n;
	}
    }
    // Clear old moves.  And write out new pos.
    foreachship (sp)
	if (sp->status.dir != 0)
	    *sp->status.movebuf = 0;
}

static bool stillmoving (unsigned k)
{
    foreachship(sp)
	if (sp->status.movebuf[k])
	    return true;
    return false;
}

static bool is_isolated (const struct Ship* ship)
{
    foreachship(sp)
	if (ship != sp && range(ship, sp) <= 10)
	    return false;
    return true;
}

static int push (const struct Ship* from, const struct Ship* to)
{
    if (to->specs.guns > from->specs.guns)
	return 1;
    if (to->specs.guns < from->specs.guns)
	return 0;
    return from < to;
}

static void step (struct Ship* sp, int com, bool* moved)
{
    if (com == 'r') {
	if (++sp->status.dir == 9)
	    sp->status.dir = 1;
    } else if (com == 'l') {
	if (--sp->status.dir == 0)
	    sp->status.dir = 8;
    } else if (com >= '0' && com <= '7') {
	unsigned dist = com - '0';
	if (!(sp->status.dir % 2))
	    dist = c_Diagonals[dist];
	sp->status.row -= c_dr[sp->status.dir] * dist;
	sp->status.col -= c_dc[sp->status.dir] * dist;
	*moved = true;
    } else if (com == 'd') {
	if (!*moved) {
	    if (_windspeed != 0 && ++sp->status.drift > 2 && ((sp->specs.sclass >= 3 && !snagged(sp)) || !(_turn%2))) {
		sp->status.row -= c_dr[_winddir];
		sp->status.col -= c_dc[_winddir];
	    }
	} else
	    sp->status.drift = 0;
    }
}

static void sendbp (struct Ship* from, struct Ship* to, int sections, int isdefense)
{
    struct BP* bp = isdefense ? from->status.DBP : from->status.OBP;
    unsigned n = 0;
    while (n < NBP && bp[n].turnsent)
	++n;
    if (n < NBP && sections) {
	if (isdefense) {
	    ship_set_defensive_parties (from, n, _turn, to->status.index, sections);
	    makemsg (from, "repelling boarders");
	} else {
	    ship_set_offensive_parties (from, n, _turn, to->status.index, sections);
	    makesignal (from, "boarding the $$", to);
	}
    }
}

static unsigned is_toughmelee (const struct Ship* ship, const struct Ship* to, bool isdefense, int count)
{
    unsigned qual = ship->specs.qual;
    unsigned obp = 0;
    const struct BP* bp = isdefense ? ship->status.DBP : ship->status.OBP;
    for (unsigned n = 0; n < NBP; ++n, ++bp) {
	if (bp->turnsent && (to->status.index == bp->toship || isdefense)) {
	    obp += bp->mensent / 100 ? ship->specs.crew[0] * qual : 0;
	    obp += (bp->mensent % 100) / 10 ? ship->specs.crew[1] * qual : 0;
	    obp += bp->mensent % 10 ? ship->specs.crew[2] * qual : 0;
	}
    }
    if (count || isdefense)
	return obp;
    unsigned OBP = is_toughmelee (to, ship, false, count + 1);
    unsigned dbp = is_toughmelee (ship, to,  true, count + 1);
    unsigned DBP = is_toughmelee (to, ship,  true, count + 1);
    return OBP > obp + 10 || OBP + DBP >= obp + dbp + 10;
}

static void reload (void)
{
    foreachship(sp)
	sp->status.loadwith = 0;
}

static void checksails (void)
{
    foreachship(sp) {
	if (sp == _ms)
	    continue;
	int rig = sp->specs.rig[0];
	if (_windspeed == 6 || (_windspeed == 5 && sp->specs.sclass > 4))
	    rig = 0;
	bool full = false;
	if (rig && sp->specs.crew[2]) {
	    struct Ship* close = closestenemy (sp, 0, 0);
	    if (close != 0)
		if (range (sp, close) > 9)
		    full = true;
	}
	if ((sp->status.FS != 0) != full)
	    sp->status.FS = full;
    }
}

static void ungrap (struct Ship* from, struct Ship* to)
{
    int k = grappled2(from, to);
    if (!k)
	return;
    bool friend = shipnationality(from) == shipnationality(to);
    while (--k >= 0) {
	if (friend || dieroll() < 3) {
	    cleangrapple(from, to, 0);
	    makesignal(from, "ungrappling $$", to);
	}
    }
}

static void grap (struct Ship* from, struct Ship* to)
{
    if (shipnationality(from) != shipnationality(to) && dieroll() > 2)
	return;
    grapple_ship_with (from, to->status.index);
    grapple_ship_with (to, from->status.index);
    makesignal (from, "grappled with $$", to);
}

static void subtract (struct Ship* from, int totalfrom, int pcfrom)
{
    if (!from->status.captured && totalfrom) {
	for (unsigned n = 0; n < 3; ++n) {
	    if (totalfrom > from->specs.crew[n]) {
		totalfrom -= from->specs.crew[n];
		from->specs.crew[n] = 0;
	    } else {
		from->specs.crew[n] -= totalfrom;
		totalfrom = 0;
	    }
	}
    } else if (totalfrom) {
	pcfrom -= totalfrom;
	pcfrom = pcfrom < 0 ? 0 : pcfrom;
	from->status.pcrew = pcfrom;
    }
}

// returns # of crew squares sent
static unsigned mensent (const struct Ship* from, const struct Ship* to, bool isdefense)
{
    const struct BP* bp = isdefense ? from->status.DBP : from->status.OBP;
    unsigned men = 0;
    for (unsigned n = 0; n < NBP; ++n, ++bp)
	if (bp->turnsent && bp->toship == to->status.index)
	    men += bp->mensent;
    unsigned c1 = 0, c2 = 0, c3 = 0;
    if (men) {
	c1 = men / 100 ? from->specs.crew[0] : 0;
	c2 = (men % 100) / 10 ? from->specs.crew[1] : 0;
	c3 = men / 10 ? from->specs.crew[2] : 0;
	c3 = from->status.captured ? from->status.pcrew : from->specs.crew[2];
    }
    return c1 + c2 + c3;
}
