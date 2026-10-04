// Copyright (c) 1980 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "spirhunt.h"

//----------------------------------------------------------------------

static void long_range_tractor_beam (void);
static void update_galaxy_snapshot (void);

//----------------------------------------------------------------------

// CAUSE TIME TO ELAPSE
//
// This routine does a hell of a lot. It elapses time, eats up energy,
// regenerates energy, processes any events that occur, and so on.
//
int events (bool in_time_warp)
{
    // if nothing happened, just allow for any pirates killed
    if (Move.time <= 0.0) {
	Now.time = Now.resource / pirates_remaining();
	return 0;
    }

    // indicate that the cloaking device is now working
    Ship.cloakgood = true;

    // idate is the initial date
    const float idate = Now.date;

    // schedule attacks if resting too long
    if (Move.time > 0.5 && Move.resting)
	schedule (E_ATTACK, 0.5, 0, 0, 0);

    // scan the event list
    for (;;) {
	bool restcancel = false;
	int evnum = -1;
	// xdate is the date of the current event
	float xdate = idate + Move.time;

	// find the first event that has happened
	struct event* e = NULL;
	struct event* ev = NULL;
	for (unsigned i = 0; i < MAXEVENTS; ++i) {
	    e = &Event[i];
	    if (e->evcode == 0 || (e->evcode & E_GHOST))
		continue;
	    if (e->date < xdate) {
		xdate = e->date;
		ev = e;
		evnum = i;
	    }
	}
	e = ev;

	// find the time between events
	float rtime = xdate - Now.date;

	// decrement the magic "Galactic Resources" pseudo-variable
	Now.resource -= pirates_remaining() * rtime;
	// and recompute the time left
	Now.time = Now.resource / pirates_remaining();

	// move us up to the next date
	Now.date = xdate;

	// check for out of time
	if (Now.time <= 0.0)
	    lose(L_NOTIME);

	// if evnum < 0, no events occurred
	if (evnum < 0)
	    break;

	// otherwise one did. Find out what it is
	switch (e->evcode & E_EVENT) {
	    case E_SNOVA:     // supernova
		snova (-1, 0);			// cause the supernova to happen
		xresched (e, E_SNOVA, 1);	// and schedule the next one
		break;

	    case E_LRTB:      // long range tractor beam
		// schedule the next one
		xresched (e, E_LRTB, pirates_remaining());
		// LRTB cannot occur if we are docked
		if (Ship.cond != DOCKED) {
		    long_range_tractor_beam();
		    Move.time = xdate - idate; // truncate the move time
		}
		break;

	    case E_PATSB:     // pirate attacks starbase
		// if out of bases, forget it
		if (Now.bases <= 0)
		    unschedule (e);
		else {
		    // check for starbase and pirates in same quadrant
		    unsigned attacked_base = 0;
		    for (; attacked_base < Now.bases; ++attacked_base) {
			const struct xy* pbl = &Now.base[attacked_base];
			// see if a pirate exists in this quadrant
			const struct quad* q = &Quad[pbl->y][pbl->x];
			if (q->pirates <= 0)
			    continue;

			// see if already distressed
			unsigned j = 0;
			for (; j < MAXEVENTS; ++j) {
			    const struct event* de = &Event[j];
			    if ((de->evcode & E_EVENT) != E_PDESB)
				continue;
			    if (de->quad.x == pbl->x && de->quad.y == pbl->y)
				break;
			}
			if (j < MAXEVENTS)
			    continue;

			// got a potential attack
			break;
		    }
		    e = ev;
		    if (attacked_base >= Now.bases) {
			// not now; wait a while and see if some pirates move in
			reschedule (e, 3*fnrand());
			break;
		    }
		    // schedule a new attack, and a destruction of the base
		    xresched (e, E_PATSB, 1);
		    e = xsched (E_PDESB, 1, Now.base[attacked_base].x, Now.base[attacked_base].y, 0);
		    if (!e)
			break;

		    // report it if we can
		    if (!device_damaged (SSRADIO)) {
			print_msg ("\nCaptain, we have received a distress signal\n"
				    "  from the starbase in quadrant " QUAD_FMT ".\n",
				    Now.base[attacked_base].x, Now.base[attacked_base].y);
			restcancel = true;
		    } else // SSRADIO out, make it so we can't see the distress call
			e->evcode |= E_HIDDEN;
		}
		break;

	    case E_PDESB:     // pirate destroys starbase
		unschedule (e);
		if (Quad[e->quad.y][e->quad.x].bases > 0 && Quad[e->quad.y][e->quad.x].pirates > 0) {
		    if (e->quad.x == Ship.quad.x && e->quad.y == Ship.quad.y)
			print_msg ("\n"); // message if in this quadrant
		    kill_starbase (e->quad.x, e->quad.y);
		}
		break;

	    case E_ISSUE:     // issue a distress call
		xresched (e, E_ISSUE, 1);
		// try a whole bunch of times to find something suitable
		for (unsigned i = 0; i < NQUADS*NQUADS; ++i) {
		    uint8_t ix = nrand(NQUADS);
		    uint8_t iy = nrand(NQUADS);
		    struct quad* q = &Quad[iy][ix];
		    // need a quadrant which is not the current one,
		    // which has some stars which are inhabited and
		    // not already under attack, which is not
		    // supernova'ed, and which has some pirates in it
		    if (ix != Ship.quad.x && iy != Ship.quad.y && q->stars != SUPERNOVA && !q->distressed && q->systemname && q->pirates) {
			// got one!!  Schedule its enslavement
			e = xsched (E_ENSLV, 1, ix, iy, q->systemname);
			if (!e)
			    break;
			q->distressed = true;
			++Ship.distressed;

			// tell the captain about it if we can
			if (!device_damaged (SSRADIO)) {
			    print_msg ("\nCaptain, starsystem %s in quadrant " QUAD_FMT " is under attack\n", Systemname[e->systemname], ix, iy);
			    restcancel = true;
			} else
			    e->evcode |= E_HIDDEN;
		    }
		}
		break;

	    case E_ENSLV:     // starsystem is enslaved
		unschedule (e);
		// see if current distress call still active
		if (Quad[e->quad.y][e->quad.x].pirates > 0) {
		    // Enslaved starsystems create more pirates
		    struct event* re = schedule (E_REPRO, Param.eventdly[E_REPRO] * fnrand(), e->quad.x, e->quad.y, e->systemname);
		    if (!re)
			break;
		    // Report the disaster if we can
		    if (!device_damaged (SSRADIO))
			print_msg ("\nWe've lost contact with starsystem %s\nin quadrant " QUAD_FMT ".\n", Systemname[re->systemname], re->quad.x, re->quad.y);
		    else
			re->evcode |= E_HIDDEN;
		}
		break;

	    case E_REPRO: { // pirate reproduces
		// see if distress call is still active
		struct quad* q = &Quad[e->quad.y][e->quad.x];
		if (q->pirates <= 0) {
		    unschedule (e);
		    break;
		}
		xresched (e, E_REPRO, 1);
		// reproduce one pirate
		uint8_t ix = e->quad.x;
		uint8_t iy = e->quad.y;
		if (pirates_remaining() == INT8_MAX)
		    break;     // full right now
		if (q->pirates >= QUAD_PIRATES) {
		    // this quadrant not ok, pick an adjacent one
		    int i, j = 0;
		    for (i = iy - 1; i <= iy + 1; ++i) {
			if (i < 0 || i >= NQUADS)
			    continue;
			for (j = ix - 1; j <= ix + 1; ++j) {
			    if (j < 0 || j >= NQUADS)
				continue;
			    q = &Quad[i][j];
			    // check for this quad ok (not full & no snova)
			    if (q->pirates >= QUAD_PIRATES || q->stars == SUPERNOVA)
				continue;
			    break;
			}
			if (j <= ix + 1)
			    break;
		    }
		    if (j > ix + 1)
			break; // cannot create another yet
		    iy = i;
		    ix = j;
		}
		// deliver the child
		++q->pirates;
		if (ix == Ship.quad.x && iy == Ship.quad.y) {
		    struct Pirate* p = &Etc.pirate[q->pirates-1];
		    p->sect = random_empty_sector();
		    p->power = Param.piratepwr;
		    sort_pirates();
		}

		// recompute time left
		Now.time = Now.resource / pirates_remaining();
		} break;

	    case E_SNAP:      // take a snapshot of the galaxy
		xresched (e, E_SNAP, 1);
		update_galaxy_snapshot();
		break;

	    case E_ATTACK:    // pirates attack during rest period
		if (Move.resting) {
		    attack (1);
		    reschedule (e, 0.5);
		} else
		    unschedule (e);
		break;

	    case E_FIXDV:
		print_msg ("Repair work on the %s is finished.\n", DeviceName[e->systemname]);
		if (e->systemname == SSRADIO)
		    restcancel = output_hidden_distress_calls();
		else if (e->systemname == LIFESUP)
		    Ship.reserves = Param.reserves;
		unschedule (e);
		break;
	}
	if (restcancel && Move.resting)
	    Move.time = xdate - idate;
    }

    // unschedule an attack during a rest period
    for (struct event* e; (e = next_event_of_type (E_ATTACK));)
	unschedule (e);

    if (!in_time_warp) {
	// eat up energy if cloaked
	if (Ship.cloaked)
	    Ship.energy -= Param.cloakenergy * Move.time;

	// regenerate resources
	float rtime = Move.time / 5.f;
	Ship.shield += (Param.shield - Ship.shield) * rtime;
	Ship.energy += (Param.energy - Ship.energy) * rtime;

	// decrement life support reserves
	if (device_damaged (LIFESUP) && Ship.cond != DOCKED)
	    Ship.reserves -= Move.time;
    }
    return 0;
}

static void long_range_tractor_beam (void)
{
    // pick a new quadrant
    unsigned ix, iy;
    do {
	int i = nrand(pirates_remaining()) + 1;
	for (ix = 0; ix < NQUADS; ++ix) {
	    for (iy = 0; iy < NQUADS; ++iy) {
		const struct quad *q = &Quad[iy][ix];
		if (q->stars != SUPERNOVA)
		    if ((i -= q->pirates) <= 0)
			break;
	    }
	    if (i <= 0)
		break;
	}
    } while (Ship.quad.x == ix && Ship.quad.y == iy);

    Ship.quad.x = ix;
    Ship.quad.y = iy;
    Ship.sect.x = nrand (NSECTS);
    Ship.sect.y = nrand (NSECTS);
    print_msg ("\nYour ship is caught in long range tractor beam\n"
	    "*** Pulled to quadrant %u,%u\n", Ship.quad.x, Ship.quad.y);
    initquad (false);
}

static void update_galaxy_snapshot (void)
{
    Snapshot.now = Now;
    memcpy (Snapshot.events, Event, sizeof(Snapshot.events));
    memcpy (Snapshot.quads, Quad, sizeof(Snapshot.quads));
}

bool output_hidden_distress_calls (void)
{
    bool emergency = false;
    for (unsigned j = 0; j < MAXEVENTS; ++j) {
	const struct event* e = &Event[j];
	if (!(e->evcode & E_HIDDEN))
	    continue; // not hidden
	if (e->evcode & E_GHOST) {
	    print_msg ("Starsystem %s in quadrant %u,%u is no longer distressed\n", systemname(&Quad[e->quad.y][e->quad.x]), e->quad.x, e->quad.y);
	    unschedule (&Event[j]);
	} else if (e->evcode == E_PDESB) {
	    print_msg ("Starbase in quadrant %u,%u is under attack\n", e->quad.x, e->quad.y);
	    emergency = true;
	} else if (e->evcode == E_ENSLV || e->evcode == E_REPRO) {
	    print_msg ("Starsystem %s in quadrant %u,%u is distressed\n", systemname(&Quad[e->quad.y][e->quad.x]), e->quad.x, e->quad.y);
	    emergency = true;
	}
    }
    return emergency;
}

// SCHEDULE AN EVENT
//
// An event of type 'type' is scheduled for time NOW + 'offset'
// into the first available slot. 'x', 'y', and 'z' are
// considered the attributes for this event.
//
// The address of the slot is returned.
//
struct event* schedule (enum EventType type, float offset, uint8_t x, uint8_t y, uint8_t sysname)
{
    float date = Now.date + offset;
    for (unsigned i = 0; i < MAXEVENTS; ++i) {
	struct event *e = &Event[i];
	if (e->evcode)
	    continue;
	// got a slot
	e->evcode = type;
	e->date = date;
	e->quad.x = x;
	e->quad.y = y;
	e->systemname = sysname;
	return e;
    }
    return NULL;
}

// UNSCHEDULE AN EVENT
//
// The event at slot 'e' is deleted.
void unschedule (struct event* e)
{
    e->date = FLT_MAX;
    e->evcode = 0;
}

// NEXT EVENT
//
// Finds the next event of the given type
struct event* next_event_of_type (enum EventType type)
{
    struct event* nearest = NULL;
    for (unsigned i = 0; i < MAXEVENTS; ++i)
	if (Event[i].evcode == type && (!nearest || Event[i].date < nearest->date))
	    nearest = &Event[i];
    return nearest;
}

static float random_time_offset (enum EventType ev, unsigned factor)
{
    // c_offac is -8*ln(i/16), to replace original log computation
    static const uint8_t c_offac[] = { 22, 17, 13, 11, 9, 8, 7, 6, 5, 4, 3, 2, 2, 1, 1, 1 };
    return Param.eventdly[ev] * Param.time * c_offac[nrand(ArraySize(c_offac))] / (8*factor);
}

// Abreviated schedule routine
//
// Parameters are the event index and a factor for the time figure.
struct event* xsched (enum EventType ev, unsigned factor, uint8_t x, uint8_t y, uint8_t sysname)
{
    return schedule (ev, random_time_offset (ev, factor), x, y, sysname);
}

// Simplified reschedule routine
//
// Parameters are the event index, the initial date, and the
// division factor. Look at the code to see what really happens.
//
void xresched (struct event* e, enum EventType ev, unsigned factor)
{
    reschedule (e, random_time_offset (ev, factor));
}
