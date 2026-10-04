// Copyright (c) 1987 by Ed James <edjames@berkeley.edu>
// This file is free software, distributed under the BSD license.

#include "atc.h"

//----------------------------------------------------------------------

static unsigned next_plane_index (void);
static bool too_close(const struct Plane *p1, const struct Plane *p2, unsigned dist);
static int dir_deg(int d);

//----------------------------------------------------------------------

void update_planes (void)
{
    ++_clck;

    // Planes take off and move from _ground to the _air
    for (size_t i = 0; i < _ground.size; ++i) {
	if (_ground.d[i].new_altitude > 0) {
	    vector_push_back (&_air, &_ground.d[i]);
	    vector_erase (&_ground, i--);
	}
    }

    // do altitude change and basic movement
    vector_foreach (struct Plane, pp, _air) {
	// type 0 only move every other turn
	if (pp->plane_type == 0 && _clck & 1)
	    continue;

	--pp->fuel;
	if (!pp->fuel)
	    loser (pp, "ran out of fuel.");

	pp->altitude += sign (pp->new_altitude - pp->altitude);

	if (!pp->delayd) {
	    int dir_diff = pp->new_dir - pp->dir;
	    // Allow for circle commands
	    if (pp->new_dir >= 0 && pp->new_dir < MAXDIR) {
		if (dir_diff > MAXDIR/2)
		    dir_diff -= MAXDIR;
		else if (dir_diff < -(MAXDIR/2))
		    dir_diff += MAXDIR;
	    }
	    if (dir_diff > 2)
		dir_diff = 2;
	    else if (dir_diff < -2)
		dir_diff = -2;
	    pp->dir += dir_diff;
	    if (pp->dir >= MAXDIR)
		pp->dir -= MAXDIR;
	    else if (pp->dir < 0)
		pp->dir += MAXDIR;
	}

	pp->xpos += _displacement[pp->dir].dx;
	pp->ypos += _displacement[pp->dir].dy;

	if (pp->delayd && pp->xpos == _sp->beacon[pp->delayd_no].x && pp->ypos == _sp->beacon[pp->delayd_no].y) {
	    pp->delayd = false;
	    if (pp->status == S_UNMARKED)
		pp->status = S_MARKED;
	}

	switch (pp->dest_type) {
	    case T_AIRPORT:
		if (pp->xpos == _sp->airport[pp->dest_no].x && pp->ypos == _sp->airport[pp->dest_no].y && !pp->altitude) {
		    if (pp->dir != _sp->airport[pp->dest_no].dir)
			loser (pp, "landed in the wrong direction.");
		    else {
			pp->status = S_GONE;
			continue;
		    }
		}
		break;
	    default:
		if (pp->xpos == _sp->exit[pp->dest_no].x && pp->ypos == _sp->exit[pp->dest_no].y) {
		    if (pp->altitude != 9)
			loser (pp, "exited at the wrong altitude (%u != 9).", pp->altitude);
		    else {
			pp->status = S_GONE;
			continue;
		    }
		}
		break;
	}
	if (pp->altitude > 9) // "this is impossible"
	    loser (pp, "exceded flight ceiling (9).");
	if (pp->altitude <= 0) {
	    for (unsigned i = 0; i < _sp->num_airports; ++i) {
		if (pp->xpos == _sp->airport[i].x && pp->ypos == _sp->airport[i].y) {
		    if (pp->dest_type == T_AIRPORT)
			loser (pp, "landed at the wrong airport (%u != %u).", i, pp->dest_no);
		    else
			loser (pp, "landed instead of exited.");
		}
	    }
	    loser (pp, "crashed.");
	}
	if (pp->xpos < 1 || pp->xpos >= _sp->width-1u || pp->ypos < 1 || pp->ypos >= _sp->height-1u) {
	    for (unsigned i = 0; i < _sp->num_exits; ++i) {
		if (pp->xpos == _sp->exit[i].x && pp->ypos == _sp->exit[i].y) {
		    if (pp->dest_type == T_EXIT)
			loser (pp, "exited via the wrong exit (%u != %u).", i, pp->dest_no);
		    else
			loser (pp, "exited instead of landed.");
		}
	    }
	    loser (pp, "illegally left your airspace.");
	}
    }

    // Traverse the list once, deleting the planes that are gone.
    for (size_t i = 0; i < _air.size; ++i) {
	if (_air.d[i].status == S_GONE) {
	    ++_safe_planes;
	    vector_erase (&_air, i--);
	}
    }

    draw_all();

    vector_foreach (const struct Plane, p1, _air)
	for (const struct Plane* p2 = p1+1; p2 < p1end; ++p2)
	    if (too_close (p1, p2, 1))
		loser (p1, "collided with plane %c.", plane_name(p2));

    // Check every other update. Actually, only add on even updates.
    // Otherwise, prop jobs show up *on* entrance. Remember that
    // we don't update props on odd updates.
    if (!nrand (_sp->newplane_time))
	addplane();
}

const char* command (const struct Plane* pp)
{
    static char buf[50] = "";
    struct StringBuilder sb = StringBuilder_new (buf);
    StringBuilder_appendf (&sb, "%c%d%c%c%d: ", plane_name(pp), pp->altitude, (pp->fuel < LOWFUEL) ? '*' : ' ', (pp->dest_type == T_AIRPORT) ? 'A' : 'E', pp->dest_no);

    const char* comm_start = StringBuilder_ptr (&sb);
    if (!pp->altitude)
	StringBuilder_appendf (&sb, "Holding @ A%d", pp->orig_no);
    else if (pp->new_dir >= MAXDIR)
	StringBuilder_append (&sb, "Circle");
    else if (pp->new_dir != pp->dir)
	StringBuilder_appendf (&sb, "%d", dir_deg(pp->new_dir));
    if (pp->delayd)
	StringBuilder_appendf (&sb, " @ B%u", pp->delayd_no);
    if (*comm_start == '\0' && (pp->status == S_UNMARKED || pp->status == S_IGNORED))
	StringBuilder_append (&sb, "---------");
    return buf;
}

char plane_name (const struct Plane * p)
{
    return (p->plane_type == 0 ? 'A' : 'a') + p->plane_no;
}

static unsigned next_plane_index (void)
{
    uint8_t havel [MAXPLANES] = {0};
    vector_foreach (const struct Plane, p, _air)
	++havel[p->plane_no];
    vector_foreach (const struct Plane, p, _ground)
	++havel[p->plane_no];
    unsigned lowi = 0;
    while (lowi < ArraySize(havel) && havel[lowi])
	++lowi;
    return lowi;
}

int addplane(void)
{
    struct Plane p;
    memset(&p, 0, sizeof(p));
    p.plane_no = next_plane_index();
    if (p.plane_no >= MAXPLANES)
	return -1;
    p.status = S_MARKED;
    p.plane_type = nrand(2);
    p.fuel = _sp->width + _sp->height;

    unsigned num_starts = _sp->num_exits + _sp->num_airports;
    unsigned planeto = nrand (num_starts);
    if (planeto < _sp->num_exits) {
	p.dest_type = T_EXIT;
	p.dest_no = planeto;
    } else {
	p.dest_type = T_AIRPORT;
	p.dest_no = planeto - _sp->num_exits;
    }

    unsigned planefrom = planeto;
    while (planefrom == planeto) {
	planefrom = nrand (num_starts);
	if (planefrom < _sp->num_exits) {
	    p.orig_type = T_EXIT;
	    p.orig_no = planefrom;
	    p.xpos = _sp->exit[planefrom].x;
	    p.ypos = _sp->exit[planefrom].y;
	    p.new_dir = p.dir = _sp->exit[planefrom].dir;
	    p.altitude = p.new_altitude = 7;
	    vector_foreach (const struct Plane, pother, _air)
		if (too_close (pother, &p, 4))
		    planefrom = planeto;
	} else {
	    p.orig_type = T_AIRPORT;
	    p.orig_no = planefrom - _sp->num_exits;
	    p.xpos = _sp->airport[p.orig_no].x;
	    p.ypos = _sp->airport[p.orig_no].y;
	    p.new_dir = p.dir = _sp->airport[p.orig_no].dir;
	    p.altitude = p.new_altitude = 0;
	}
    }
    vector_push_back (p.orig_type == T_AIRPORT ? &_ground : &_air, &p);
    return p.dest_type;
}

struct Plane* findplane (unsigned n)
{
    vector_foreach (struct Plane, p, _air)
	if (p->plane_no == n)
	    return p;
    vector_foreach (struct Plane, p, _ground)
	if (p->plane_no == n)
	    return p;
    return NULL;
}

static bool too_close(const struct Plane * p1, const struct Plane * p2, unsigned dist)
{
    return absv(p1->altitude - p2->altitude) <= dist && absv(p1->xpos - p2->xpos) <= dist && absv(p1->ypos - p2->ypos) <= dist;
}

static int dir_deg(int d)
{
    return (d >= 0 && d < 8) ? d * 45 : -1;
}
