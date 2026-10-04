// Copyright (c) 1980 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "spirhunt.h"

//----------------------------------------------------------------------

static float move_ship (int ramflag, struct xy to, float speed);
static void move_to_random_location (void);
static void ram (unsigned ix, unsigned iy);
static void warp (int fl, struct xy to);

//----------------------------------------------------------------------

// Move Under Warp or Impulse Power
//
// 'Ramflag' is set if we are to be allowed to ram stars,
// pirates, etc. This is passed from warp(), which gets it from
// either play() or ram(). Course is the course (0 -> 360) at
// which we want to move. 'Speed' is the speed we
// want to go, and 'time' is the expected time. It
// can get cut short if a long range tractor beam is to occur. We
// cut short the move so that the user doesn't get docked time and
// energy for distance which he didn't travel.
//
// We check the course through the current quadrant to see that he
// doesn't run into anything. After that, though, space sort of
// bends around him. Note that this puts us in the awkward posi-
// tion of being able to be dropped into a sector which is com-
// pletely surrounded by stars. Oh Well.
//
// Note that if your computer is out, you ram things anyway.
//
// Return value is the actual distance travelled, in quads.
//
static float move_ship (int ramflag, struct xy to, float speed)
{
    struct xy from = absolute_sector_coord (Ship.quad, Ship.sect);
    unsigned dist = sector_distance (from, to);

    // check for long range tractor beams during the move
    const struct event* lrtb_event = next_event_of_type (E_LRTB);
    if (lrtb_event) {
        float lrtb_time = lrtb_event->date - Now.date;
	if (lrtb_time >= 0 && current_quad()->pirates < 3) {
	    unsigned dist_to_lrtb = lrtb_time * speed * NSECTS + 1; // +1 to round up
	    if (dist_to_lrtb < dist)
		dist = dist_to_lrtb;
	}
    }

    // now using time
    Move.free = false;

    for (struct line_iterator li = make_line_iterator (from, to);;) {
	advance_line_iterator (&li);
	if (sector_distance (from, li.p) > dist)
	    break;

	struct xy quad = { li.p.x/NSECTS, li.p.y/NSECTS };
	struct xy sect = { li.p.x%NSECTS, li.p.y%NSECTS };

	// Check for quadrant change
	if (quad.x != Ship.quad.x || quad.y != Ship.quad.y) {
	    if (quad.x >= NQUADS || quad.y >= NQUADS)
		break;	// should never happen, but let's not hang
	    Move.newquad = 2;
	    Ship.quad = quad;
	    Ship.sect = sect;
	    initquad (false);
	} else {
	    // Check for obstacles and ramming
	    enum SectorContents sc = sector_contents (sect.x, sect.y);
	    if (sc != EMPTY) {
		if (!device_damaged (COMPUTER) && ramflag <= 0) {
		    print_msg ("Stopped by ");
		    if (sc == STAR)
			print_msg ("star");
		    else if (sc == BASE)
			print_msg ("starbase");
		    else if (sc == HOLE)
			print_msg ("black hole");
		    else if (sc == PIRATE)
			print_msg ("pirate");
		    else if (sc == INHABIT)
			print_msg ("planet");
		    else
			print_msg ("obstacle");
		    print_msg (" at " SECT_FMT "\n", sect.x, sect.y);
		    Ship.energy -= Param.stopengy * speed;
		    break;
		}
		if (sc == HOLE) {
		    print_msg ("You fall into a black hole.\n");
		    move_to_random_location();
		    initquad (false);
		    break;
		}
		ram (sect.x, sect.y);
		break;
	    }
	    Ship.sect = sect;
	}
	draw_screen();
	delay (12);
    }

    // Return the actual distance travelled
    return sector_distance (from, absolute_sector_coord (Ship.quad, Ship.sect)) / (float) NSECTS;
}

// Dump the starship somewhere in the galaxy
//
// Note that the quadrant is NOT initialized here. This must
// be done from the calling routine.
//
// Repair of devices must be deferred.

static void move_to_random_location (void)
{
    Ship.quad.x = nrand (NQUADS);
    Ship.quad.y = nrand (NQUADS);
    Ship.sect.x = nrand (NSECTS);
    Ship.sect.y = nrand (NSECTS);
    float x = 1.5 * fnrand();
    Move.time += x;
    // bump repair dates forward
    for (unsigned i = 0; i < MAXEVENTS; ++i) {
	struct event *e = &Event[i];
	if (e->evcode == E_FIXDV)
	    reschedule (e, (e->date - Now.date) + x);
    }
    events (1);
    print_msg ("You are now in quadrant " QUAD_FMT ". Today is %.2f\n", Ship.quad.x, Ship.quad.y, Now.date);
    Move.time = 0;
}

// RAM SOME OBJECT
//
// You have run into some sort of object. It may be a pirate,
// a star, or a starbase. If you run into a star, you are really
// stupid, because there is no hope for you.
//
// If you run into something else, you destroy that object. You
// also rack up incredible damages.
//
static void ram (unsigned ix, unsigned iy)
{
    print_msg ("RED ALERT: collision imminent\n");
    enum SectorContents c = sector_contents(ix,iy);
    if (c == PIRATE) {
	print_msg ("Your ship rams pirate at " SECT_FMT "\n", ix, iy);
	draw_and_sleep (1);
	kill_pirate (ix, iy);
    } else if (c == STAR || c == INHABIT) {
	print_msg ("Captain, isn't it getting hot in here?\n");
	draw_and_sleep (1);
	print_msg ("Hull temperature approaching 550 Degrees Kelvin.\n");
	draw_and_sleep (1);
	lose (L_STAR);
    } else if (c == BASE) {
	print_msg ("You ran into the starbase at " SECT_FMT "\n", ix, iy);
	draw_and_sleep (1);
	kill_starbase (Ship.quad.x, Ship.quad.y);
	++Game.bases_killed;
    }
    print_msg ("Your ship sustained heavy damage.\n");

    Ship.shldup = false;	// No chance that your shields remained up in all that
    unsigned deaths = min_u (Ship.crew, 10 + nrand(40)); // Crew killed in the collision
    Ship.crew -= deaths;
    Game.deaths += deaths;
    print_msg ("%u casualties reported.\n", deaths);

    // Damage devices with an 80% probability
    for (unsigned i = 0; i < NDEV; ++i)
	if (nrand(100) < 80)
	    damage_device (i, 2.5*fnrand()+0.5);
}

// SET WARP FACTOR
//
// The warp factor is set for future move commands. It is
// checked for consistancy.
void setwarp (void)
{
    float warpfac = getfltpar("Warp factor");
    if (warpfac <= 0)
	return;
    if (warpfac < 1) {
	print_msg ("Minimum warp speed is 1\n");
	return;
    }
    if (warpfac > 9.9) {
	print_msg ("Maximum speed is warp 9.9\n");
	return;
    }
    if (warpfac > 6)
	print_msg ("Damage to warp engines may occur above warp 6\n");
    Ship.warp = warpfac*10;
}

// MOVE UNDER WARP POWER
//
// This is both the "move" and the "ram" commands, differing
// only in the flag 'fl'. It is also used for automatic
// emergency override mode, when 'fl' is < 0 and 'c' and 'd'
// are the course and distance to be moved. If 'fl' >= 0,
// the course and distance are asked of the captain.
//
// The guts of this routine are in the routine move(), which
// is shared with impulse(). Also, the working part of this
// routine is very small; the rest is to handle the slight chance
// that you may be moving at some riduculous speed. In that
// case, there is code to handle time warps, etc.

void dowarp (int fl)
{
    struct xy to = getdest ("Destination");
    if (to.x > NQUADS*NSECTS || to.y > NQUADS*NSECTS)
	return;
    warp (fl, to);
}

static void warp (int fl, struct xy to)
{
    if (Ship.cond == DOCKED) {
	print_msg ("You are docked\n");
	return;
    }
    if (device_damaged (WARP)) {
	announce_device_damage (WARP);
	return;
    }

    struct xy from = absolute_sector_coord (Ship.quad, Ship.sect);
    float dist = sector_distance (from, to) / (float) NSECTS;

    // check to see that we are not using an absurd amount of power
    float power = (dist + 0.05) * (Ship.warp*Ship.warp*Ship.warp)/1000.f;
    unsigned percent = 100 * power / Ship.energy;
    if (percent >= 85) {
	print_msg ("That would consume %u%% of our remaining energy.\n", percent);
	if (!getynpar ("Are you sure"))
	    return;
    }

    // compute the speed we will move at, and the time it will take
    float speed = (Ship.warp*Ship.warp) / (Param.warptime * 100.f);
    float time = dist / speed;

    // check to see that that value is not ridiculous
    percent = 100 * time / Now.time;
    if (percent >= 85) {
	print_msg ("That would take %u%% of our remaining time.\n", percent);
	if (!getynpar("Are you sure"))
	    return;
    }

    // compute how far we will go if we get damages
    if (Ship.warp > 60 && nrand(100) < 20 + 1.5f * (Ship.warp - 60)) {
	float frac = fnrand();
	dist *= frac;
	time *= frac;
	damage_device (WARP, (frac + 1.0) * (Ship.warp/10.f) * (fnrand() + 0.25) * 0.20);
    }

    // do the move
    dist = move_ship (fl, to, speed);
    Move.time = dist / speed;
    Ship.energy -= dist * Ship.warp * Ship.warp * Ship.warp * (Ship.shldup + 1) / 1000.f;

    // test for bizarre events
    if (Ship.warp <= 90)
	return;
    print_msg ("\n\n  ___ Speed exceeding warp nine ___\n\n");
    draw_and_sleep(2);
    print_msg ("Ship's safety systems malfunction\n");
    draw_and_sleep(2);
    print_msg ("Crew experiencing extreme sensory distortion\n");
    draw_and_sleep(4);
    if (nrand(100) >= 100 * dist) {
	print_msg ("Equilibrium restored -- all systems normal\n");
	return;
    }

    // select a bizzare thing to happen to us
    percent = nrand(100);
    if (percent < 70) {
	// time warp
	if (percent < 35 || !Snapshot.now.date) {
	    // positive time warp
	    time = ((Ship.warp - 80)/10.f) * dist * (fnrand() + 1.0);
	    Now.date += time;
	    print_msg ("Positive time portal entered -- we are now in %.2f\n", Now.date);
	    for (unsigned i = 0; i < MAXEVENTS; ++i) {
		percent = Event[i].evcode;
		if (percent == E_FIXDV || percent == E_LRTB)
		    Event[i].date += time;
	    }
	    return;
	}

	// got lucky: a negative time portal
	time = Now.date;
	Now = Snapshot.now;
	memcpy (Quad, Snapshot.quads, sizeof(Quad));
	memcpy (Event, Snapshot.events, sizeof(Event));
	Snapshot.now.date = 0;
	print_msg ("Negative time portal entered -- we are now in %.2f\n", Now.date);
	for (unsigned i = 0; i < MAXEVENTS; ++i)
	    if (Event[i].evcode == E_FIXDV)
		reschedule (&Event[i], Event[i].date - time);
	return;
    }

    // test for just a lot of damage
    if (percent < 80)
	lose (L_TOOFAST);
    print_msg ("Equilibrium restored -- extreme damage occurred to ship systems\n");
    for (unsigned i = 0; i < NDEV; ++i)
	damage_device (i, 3.0*fnrand() + 0.5);
    Ship.shldup = 0;
}

// move under impulse power
void impulse (void)
{
    if (Ship.cond == DOCKED) {
	print_msg ("Sorry captain, but we are still docked.\n");
	return;
    }
    if (device_damaged (IMPULSE)) {
	announce_device_damage (IMPULSE);
	return;
    }
    struct xy to = getdest ("Destination");
    if (to.x > NQUADS*NSECTS || to.y > NQUADS*NSECTS)
	return;
    struct xy from = absolute_sector_coord (Ship.quad, Ship.sect);
    float dist = sector_distance (from, to) / (float) NSECTS;
    unsigned energy = 20 + 100 * dist;
    unsigned percent = 100 * energy / Ship.energy;
    if (percent >= 85) {
	print_msg ("That would consume %u%% of our remaining energy.\n", percent);
	if (!getynpar("Are you sure"))
	    return;
	print_msg ("Aye aye, sir\n");
    }
    const float speed = 0.095;
    float time = dist / speed;
    percent = 100 * time / Now.time;
    if (percent >= 85) {
	print_msg ("That would take %d%% of our remaining time.\n", percent);
	if (!getynpar ("Are you sure"))
	    return;
	print_msg ("(He's finally gone mad)\n");
    }
    dist = move_ship (0, to, speed);
    Move.time = dist / speed;
    Ship.energy -= 20 + 100 * dist;
}

// Automatic Override
//
// If we should be so unlucky as to be caught in a quadrant
// with a supernova in it, this routine is called. It is
// called from checkcond().
//
// It sets you to a random warp (guaranteed to be over 6.0)
// and starts sending you off "somewhere" (whereever that is).
//
// Please note that it is VERY important that you reset your
// warp speed after the automatic override is called. The new
// warp factor does not stay in effect for just this routine.
//
// This routine will never try to send you more than sqrt(2)
// quadrants, since that is all that is needed.
//
void autover (void)
{
    print_msg ("RED ALERT: You are in a supernova quadrant\n"
	    "*** Emergency override attempts to escape\n");
    struct xy escapesect = { nrand (NSECTS), nrand (NSECTS) };
    struct xy escapequad = { Ship.quad.x + nrand(3) - 1, Ship.quad.y + nrand(3) - 1 };
    if (escapequad.x > NQUADS || escapequad.y > NQUADS)
	lose (L_SNOVA);
    Ship.cond = RED;
    Ship.warp = 60 + nrand(20);
    warp (-1, absolute_sector_coord (escapequad, escapesect));
    attack (0);
}

// line_iterator does Bresenham's line algorithm in iterator form
struct line_iterator make_line_iterator (struct xy from, struct xy to)
{
    struct line_iterator r = {};
    r.p = from;
    unsigned adx, ady;
    if (to.x >= from.x) {
	r.xs = 1;
	adx = to.x - from.x;
    } else {
	r.xs = -1;
	adx = from.x - to.x;
    }
    if (to.y >= from.y) {
	r.ys = 1;
	ady = to.y - from.y;
    } else {
	r.ys = -1;
	ady = from.y - to.y;
    }
    r.xmajor = (adx >= ady);
    if (r.xmajor) {
	r.ds = ady;
	r.dl = adx;
    } else {
	r.ds = adx;
	r.dl = ady;
    }
    r.d = -r.dl;
    r.ds *= 2;
    r.dl *= 2;
    return r;
}

void advance_line_iterator (struct line_iterator* li)
{
    li->d += li->ds;
    bool sstep = (li->d >= 0);
    if (sstep)
	li->d -= li->dl;
    if (li->xmajor || sstep)
	li->p.x += li->xs;
    if (!li->xmajor || sstep)
	li->p.y += li->ys;
}

// RETRIEVE THE STARSYSTEM NAME
const char* systemname (const struct quad* q)
{
    return q->systemname ? Systemname[q->systemname] : NULL;
}

// GET SECTOR CONTENTS
//
// Gets the printable char for the contents of sector x,y

enum SectorContents sector_contents (uint8_t x, uint8_t y)
{
    if (x >= NSECTS || y >= NSECTS)
	return EMPTY;
    if (Ship.sect.x == x && Ship.sect.y == y)
	return YOURSHIP;
    const struct quad* q = current_quad();
    if (q->bases && Etc.starbase.x == x && Etc.starbase.y == y)
	return BASE;
    if (q->systemname && Etc.inhabited.x == x && Etc.inhabited.y == y)
	return INHABIT;
    for (unsigned i = 0; i < q->holes; ++i)
	if (Etc.blackholes[i].x == x && Etc.blackholes[i].y == y)
	    return HOLE;
    for (unsigned i = 0; i < q->stars; ++i)
	if (Etc.stars[i].x == x && Etc.stars[i].y == y)
	    return STAR;
    for (unsigned i = 0; i < q->pirates; ++i)
	if (Etc.pirate[i].sect.x == x && Etc.pirate[i].sect.y == y)
	    return PIRATE;
    return EMPTY;
}

// Returns the quad where Ship is
struct quad* current_quad (void)
    { return &Quad[Ship.quad.y][Ship.quad.x]; }

// Distance between sector coordinates
unsigned sector_distance (struct xy a, struct xy b)
{
    unsigned dx = absv(b.x-a.x), dy = absv(b.y-a.y),
	ld = max_u (dx, dy), sd = min_u (dx, dy);
    // straight+diagonal approximation (181 = sqrt(2)*128)
    return (ld-sd)+sd*181/128;
}

// Counts remaining pirates in the galaxy
unsigned pirates_remaining (void)
{
    unsigned n = 0;
    for (unsigned y = 0; y < NQUADS; ++y)
	for (unsigned x = 0; x < NQUADS; ++x)
	    n += Quad[y][x].pirates;
    return n;
}
