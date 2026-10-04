// Copyright (c) 1980 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "spirhunt.h"

static int compare_pirates (const void* vp1, const void* vp2)
{
    const struct Pirate *p1 = vp1, *p2 = vp2;
    return sign (sector_distance (p1->sect, Ship.sect) - sector_distance (p2->sect, Ship.sect));
}

// sort pirates by distance
void sort_pirates (void)
{
    qsort (Etc.pirate, current_quad()->pirates, sizeof(Etc.pirate[0]), compare_pirates);
}

static unsigned ship_device_to_damage (void)
{
    //{{{ Damage probability per ship device
    // these probabilities must sum to 1000
    static const uint8_t c_damprob [NDEV] = {
	110,	// warp drive		11.0%
	110,	// short range scanners	11.0%
	130,	// long range scanners	13.0%
	125,	// plasers		12.5%
	125,	// torpedoes		12.5%
	75,	// impulse engines	 7.5%
	150,	// shield control	15.0%
	20,	// computer		 2.0%
	35,	// radio		 3.5%
	30,	// life support		 3.0%
	90	// cloak		 9.0%
    };
    //}}}
    // select a device from probability vector
    unsigned l = 0;
    for (unsigned p = nrand(1000); p > c_damprob[l]; ++l)
	p -= c_damprob[l];
    return l;
}

struct Pirate* pirate_at (unsigned x, unsigned y)
{
    for (unsigned i = 0; i < current_quad()->pirates; ++i)
	if (Etc.pirate[i].sect.x == x && Etc.pirate[i].sect.y == y)
	    return &Etc.pirate[i];
    return NULL;
}

static void move_pirates (bool after_attack)
{
    for (int n = 0; n < current_quad()->pirates; ++n) {
	struct Pirate* p = &Etc.pirate[n];

	// Calculate desired move distance.
	int sdx = Ship.sect.x - p->sect.x;
	int sdy = Ship.sect.y - p->sect.y;

	unsigned move_range = NSECTS/2;
	unsigned forward_bias = move_range*p->power/Param.piratepwr;

	// Moves toward player before attack, away after attack.
	int dx, dy;
	if (after_attack) {
	    dx = -nrand (max_u (move_range-forward_bias, 1));
	    dy = -nrand (max_u (move_range-forward_bias, 1));
	} else {
	    dx = nrand (min_u (forward_bias, absv(sdx)+1));
	    dy = nrand (min_u (forward_bias, absv(sdy)+1));
	}
	if (sdx < 0)
	    dx = -dx;	// Ship.sect.x < p->sect.x, dx toward ship is negative
	if (sdy < 0)
	    dy = -dy;

	// Clip against edges, adjusting to intersection
	struct xy nquad = Ship.quad;
	struct xy nsect = p->sect;
	if (nsect.x + dx < 0) {
	    if (nquad.x)
		--nquad.x;
	    int cdx = -nsect.x;
	    if (dx)
		dy = dy*cdx/dx;
	    dx = cdx;
	} else if (nsect.x + dx >= NSECTS) {
	    if (nquad.x+1 < NQUADS)
		++nquad.x;
	    int cdx = (NSECTS-1)-nsect.x;
	    if (dx)
		dy = dy*cdx/dx;
	    dx = cdx;
	}
	if (nsect.y + dy < 0) {
	    if (nquad.y)
		--nquad.y;
	    int cdy = -nsect.y;
	    if (dy)
		dx = dx*cdy/dy;
	    dy = cdy;
	} else if (nsect.y + dy >= NSECTS) {
	    if (nquad.y+1)
		++nquad.y;
	    int cdy = (NSECTS-1)-nsect.y;
	    if (dy)
		dx = dx*cdy/dy;
	    dy = cdy;
	}
	nsect.x += dx;
	nsect.y += dy;

	// Trace the move
	for (struct line_iterator li = make_line_iterator (p->sect, nsect); li.p.x != nsect.x || li.p.y != nsect.y;) {
	    advance_line_iterator (&li);
	    if (li.p.x >= NSECTS || li.p.y >= NSECTS)
		break;
	    if (EMPTY != sector_contents (li.p.x, li.p.y)) {
		nquad = Ship.quad; // hit something, can't leave
		break;
	    }
	    p->sect = li.p;
	    draw_screen();
	    delay (12);
	}

	// Check for quadrant change
	if ((nquad.x != Ship.quad.x || nquad.y != Ship.quad.y)
		&& Quad[nquad.x][nquad.y].pirates < QUAD_PIRATES
		&& Quad[nquad.x][nquad.y].stars != SUPERNOVA) {
	    if (!device_damaged (SRSCAN))
		print_msg ("Pirate at " SECT_FMT " escapes to quadrant " QUAD_FMT "\n", p->sect.x, p->sect.y, nquad.x, nquad.y);
	    *p = Etc.pirate[--current_quad()->pirates];
	    p = NULL;
	    ++Quad[nquad.y][nquad.x].pirates;
	}
    }
    sort_pirates();
}

// Pirate Attack Routine
//
// This routine performs the pirate attack provided that
// (1) Something happened this move (i.e., not free), and
// (2) You are not cloaked. Note that if you issue the
// cloak command, you are not considered cloaked until you
// expend some time.
//
// Pirates are permitted to move both before and after the
// attack. They will tend to move toward you before the
// attack and away from you after the attack.
//
// Under certain conditions you can get a critical hit. This
// sort of hit damages devices. The probability that a given
// device is damaged depends on the device. Well protected
// devices (such as the computer, which is in the core of the
// ship and has considerable redundancy) almost never get
// damaged, whereas devices which are exposed (such as the
// warp engines) or which are particularly delicate (such as
// the cloak) have a much higher probability of being damaged.
//
// Casualties can also occur.
//
void attack (int resting)
{
    if (Move.free)
	return;
    if (!current_quad()->pirates || current_quad()->stars == SUPERNOVA)
	return;
    if (Ship.cloaked && Ship.cloakgood)
	return;

    // Move before attack
    move_pirates (false);
    if (Ship.cond == DOCKED) {
	if (!resting)
	    print_msg ("You are protected by starbase shields\n");
	return;
    }

    // Shields do not raise instantaneously, with effectiveness increasing over time
    float chgfac = 1.0;
    if (Move.shldchg)
	chgfac = 0.25 + 0.50 * fnrand();

    unsigned maxhit = 0, tothit = 0;

    // Let each pirate do his damndest
    for (int i = 0; i < current_quad()->pirates; ++i) {
	struct Pirate* p = &Etc.pirate[i];
	if (p->power < 20)
	    continue; // no power to attack with
	if (!device_damaged (SRSCAN))
	    print_msg ("Pirate at " SECT_FMT " ", p->sect.x, p->sect.y);

	// Run a line to see what gets hit
	struct line_iterator li = make_line_iterator (p->sect, Ship.sect);
	enum SectorContents sc = EMPTY;
	do {
	    advance_line_iterator (&li);
	} while (li.p.x < NSECTS && li.p.y < NSECTS && ((sc = sector_contents (li.p.x, li.p.y)) == EMPTY || !nrand(32)));

	animate_plaser (p->sect, Ship.sect);
	if (!device_damaged (SRSCAN)) {
	    if (sc == EMPTY)
		print_msg ("misses\n");
	    else if (sc == YOURSHIP)
		print_msg ("hits you: ");
	    else if (sc == STAR)
		print_msg ("hits a star\n");
	    else if (sc == BASE)
		print_msg ("hits the starbase\n");
	    else if (sc == INHABIT)
		print_msg ("hits the planet\n");
	    else if (sc == HOLE)
		print_msg ("hits a black hole\n");
	    else if (sc == PIRATE)
		print_msg ("hits his buddy at " SECT_FMT "\n", li.p.x, li.p.y);
	}

	// Calculate effectiveness
	unsigned eff = plaser_effectiveness (sector_distance (p->sect, Ship.sect));
	unsigned hit = p->power * (eff - nrand(eff/8)) / 128;
	p->power *= Param.plasfac * (1.0 + (fnrand() - 0.5) * 0.2);

	// Friendly fire (heh heh)
	if (sc == PIRATE) {
	    struct Pirate* dp = pirate_at (li.p.x, li.p.y);
	    if (dp) {
		dp->power -= min_u (dp->power, hit);
		if (!dp->power)
		    kill_pirate (dp->sect.x, dp->sect.y);
	    }
	}

	if (sc != YOURSHIP)
	    continue;

	// See how much of hit shields will absorb
	unsigned shldabsb = 0;
	if (Ship.shldup || Move.shldchg) {
	    shldabsb = min_u (chgfac * hit * Ship.shield / Param.shield, Ship.shield);
	    Ship.shield -= shldabsb;
	    hit -= shldabsb;
	}
	hit = min_u (hit, Ship.energy);

	// Keep track of hits to assess casualties
	tothit += hit;
	if (hit > maxhit)
	    maxhit = hit;

	// Actually do the hit
	print_msg ("%u unit hit", hit+shldabsb);
	if (shldabsb > 0)
	    print_msg (", shields absorb %u%%", shldabsb*100/(hit+shldabsb));
	print_msg ("\n");
	if (!(Ship.energy -= hit))
	    return lose (L_DSTRYD);

	// Check damages
	if (hit >= 13*(25-nrand(12))) {
	    print_msg ("CRITICAL HIT!!!\n");
	    unsigned l = ship_device_to_damage();
	    damage_device (l, hit / (150 + nrand(50)) + 0.5);
	    if (device_damaged (SHIELD)) {
		if (Ship.shldup)
		    print_msg ("Shields knocked down, captain.\n");
		Ship.shield = 0;
		Ship.shldup = 0;
		Move.shldchg = 0;
	    }
	}
    }

    // Big hits, or a lot of smaller ones, kill crew
    if (maxhit >= 200 || tothit >= 500) {
	unsigned cas = min_u (tothit * fnrand() * 0.015, Ship.crew);
	if (cas >= 2) {
	    print_msg ("%u casualties reported.\n", cas);
	    Game.deaths += cas;
	    Ship.crew -= cas;
	}
    }

    // Allow pirates to move after attacking
    move_pirates (true);
}

// Ask a pirate To Surrender (Fat chance)
//
// The Subspace Radio is needed to ask a pirate if he will kindly
// surrender. A random pirate from the ones in the quadrant is
// chosen.
//
// The pirate is requested to surrender. The probability of this
// is a function of that pirate's remaining power, our power, etc.
//
void capture (void)
{
    if (device_damaged (SSRADIO)) {
	announce_device_damage (SSRADIO);
	return;
    }
    if (Ship.cloaked) {
	print_msg ("Pirates don't talk to ghosts\n");
	return;
    }
    if (!current_quad()->pirates) {
	print_msg ("Getting no response, sir\n");
	return;
    }
    if (Ship.brigfree < Param.pirate_crew) {
	print_msg ("Captain, our brig is already full of pirates\n");
	return;
    }

    Move.free = false;
    Move.time = 0.05;

    // See if any pirate wants to surrender
    struct Pirate* sp = NULL;
    for (unsigned i = 0, npirates = current_quad()->pirates; i < npirates; ++i) {
	if (nrand(100) < Param.piratepwr * Ship.energy / (Etc.pirate[i].power * npirates * 1024)) {
	    sp = &Etc.pirate[i];
	    break;
	}
    }
    if (!sp) {
	print_msg ("Captain, the pirates said... uh, I don't think I can repeat that\n");
	return; // big surprise, he refuses to surrender
    }

    // Unexpectedly, the pirate surrendered
    print_msg ("Pirate at " SECT_FMT " surrenders\n", sp->sect.x, sp->sect.y);
    unsigned captives = Param.pirate_crew;
    if (!nrand(8)) {
	captives -= nrand (Param.pirate_crew/2);
	print_msg ("%u pirates commit suicide rather than be taken captive\n", Param.pirate_crew - captives);
    }
    Ship.brigfree -= captives;
    print_msg ("%d captives taken\n", captives);
    kill_pirate (sp->sect.x, sp->sect.y);
}
