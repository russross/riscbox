// Copyright (c) 1983 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "sail.h"

//----------------------------------------------------------------------

static void strike (struct Ship* ship);
static unsigned distance (int dx, int dy);
static int angle (int dr, int dc);

//----------------------------------------------------------------------

void table (struct Ship *from, struct Ship *on, int rig, int shot, int hittable, int roll)
{
    int hhits = 0, chits = 0, ghits = 0, rhits = 0;
    int Ghit = 0, Hhit = 0, Rhit = 0, Chit = 0;
    int guns, car, pc, hull;
    int crew[3];
    int n;
    int rigg[4];
    const char *message;
    const struct Tables *tp;

    pc = on->status.pcrew;
    hull = on->specs.hull;
    crew[0] = on->specs.crew[0];
    crew[1] = on->specs.crew[1];
    crew[2] = on->specs.crew[2];
    rigg[0] = on->specs.rig[0];
    rigg[1] = on->specs.rig[1];
    rigg[2] = on->specs.rig[2];
    rigg[3] = on->specs.rig[3];
    if (shot == L_GRAPE)
	Chit = chits = hittable;
    else {
	tp = &(rig ? c_RigTable : c_HullTable)[hittable][roll - 1];
	Chit = chits = tp->C;
	Rhit = rhits = tp->R;
	Hhit = hhits = tp->H;
	Ghit = ghits = tp->G;
	if (on->status.FS)
	    rhits *= 2;
	if (shot == L_CHAIN) {
	    Ghit = ghits = 0;
	    Hhit = hhits = 0;
	}
    }
    if (on->status.captured) {
	pc -= (chits + 1) / 2;
	chits /= 2;
    }
    for (n = 0; n < 3; ++n)
	if (chits > crew[n]) {
	    chits -= crew[n];
	    crew[n] = 0;
	} else {
	    crew[n] -= chits;
	    chits = 0;
	}
    for (n = 0; n < 3; ++n)
	if (rhits > rigg[n]) {
	    rhits -= rigg[n];
	    rigg[n] = 0;
	} else {
	    rigg[n] -= rhits;
	    rhits = 0;
	}
    if (rigg[3] != -1 && rhits > rigg[3]) {
	rhits -= rigg[3];
	rigg[3] = 0;
    } else if (rigg[3] != -1)
	rigg[3] -= rhits;
    if (rig && !rigg[2] && (!rigg[3] || rigg[3] == -1))
	makemsg(on, "dismasted!");
    if (portside(from, on, 0)) {
	guns = on->specs.gun.r;
	car = on->specs.car.r;
    } else {
	guns = on->specs.gun.l;
	car = on->specs.car.l;
    }
    if (ghits > car) {
	ghits -= car;
	car = 0;
    } else {
	car -= ghits;
	ghits = 0;
    }
    if (ghits > guns) {
	ghits -= guns;
	guns = 0;
    } else {
	guns -= ghits;
	ghits = 0;
    }
    hull -= ghits;
    if (Ghit) {
	if (portside (from, on, 0)) {
	    on->specs.gun.r = guns;
	    on->specs.car.r = car;
	} else {
	    on->specs.gun.l = guns;
	    on->specs.car.l = car;
	}
    }
    hull -= hhits;
    hull = hull < 0 ? 0 : hull;
    if (on->status.captured && Chit)
	on->status.pcrew = pc;
    if (Hhit)
	on->specs.hull = hull;
    if (Chit) {
	on->specs.crew[0] = crew[0];
	on->specs.crew[1] = crew[1];
	on->specs.crew[2] = crew[2];
    }
    if (Rhit) {
	on->specs.rig[0] = rigg[0];
	on->specs.rig[1] = rigg[1];
	on->specs.rig[2] = rigg[2];
	on->specs.rig[3] = rigg[3];
    }
    switch (shot) {
	case L_ROUND:	message = "firing round shot on $$"; break;
	case L_GRAPE:	message = "firing grape shot on $$"; break;
	case L_CHAIN:	message = "firing chain shot on $$"; break;
	case L_DOUBLE:	message = "firing double shot on $$"; break;
	case L_EXPLODE:	message = "exploding shot on $$"; break;
	default:
	    printf ("Unknown shot type %d", shot);
	    exit (EXIT_FAILURE);

    }
    makesignal (from, message, on);
    if (roll == 6 && rig) {
	switch (Rhit) {
	    case 0:	message = "fore topsail sheets parted"; break;
	    case 1:	message = "mizzen shrouds parted"; break;
	    case 2:	message = "main topsail yard shot away"; break;
	    case 4:	message = "fore topmast and foremast shrouds shot away"; break;
	    case 5:	message = "mizzen mast and yard shot through"; break;
	    case 6:	message = "foremast and spritsail yard shattered"; break;
	    case 7:	message = "main topmast and mizzen mast shattered"; break;
	    default:
		printf ("Bad Rhit = %d", Rhit);
		exit (EXIT_FAILURE);
	}
	makemsg (on, "%s", message);
    } else if (roll == 6) {
	switch (Hhit) {
	    case 0:	message = "anchor cables severed"; break;
	    case 1:	message = "two anchor stocks shot away"; break;
	    case 2:	message = "quarterdeck bulwarks damaged"; break;
	    case 3:	message = "three gun ports shot away"; break;
	    case 4:	message = "four guns dismounted"; break;
	    case 5:
		message = "rudder cables shot through";
		on->specs.ta = 0;
		break;
	    case 6:
		message = "shot holes below the water line";
		break;
	    default:
		printf ("Bad Hhit = %d", Hhit);
		exit (EXIT_FAILURE);
	}
	makemsg (on, "%s", message);
    }
    // if (Chit > 1 && on->status.readyL&R_INITIAL && on->status.readyR&R_INITIAL) {
    // --on->specs.qual;
    // if (on->specs.qual <= 0) {
    // makemsg(on, "crew mutinying!");
    // on->specs.qual = 5;
    // on->status.captured = SHIP(on->status.index);
    // } else
    // makemsg(on, "crew demoralized");
    // }
    if (!hull)
	strike (on);
}

void Cleansnag (struct Ship *from, struct Ship *to, int all, int flag)
{
    if (flag & 1) {
	ungrapple_ship_from (from, to->status.index, all);
	ungrapple_ship_from (to, from->status.index, all);
    }
    if (flag & 2) {
	unfoul_ship_from (from, to->status.index, all);
	unfoul_ship_from (to, from->status.index, all);
    }
    if (!snagged2 (from, to)) {
	if (!snagged (from)) {
	    unboard (from, from, 1);	// defense
	    unboard (from, from, 0);	// defense
	} else
	    unboard (from, to, 0);	// offense
	if (!snagged (to)) {
	    unboard (to, to, 1);	// defense
	    unboard (to, to, 0);	// defense
	} else
	    unboard (to, from, 0);	// offense
    }
}

static void strike (struct Ship* ship)
{
    if (ship->status.struck)
	return;
    ship->status.struck = 1;
    unboard (ship, ship, 0);		// all offense
    unboard (ship, ship, 1);		// all defense
    unsigned willsink = dieroll();	// ship may sink or explode
    if (willsink >= 5)
	ship->status.explode = 1;
    else if (willsink >= 3)
	ship->status.sink = 1;
    makemsg (ship, "striking her colours!");
}

unsigned maxturns (const struct Ship* ship, char* af)
{
    unsigned turns = ship->specs.ta;
    *af = (ship->status.drift > 1 && turns);
    if (*af) {
	--turns;
	if (ship->status.FS == 1)
	    turns = 0;
    }
    return turns;
}

int maxmove (const struct Ship *ship, int dir, int fs)
{
    int riggone = 0, flank = 0;
    int mm = ship->specs.bs;
    if (!ship->specs.rig[0])
	++riggone;
    if (!ship->specs.rig[1])
	++riggone;
    if (!ship->specs.rig[2])
	++riggone;
    if (!ship->specs.rig[3])
	++riggone;
    if ((ship->status.FS || fs) && fs != -1) {
	flank = 1;
	mm = ship->specs.fs;
    }
    if (dir == _winddir)
	mm -= 1 + c_WindEffectTable[_windspeed][ship->specs.sclass - 1].B;
    else if (dir == _winddir + 2 || dir == _winddir - 2 || dir == _winddir - 6 || dir == _winddir + 6)
	mm -= 1 + c_WindEffectTable[_windspeed][ship->specs.sclass - 1].C;
    else if (dir == _winddir + 3 || dir == _winddir - 3 || dir == _winddir - 5 || dir == _winddir + 5)
	mm = (flank ? 2 : 1) - c_WindEffectTable[_windspeed][ship->specs.sclass - 1].D;
    else if (dir == _winddir + 4 || dir == _winddir - 4)
	mm = 0;
    else
	mm -= c_WindEffectTable[_windspeed][ship->specs.sclass - 1].A;
    mm -= riggone;
    mm = mm < 0 ? 0 : mm;
    return mm;
}

bool meleeing (const struct Ship* from, const struct Ship* to)
{
    const struct BP* p = from->status.OBP;
    const struct BP* q = p + NBP;
    for (; p < q; ++p)
	if (p->turnsent && p->toship == to->status.index)
	    return true;
    return false;
}

bool boarding (const struct Ship* from, bool isdefense)
{
    const struct BP* p = isdefense ? from->status.DBP : from->status.OBP;
    const struct BP* q = p + NBP;
    for (; p < q; ++p)
	if (p->turnsent)
	    return true;
    return false;
}

void unboard (struct Ship *ship, const struct Ship* to, bool isdefense)
{
    const struct BP* p = isdefense ? ship->status.DBP : ship->status.OBP;
    for (int n = 0; n < NBP; ++p, ++n) {
	if (p->turnsent && (p->toship == to->status.index || isdefense || ship == to)) {
	    if (isdefense)
		ship_set_defensive_parties (ship, n, 0, 0, 0);
	    else
		ship_set_offensive_parties (ship, n, 0, 0, 0);
	}
    }
}

static unsigned distance (int dx, int dy)
    { return abs(dx) >= abs(dy) ? abs(dx) + abs(dy)/2 : abs(dy) + abs(dx)/2; }

int range (const struct Ship* from, const struct Ship* to)
{
    int bow1r, bow1c, bow2r, bow2c;
    int stern1r, stern1c, stern2c, stern2r;
    int bb, bs, sb, ss, result;

    if (!to->status.dir)
	return -1;
    stern1r = bow1r = from->status.row;
    stern1c = bow1c = from->status.col;
    stern2r = bow2r = to->status.row;
    stern2c = bow2c = to->status.col;
    result = bb = distance(bow2r - bow1r, bow2c - bow1c);
    if (bb < 5) {
	stern2r += c_dr[to->status.dir];
	stern2c += c_dc[to->status.dir];
	stern1r += c_dr[from->status.dir];
	stern1c += c_dc[from->status.dir];
	bs = distance((bow2r - stern1r), (bow2c - stern1c));
	sb = distance((bow1r - stern2r), (bow1c - stern2c));
	ss = distance((stern2r - stern1r), (stern2c - stern1c));
	result = min_i (bb, min_i (bs, min_i (sb, ss)));
    }
    return result;
}

struct Ship* closestenemy (const struct Ship* from, int side, bool anyship)
{
    unsigned mindist = UINT_MAX;
    struct Ship* closest = NULL;
    uint8_t a = shipnationality(from);
    foreachship (sp) {
	if (sp == from
	    || !sp->status.dir
	    || (a == shipnationality(sp) && !anyship)
	    || (side && gunsbear(from, sp) != side))
	    continue;
	unsigned dist = range(from, sp);
	if (dist < mindist) {
	    closest = sp;
	    mindist = dist;
	}
    }
    return closest;
}

static int angle (int dr, int dc)
{
    int i;
    if (dc >= 0 && dr > 0)
	i = 0;
    else if (dr <= 0 && dc > 0)
	i = 2;
    else if (dc <= 0 && dr < 0)
	i = 4;
    else
	i = 6;
    dr = abs(dr);
    dc = abs(dc);
    if ((i == 0 || i == 4) && dc * 2.4 > dr) {
	++i;
	if (dc > dr * 2.4)
	    ++i;
    } else if ((i == 2 || i == 6) && dr * 2.4 > dc) {
	++i;
	if (dr > dc * 2.4)
	    ++i;
    }
    return i % 8 + 1;
}

// checks for target bow or stern
int gunsbear (const struct Ship* from, const struct Ship* to)
{
    int dy = from->status.row - to->status.row;
    int dx = to->status.col - from->status.col;
    for (int i = 2; i; --i) {
	int ang = angle(dy, dx);
	if (ang - from->status.dir + 1 < 1)
	    ang += 8;
	if (ang >= 2 && ang <= 4)
	    return 'r';
	if (ang >= 6 && ang <= 7)
	    return 'l';
	dy += c_dr[to->status.dir];
	dx += c_dc[to->status.dir];
    }
    return 0;
}

// returns true if fromship is shooting at onship's starboard side
int portside (const struct Ship* from, const struct Ship* on, int quick)
{
    int dy = from->status.row - on->status.row;
    int dx = on->status.col - from->status.col;
    if (quick == -1) {
	dy += c_dr[on->status.dir];
	dx += c_dc[on->status.dir];
    }
    int ang = angle(dy, dx);
    if (quick != 0)
	return ang;
    ang = (ang + 4 - on->status.dir - 1) % 8 + 1;
    return ang < 5;
}

int colours (const struct Ship* sp)
{
    char flag = '\0';
    if (sp->status.struck)
	flag = '!';
    if (sp->status.explode)
	flag = '#';
    if (sp->status.sink)
	flag = '~';
    if (sp->status.struck)
	return flag;
    flag = *c_CountryName[shipnationality(sp)];
    return sp->status.FS ? flag : tolower(flag);
}

void foul_ship_with (struct Ship* s, unsigned w)
{
    struct snag* p = &s->status.foul[w];
    if (SHIP(w)->status.dir == 0)
	return;
    if (!p->sn_count++)
	p->sn_turn = _turn;
    ++s->status.nfoul;
}

void unfoul_ship_from (struct Ship* s, unsigned fi, bool all)
{
    struct snag* f = &s->status.foul[fi];
    if (f->sn_count > 0) {
	if (all) {
	    s->status.nfoul -= f->sn_count;
	    f->sn_count = 0;
	} else {
	    --s->status.nfoul;
	    --f->sn_count;
	}
    }
}

void ungrapple_ship_from (struct Ship* s, unsigned fi, bool all)
{
    struct snag* f = &s->status.grap[fi];
    if (f->sn_count > 0) {
	if (all) {
	    s->status.ngrap -= f->sn_count;
	    f->sn_count = 0;
	} else {
	    --s->status.ngrap;
	    --f->sn_count;
	}
    }
}

void grapple_ship_with (struct Ship* s, unsigned w)
{
    struct snag* p = &s->status.grap[w];
    if (!SHIP(w)->status.dir)
	return;
    if (!p->sn_count++)
	p->sn_turn = _turn;
    ++s->status.ngrap;
}

void ship_set_defensive_parties (struct Ship* s, unsigned side, unsigned turnsent, unsigned toship, unsigned mensent)
{
    struct BP* p = &s->status.DBP[side];
    p->turnsent = turnsent;
    p->toship = toship;
    p->mensent = mensent;
}

void ship_set_offensive_parties (struct Ship* s, unsigned side, unsigned turnsent, unsigned toship, unsigned mensent)
{
    struct BP* p = &s->status.OBP[side];
    p->turnsent = turnsent;
    p->toship = toship;
    p->mensent = mensent;
}
