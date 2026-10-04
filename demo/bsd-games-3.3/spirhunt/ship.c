// Copyright (c) 1980 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "spirhunt.h"

//{{{ Ship devices -----------------------------------------------------

// CHECK IF A DEVICE IS OUT
//
// The indicated device is checked to see if it is disabled. If
// it is, an attempt is made to use the starbase device. If both
// of these fails, it returns non-zero (device is REALLY out),
// otherwise it returns zero (I can get to it somehow).
//
// It prints appropriate messages too.
//
bool device_works (enum ShipDevice d)
{
    // check for device ok
    if (!device_damaged (d))
	return true;
    announce_device_damage (d);	// report it as being dead
    if (Ship.cond == DOCKED) {
	print_msg ("  Using starbase %s\n", DeviceName[d]);
	return true;
    }
    return false;
}

// damage control report
//
// Print damages and time to fix. This is taken from the event
// list. A couple of factors are set up, based on whether or not
// we are docked. (One of these factors will always be 1.0.)
// The event list is then scanned for damage fix events, the
// time until they occur is determined, and printed out. The
// magic number DAMFAC is used to tell how much faster you can
// fix things if you are docked.

void damage_report (void)
{
    // Set up the magic factors to output repair time
    float m1 = 1, m2 = Param.dockfac;
    if (Ship.cond == DOCKED) {
	m1 /= Param.dockfac;
	m2 = 1;
    }

    // scan for damages
    bool allok = true;
    for (unsigned i = 0; i < MAXEVENTS; ++i) {
	const struct event* e = &Event[i];
	if (e->evcode != E_FIXDV)
	    continue;
	// output the title first time
	if (allok) {
	    allok = false;
	    print_msg ("Damage control report:\n"
		    "\t\t\t  repair times\n"
		    "device\t\t\tin flight  docked\n");
	}
	// compute time till fixed, then adjust by the magic factors
	float x = e->date - Now.date;
	print_msg ("%-24s%7.2f  %7.2f\n", DeviceName[e->systemname], x*m1 + 0.005, x*m2 + 0.005);
    }

    // if everything was ok, reassure the nervous captain
    if (allok)
	print_msg ("All devices functional\n");
}

// Schedule Ship.damages to a Device
//
// Device 'dev1' is damaged in an amount 'dam'. Dam is measured
// in days, and is an additional amount of damage. It should
// be the amount to occur in non-docked mode. The adjustment
// to docked mode occurs automatically if we are docked.
//
// Note that the repair of the device occurs on a DATE, meaning
// that the dock() and undock() have to reschedule the event.
//
// dev1 - device index
// dam - time to repair
//
void damage_device (enum ShipDevice dev, float dam)
{
    if (dam <= 0)
	return; // ignore zero damages
    print_msg ("\t%s damaged\n", DeviceName[dev]);

    // find actual length till it will be fixed
    if (Ship.cond == DOCKED)
	dam *= Param.dockfac;
    // set the damage flag
    int f = device_damaged (dev);
    if (!f) {
	// new damages -- schedule a fix
	schedule(E_FIXDV, dam, 0, 0, dev);
	return;
    }
    // if device already damaged, add to existing damage
    for (unsigned i = 0; i < MAXEVENTS; ++i) {
	struct event* e = &Event[i];
	if (e->evcode != E_FIXDV || e->systemname != dev)
	    continue;
	// got the right one; add on the new damages
	reschedule (e, e->date - Now.date + dam);
	return;
    }
}

// This is a boolean function which returns non-zero if the
// specified device is broken. It does this by checking the
// event list for a "device fix" action on that device.
//
bool device_damaged (enum ShipDevice d)
{
    for (unsigned i = 0; i < MAXEVENTS; ++i)
	if (Event[i].evcode == E_FIXDV && Event[i].systemname == d)
	    return true;
    return false;	// device fix not in event list -- device must not be broken
}

// Announce Device Out
void announce_device_damage (enum ShipDevice dev)
{
    const char* d = DeviceName[dev];
    print_msg ("%s ", d);
    if (d[strlen(d)-1] == 's')
	print_msg ("are");
    else
	print_msg ("is");
    print_msg (" damaged\n");
}

//}}}-------------------------------------------------------------------
//{{{ Docking and resting

static bool adjacent_to_starbase (void)
{
    return current_quad()->bases
	&& absv (Ship.sect.x - Etc.starbase.x) <= 1
	&& absv (Ship.sect.y - Etc.starbase.y) <= 1;
}

// DOCK TO STARBASE
//
// The starship is docked to a starbase. For this to work you
// must be adjacent to a starbase.
//
// You get your supplies replenished and your captives are
// disembarked. Note that your score is updated now, not when
// you actually take the captives.
//
// Any repairs that need to be done are rescheduled to take
// place sooner. This provides for the faster repairs when you
// are docked.

void dock (void)
{
    if (Ship.cond == DOCKED)
	return print_msg ("We are already docked.\n");
    if (!adjacent_to_starbase())
	return print_msg ("We are not adjacent to a starbase.\n");
    if (Ship.cloaked)
	return print_msg ("We can not dock while cloaked.\n");
    print_msg ("Docking procedure successful.\n");

    // Reset ship's defenses
    Ship.cond = DOCKED;
    Ship.shldup = false;
    Ship.reserves = Param.reserves;

    // Restore resources
    Ship.energy = Param.energy;
    Ship.torped = Param.torped;
    Ship.shield = Param.shield;
    Ship.crew = Param.crew;

    // Offload captured pirates
    unsigned ncaptives = Param.brigfree - Ship.brigfree;
    if (ncaptives) {
	print_msg ("%u captive pirates offloaded to starbase for trial\n", ncaptives);
	Game.captives += ncaptives;
	Ship.brigfree += ncaptives;
    }

    // If the radio was broken, starbase can update
    output_hidden_distress_calls();

    // Reschedule any device repairs
    for (unsigned i = 0; i < MAXEVENTS; ++i)
	if (Event[i].evcode == E_FIXDV)
	    reschedule (&Event[i], (Event[i].date - Now.date) * Param.dockfac);
}

// LEAVE A STARBASE
void undock (void)
{
    if (Ship.cond != DOCKED)
	return print_msg ("We are not docked.\n");
    print_msg ("Undocking complete.\n");
    Ship.cond = GREEN;
    Move.free = 0;

    // Reschedule device repair times to take longer
    for (unsigned i = 0; i < MAXEVENTS; ++i)
	if (Event[i].evcode == E_FIXDV)
	    reschedule (&Event[i], (Event[i].date - Now.date) / Param.dockfac);
}

// REST FOR REPAIRS
//
// You sit around and wait for repairs to happen. Actually, you
// sit around and wait for anything to happen. I do want to point
// out however, that pirates are not as patient as you are, and
// they tend to attack you while you are resting.
//
// You can never rest through a long range tractor beam.
//
// In events() you will be given an opportunity to cancel the
// rest period if anything momentous happens.
//
void rest (void)
{
    // get the time to rest
    float t = getfltpar ("How long");
    if (t <= 0.0)
	return;
    unsigned percent = 100 * t / Now.time + 0.5;
    if (percent >= 70) {
	print_msg ("That would take %u%% of our remaining time.\n", percent);
	if (!getynpar ("Are you sure"))
	    return;
    }
    Move.time = t;

    // boundary condition is the LRTB
    const struct event* lrtb = next_event_of_type (E_LRTB);
    if (lrtb && lrtb->date >= Now.date) {
	t = lrtb->date - Now.date;
	if (Ship.cond != DOCKED && Move.time > t)
	    Move.time = t + 0.0001;
    }
    Move.free = 0;
    Move.resting = 1;
}

//}}}-------------------------------------------------------------------
//{{{ Plasers

// 128-factor effectiveness at dist sectors
unsigned plaser_effectiveness (unsigned dist)
{
    static const uint8_t c_eff[] = // 128*0.9^d
	{ 128, 115, 104, 93, 84, 76, 68, 61, 55, 50, 45, 40, 36, 32, 29, 26 };
    return c_eff [min_u (dist, ArraySize(c_eff)-1)];
}

void plasers (void)
{
    if (Ship.cond == DOCKED)
	return print_msg ("Plasers cannot fire through starbase shields\n");
    if (Ship.cloaked)
	return print_msg ("Plasers are inoperative while cloaked\n");
    unsigned max_targets = min_u (NBANKS, current_quad()->pirates);
    bool havesrs = !device_damaged (SRSCAN);
    if (!havesrs) // With sensors out, limit targets to visual range
	for (unsigned i = 0; i < max_targets; ++i)
	    if (VISCAN_RANGE < sector_distance (Ship.sect, Etc.pirate[i].sect))
		max_targets = i;
    if (!max_targets)
	return print_msg ("There are no pirates in this quadrant\n");
    if (device_damaged (PLASER))
	return announce_device_damage (PLASER);
    if (Ship.energy < Param.energylow + max_targets)
	return print_msg ("Energy levels are too low to charge plaser banks\n");
    uint16_t energy = Ship.energy - Param.energylow;

    // Get all targets first
    struct { uint16_t units; } bank [NBANKS] = {};
    unsigned ntargets = 0;
    for (; ntargets < max_targets; ++ntargets) {
	const struct Pirate* p = &Etc.pirate[ntargets];
	unsigned dist = sector_distance (Ship.sect, p->sect);
	uint16_t eff = plaser_effectiveness (dist)*100/128;
	char prompt [32];
	snprintf (ArrayBlock(prompt), "[%hu,%hu%%] %u->" SECT_FMT, energy, eff, ntargets, p->sect.x, p->sect.y);
	unsigned units = getintpar (prompt);
	if (!units || units > energy)
	    break;
	energy -= units;
	bank[ntargets].units = units;
    }

    // Fire all banks
    unsigned wasted_energy = 0;
    for (unsigned i = 0; i < ntargets; ++i) {
	print_msg ("Plaser bank %u firing at " SECT_FMT " ", i, Etc.pirate[i].sect.x, Etc.pirate[i].sect.y);
	Ship.energy -= bank[i].units;
	unsigned hit = 0;
	for (struct line_iterator li = make_line_iterator (Ship.sect, Etc.pirate[i].sect);;) {
	    advance_line_iterator (&li);
	    if (!hit)
		print_msg (".");
	    if (li.p.x >= NSECTS || li.p.y >= NSECTS) {
		wasted_energy += bank[i].units;
		if (!hit)
		    print_msg (" missed\n");
		break;
	    }
	    enum SectorContents sc = sector_contents (li.p.x, li.p.y);
	    if (sc == EMPTY || !nrand(64))
		continue;
	    if (!hit)
		print_msg (" hit\n");
	    animate_plaser (Ship.sect, li.p);
	    unsigned dist = sector_distance (Ship.sect, li.p);
	    unsigned eff = plaser_effectiveness (dist);
	    hit = bank[i].units * (eff - nrand(eff/8)) / 128;
	    if (sc == PIRATE) {
		struct Pirate* p = pirate_at (li.p.x, li.p.y);
		if (!p) {
		    print_msg ("Uh... Sorry captain, I thought it hit something.\n");
		    break;
		}
		if (p->power < hit)
		    hit = p->power;
		print_msg ("%u unit hit on pirate", hit);
		if (!device_damaged (SRSCAN))
		    print_msg (" at " SECT_FMT, p->sect.x, p->sect.y);
		print_msg ("\n");
		p->power -= hit;
		if (!p->power)
		    kill_pirate (p->sect.x, p->sect.y);
		bank[i].units -= hit;
		if (bank[i].units)
		    continue;	// remaining energy continues throught the target
	    } else if (sc == BASE) {
		if (hit >= 350) {
		    kill_starbase (Ship.quad.x, Ship.quad.y);
		    ++Game.bases_killed;
		    bank[i].units -= hit;
		    if (bank[i].units)
			continue;
		} else
		    print_msg ("Plaser fire absorbed by starbase shields\n");
	    } else if (sc == STAR)
		print_msg ("Plaser fire absorbed by star\n");
	    else if (sc == HOLE)
		print_msg ("Plaser fire absorbed by black hole\n");
	    else if (sc == INHABIT)
		print_msg ("Plaser fire absorbed by planetary atmosphere\n");
	    else
		print_msg ("Unknown object '%c' at " SECT_FMT " destroyed\n", sc, li.p.x, li.p.y);
	    break;
	}
    }
    if (wasted_energy)
	print_msg ("%u units wasted on empty space\n", wasted_energy);
    Move.free = false;
}

//}}}-------------------------------------------------------------------
//{{{ Torpedoes

// TORPEDO CONTROL
//
// Either one or three torpedoes are fired. If three are fired, it is called
// a "burst" and you also specify a spread angle.
//
// Torpedoes are never 100% accurate. There is always a random cludge factor
// in their course which is increased if you have your shields up. Hence,
// you will find that they are more accurate at close range. However, they
// have the advantage that at long range they don't lose any of their power
// as plasers do.
//
// When the course spreads too much, you get a misfire, and the course is
// randomized even more. You also have the chance that the misfire damages
// your torpedo tubes.

void torped (void)
{
    if (Ship.cloaked)
	return print_msg ("Regulations do not permit attack while cloaked.\n");
    if (!device_works (TORPED))
	return;
    if (!Ship.torped)
	return print_msg ("All torpedoes expended\n");
    unsigned max_targets = min_u (min_u (Ship.torped, current_quad()->pirates), NTORPEDOES);
    if (!max_targets)
	return print_msg ("There's nobody here to torpedo, captain\n");

    // get the targets
    struct xy torpt [NTORPEDOES] = {{UINT8_MAX,UINT8_MAX}};
    unsigned ntofire = 0;
    for (; ntofire < max_targets; ++ntofire) {
	char prompt [16];
	snprintf (ArrayBlock(prompt), "Torpedo %u to", ntofire+1);
	torpt[ntofire] = getdest (prompt);
	if (torpt[ntofire].x >= NQUADS*NSECTS || torpt[ntofire].y >= NQUADS*NSECTS)
	    break;	// cancelled this tube
	torpt[ntofire].x %= NSECTS;	// use only sector address here
	torpt[ntofire].y %= NSECTS;
    }
    for (unsigned i = 0; i < ntofire; ++i) {
	--Ship.torped;
	for (struct line_iterator li = make_line_iterator (Ship.sect, torpt[i]);;) {
	    advance_line_iterator (&li);
	    if (li.p.x >= NSECTS || li.p.y >= NSECTS)
		break;
	    enum SectorContents sc = sector_contents (li.p.x, li.p.y);
	    if (sc == EMPTY || !nrand(64))
		continue;
	    animate_torpedo (Ship.sect, li.p);
	    if (sc == PIRATE) {
		struct Pirate* p = pirate_at (li.p.x, li.p.y);
		if (p) {
		    p->power -= min_u (p->power, 500 + nrand(501));
		    if (!p->power)
			kill_pirate (p->sect.x, p->sect.y);
		    else
			print_msg ("*** Hit on pirate at " SECT_FMT ": extensive damages\n", p->sect.x, p->sect.y);
		}
	    } else if (sc == STAR)
		nova (li.p.x, li.p.y);
	    else if (sc == HOLE)
		print_msg ("Torpedo disappears into a black hole\n");
	    else if (sc == INHABIT)
		kill_starsystem (li.p.x, li.p.y, -1);
	    else if (sc == BASE) {
		kill_starbase (Ship.quad.x, Ship.quad.y);
		++Game.bases_killed;
	    } else
		print_msg ("Unknown object '%c' at " SECT_FMT " destroyed\n", sc, li.p.x, li.p.y);
	    break;
	}
	if (device_damaged (TORPED) || current_quad()->stars == SUPERNOVA)
	    break;
    }
    Move.free = false;
}

//}}}-------------------------------------------------------------------
//{{{ Shields and cloaking

// SHIELD CONTROL
//
// Toggles shield up/down.
//
// This is not a free move. Hits that occur as a result of
// this move appear as though the shields are half up/down,
// so you get partial hits.
//
void shield (void)
{
    if (!Ship.shldup) {
	if (Ship.cond == DOCKED)
	    return print_msg ("Shields can not be raised while docked\n");
	if (device_damaged (SHIELD))
	    return announce_device_damage (SHIELD);
    }
    Ship.shldup = !Ship.shldup;
    Move.free = false;
    if (Ship.shldup) {
	Ship.energy -= Param.shupengy;
	Move.shldchg = 1;
    }
}

// CLOAKING DEVICE CONTROL
//
// Toggles cloaking device.
// This is not a free move. Cloak takes time to take effect.
//
void cloak (void)
{
    if (!Ship.cloaked) {
	if (Ship.cond == DOCKED)
	    return print_msg ("Cloaking is not permitted in dock\n");
	if (device_damaged (CLOAK))
	    return announce_device_damage (CLOAK);
    }
    Ship.cloaked = !Ship.cloaked;
    Move.free = false;
    if (Ship.cloaked)
	Ship.cloakgood = false;
}

//}}}-------------------------------------------------------------------
//{{{ Emergency operations

// call starbase for help
//
// First, the closest starbase is selected. If there is a
// a starbase in your own quadrant, you are in good shape.
// This distance takes quadrant distances into account only.
//
// A magic number is computed based on the distance which acts
// as the probability that you will be rematerialized. You
// get three tries.
//
// When it is determined that you should be able to be remater-
// ialized (i.e., when the probability thing mentioned above
// comes up positive), you are put into that quadrant (anywhere).
// Then, we try to see if there is a spot adjacent to the star-
// base. If not, you can't be rematerialized!!! Otherwise,
// it drops you there. It only tries five times to find a spot
// to drop you. After that, it's your problem.

void help (void)
{
    // Check to see if calling for help is reasonable ...
    if (Ship.cond == DOCKED)
	return print_msg ("But Captain, we're already docked\n");
    // ... or possible
    if (device_damaged (SSRADIO))
	return announce_device_damage (SSRADIO);
    draw_and_sleep (1);
    if (Now.bases <= 0)
	return print_msg ("Captain, there is no response to our distress call.\n");

    // Find the closest destination base
    unsigned dist = UINT_MAX, l = 0;
    for (unsigned i = 0; i < Now.bases; ++i) {
	unsigned d = sector_distance (Ship.quad, Now.base[i]);
	if (d < dist) {
	    dist = d;
	    l = i;
	}
    }

    // Go to that quadrant
    print_msg ("Starbase in " QUAD_FMT " responds\n", Ship.quad.x, Ship.quad.y);
    Ship.quad = Now.base[l];
    initquad (true);
    ++Game.helps;

    // Find an empty spot around the starbase
    struct xy dest = { NSECTS, NSECTS };
    for (int dx = -1; dx <= 1; ++dx) {
	for (int dy = -1; dy <= 1; ++dy) {
	    struct xy spot = Etc.starbase;
	    if (spot.x + dx > 0 && (spot.x += dx) < NSECTS
		&& spot.y + dy > 0 && (spot.y += dy) < NSECTS
		&& sector_contents (spot.x, spot.y) == EMPTY)
		dest = spot;
	}
    }
    draw_and_sleep(1);
    if (dest.x >= NSECTS || dest.y >= NSECTS) {
	print_msg ("Unfortunately the long range transporter malfunctions.\n");
	return lose (L_NOHELP);
    }
    print_msg ("Long range transport successful.\n");
    Ship.sect = dest;
    dock();
    sort_pirates();
}

// Self Destruct Sequence
//
// The computer starts up the self destruct sequence. Obviously,
// if the computer is out nothing can happen. You get a countdown
// and a request for password (which, of course, is "password" :)
//
// You get to destroy things when you blow up; hence, it is
// possible to win the game by destructing if you take the last
// pirate with you.
//
void destruct (void)
{
    if (device_damaged (COMPUTER))
	return announce_device_damage (COMPUTER);
    print_msg ("\n--- Self-destruct sequence activated ---\n");
    draw_and_sleep(1);
    // output the count 10 9 8 7 6
    for (unsigned i = 10; i > 5; --i) {
	for (unsigned j = 10; j > i; --j)
	    print_msg ("   ");
	print_msg ("%u\n", i);
	draw_and_sleep(1);
    }
    // check for password on new line only
    char checkpass[16];
    getstrpar ("Password", ArrayBlock(checkpass)-1);
    draw_and_sleep(1);
    if (strcmp (checkpass, "password") != 0)
	return print_msg ("Incorrect. Self-destruct sequence aborted.\n");
    print_msg ("Password verified; self-destruct sequence continues:\n");
    draw_and_sleep(1);
    // output count 5 4 3 2 1 0
    for (unsigned i = 6; i-- > 0;) {
	for (unsigned j = 5; j > i; --j)
	    print_msg ("   ");
	print_msg ("%u\n", i);
	draw_and_sleep(1);
    }
    animate_nova (Ship.sect);
    print_msg ("***** Your ship is destroyed *****\n");
    draw_and_sleep(1);
    Game.killed = true;
    // let's see what we can blow up!!!!
    float zap = 20.0 * Ship.energy;
    Game.deaths += Ship.crew;
    for (int i = 0; i < current_quad()->pirates;) {
	if (Etc.pirate[i].power * sector_distance (Ship.sect, Etc.pirate[i].sect) <= zap)
	    kill_pirate (Etc.pirate[i].sect.x, Etc.pirate[i].sect.y);
	else
	    ++i;
    }
    // Lose if we didn't kill the last pirate (detected by kill_pirate)
    lose (L_DSTRCT);
}

//}}}-------------------------------------------------------------------
