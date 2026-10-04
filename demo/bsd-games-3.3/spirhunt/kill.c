// Copyright (c) 1980 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "spirhunt.h"

// KILL KILL KILL !!!
//
// This file handles the killing off of almost anything.
//
// Handle a pirate's death
//
// The pirate at the sector given by the parameters is killed
// and removed from the pirate list. Notice that it is not
// removed from the event list; this is done later, when the
// the event is to be caught. Also, the time left is recomputed,
// and the game is won if that was the last pirate.
//
void kill_pirate (int ix, int iy)
{
    struct Pirate* p = pirate_at (ix, iy);
    if (!p)
	return;
    print_msg ("Pirate at " SECT_FMT " destroyed\n", p->sect.x, p->sect.y);
    *p = Etc.pirate[--current_quad()->pirates];
    ++Game.pirates_killed;

    // find out if that was the last one
    if (!pirates_remaining())
	win();

    // recompute time left
    Now.time = Now.resource / pirates_remaining();
}

// handle a starbase's death
void kill_starbase (int qx, int qy)
{
    struct quad* q = &Quad[qy][qx];
    if (!q->bases)
	return;
    q->bases = 0;
    Now.bases -= 1;
    struct xy* b = Now.base;
    for (;; ++b)
	if (qx == b->x && qy == b->y)
	    break;
    *b = Now.base[Now.bases];
    if (qx == Ship.quad.x && qy == Ship.quad.y) {
	if (Ship.cond == DOCKED)
	    undock();
	print_msg ("Starbase at " QUAD_FMT " destroyed\n", Etc.starbase.x, Etc.starbase.y);
    } else {
	if (!device_damaged (SSRADIO))
	    print_msg ("Headquarters reports that the starbase in\n"
			"   quadrant " QUAD_FMT " has been destroyed\n", qx, qy);
	else
	    schedule (E_PATSB | E_GHOST, FLT_MAX, qx, qy, 0);
    }
}

// kill an inhabited starsystem
// quad coords if f == 0, else sector coords
// f != 0 -- this quad;  f < 0 -- your fault
void kill_starsystem (int x, int y, int f)
{
    struct quad *q;
    if (f) { // current quadrant
	q = current_quad();
	const char* name = systemname(q);
	if (!name)
	    return;
	print_msg ("Inhabited starsystem %s in " SECT_FMT " destroyed\n", name, x, y);
	if (f < 0)
	    Game.killinhab += 1;
    } else // different quadrant
	q = &Quad[y][x];
    if (q->distressed) {
	for (unsigned i = 0; i < ArraySize(Event); ++i)
	    if (Event[i].systemname == q->systemname)
		unschedule (&Event[i]);
	print_msg ("Distress call for %s invalidated\n", systemname(q));
	q->distressed = false;
    }
    q->systemname = 0;
    --q->stars;
}

// "kill" a distress call
// quadrant coordinates
// f - set if user is to be informed
void kill_distress_call (unsigned x, unsigned y, bool f)
{
    Quad[y][x].distressed = false;
    for (unsigned i = 0; i < MAXEVENTS; ++i) {
	struct event* e = &Event[i];
	if (e->quad.x != x || e->quad.y != y)
	    continue;
	if (e->evcode == E_PDESB && f) {
	    print_msg ("Distress call for starbase in %u,%u nullified\n", x, y);
	    unschedule (e);
	} else if (e->evcode == E_ENSLV || e->evcode == E_REPRO) {
	    if (f) {
		print_msg ("Distress call for %s in quadrant %u,%u nullified\n", Systemname[e->systemname], x, y);
		unschedule (e);
	    } else
		e->evcode |= E_GHOST;
	}
    }
}

// CAUSE A NOVA TO OCCUR
//
// A nova occurs. It is the result of having a star hit with a torpedo.
// There are several things which may happen. The star may not be affected.
// It may go nova. It may turn into a black hole. And it may go supernova.
//
// Stars that go nova cause stars which surround them to undergo the same
// probabilistic process. Pirates next to them are destroyed. And if the
// starship is next to it, it gets zapped. If the zap is too much, it
// gets destroyed.
//
void nova (int x, int y)
{
    struct quad* cq = current_quad();
    if (sector_contents(x,y) != STAR || !cq->stars || cq->stars == SUPERNOVA)
	return;

    unsigned willnova = nrand(100);
    if (willnova < 5)
	return snova(x, y);
    else if (willnova < 15)
	return print_msg ("Star at " SECT_FMT " failed to nova.\n", x, y);

    print_msg ("Star at " SECT_FMT " gone nova\n", x, y);
    for (unsigned s = 0; s < cq->stars; ++s) {
	if (Etc.stars[s].x == x && Etc.stars[s].y == y) {
	    struct xy sl = Etc.stars[s];
	    Etc.stars[s] = Etc.stars[--cq->stars];
	    animate_nova (sl);
	}
    }
    ++Game.stars_killed;

    // Nova destroys surrounding sectors
    for (int i = x - 1; i <= x + 1; ++i) {
	if (i < 0 || i >= NSECTS)
	    continue;
	for (int j = y - 1; j <= y + 1; ++j) {
	    if (j < 0 || j >= NSECTS)
		continue;
	    switch (sector_contents(i,j)) {
		case EMPTY:
		case HOLE:	break;
		case PIRATE:	kill_pirate (i, j); break;
		case STAR:	nova (i, j); break;
		case INHABIT:	kill_starsystem (i, j, -1); break;
		case BASE:	kill_starbase (i, j); ++Game.bases_killed; break;
		case YOURSHIP: {
		    int se = 2000;
		    if (Ship.shldup) {
			if (Ship.shield >= se) {
			    Ship.shield -= se;
			    se = 0;
			} else {
			    se -= Ship.shield;
			    Ship.shield = 0;
			}
		    }
		    Ship.energy -= se;
		    if (Ship.energy <= 0)
			lose(L_SUICID);
		    break; }

		default:
		    print_msg ("Unknown object %c at %u,%u destroyed\n", sector_contents(i,j), i, j);
		    break;
	    }
	}
    }
}

// CAUSE SUPERNOVA TO OCCUR
//
// A supernova occurs. If 'ix' < 0, a random quadrant is chosen;
// otherwise, the current quadrant is taken, and (ix, iy) give
// the sector quadrants of the star which is blowing up.
//
// If the supernova turns out to be in the quadrant you are in,
// you go into "emergency override mode", which tries to get you
// out of the quadrant as fast as possible. However, if you
// don't have enough fuel, or if you by chance run into something,
// or some such thing, you blow up anyway. Oh yeh, if you are
// within two sectors of the star, there is nothing that can
// be done for you.
//
// When a star has gone supernova, the quadrant becomes uninhab-
// itable for the rest of eternity, i.e., the game. If you ever
// try stopping in such a quadrant, you will go into emergency
// override mode.

void snova (int x, int y)
{
    int qx, qy;
    int ix, iy = 0;
    int f;
    int dx, dy;
    int n;
    struct quad *q;

    f = 0;
    ix = x;
    if (ix < 0) {
	// choose a quadrant
	while (1) {
	    qx = nrand(NQUADS);
	    qy = nrand(NQUADS);
	    q = &Quad[qy][qx];
	    if (q->stars > 0)
		break;
	}
	if (Ship.quad.x == qx && Ship.quad.y == qy) {
	    // select a particular star
	    n = nrand(q->stars);
	    for (ix = 0; ix < NSECTS; ++ix) {
		for (iy = 0; iy < NSECTS; ++iy)
		    if (sector_contents(ix,iy) == STAR || sector_contents(ix,iy) == INHABIT)
			if ((n -= 1) <= 0)
			    break;
		if (n <= 0)
		    break;
	    }
	    f = 1;
	}
    } else {
	// current quadrant
	iy = y;
	qx = Ship.quad.x;
	qy = Ship.quad.y;
	q = &Quad[qy][qx];
	f = 1;
    }
    if (f) {
	// supernova is in your quadrant
	print_msg ("\nRED ALERT: supernova occuring at " SECT_FMT "\n", ix, iy);
	dx = ix - Ship.sect.x;
	dy = iy - Ship.sect.y;
	if (dx * dx + dy * dy <= 2) {
	    print_msg ("***  Emergency override attem");
	    draw_and_sleep(1);
	    print_msg ("\n");
	    lose(L_SNOVA);
	}
    } else if (!device_damaged (SSRADIO))
	print_msg ("\nCaptain, Command reports a supernova\n"
		"  in quadrant " QUAD_FMT ". Caution is advised\n", qx, qy);

    // clear out the supernova'ed quadrant
    dx = q->pirates;
    dy = q->stars;
    if (x >= 0) {
	// You caused the supernova
	Game.stars_killed += dy;
	if (q->bases)
	    kill_starbase (qx, qy);
	Game.pirates_killed += dx;
    } else if (q->bases)
	kill_starbase (qx, qy);
    kill_distress_call (qx, qy, (x >= 0));
    q->stars = SUPERNOVA;
    q->pirates = 0;
    if (!pirates_remaining()) {
	print_msg ("Lucky devil, that supernova destroyed the last pirate\n");
	win();
    }
}
