// Copyright (c) 1987 by Ed James <edjames@berkeley.edu>
// This file is free software, distributed under the BSD license.

#pragma once
#include "config.h"

//{{{ Constants --------------------------------------------------------

#define AUTHOR_STR		"ATC - by Ed James"

enum EDir {
    DIR_NORTH,
    DIR_NE,
    DIR_EAST,
    DIR_SE,
    DIR_SOUTH,
    DIR_SW,
    DIR_WEST,
    DIR_NW,
    MAXDIR
};

enum {
    T_NODEST,
    T_BEACON,
    T_EXIT,
    T_AIRPORT
};

enum {
    S_NONE,
    S_GONE,
    S_MARKED,
    S_UNMARKED,
    S_IGNORED
};

enum {
    MAXPLANES	= 20,
    INPUT_LINES	= 3,
    PLANE_COLS	= 20,
    LOWFUEL	= 15
};

//}}}-------------------------------------------------------------------
//{{{ Types

struct ScreenPos {
    uint8_t	x;
    uint8_t	y;
    uint16_t	dir;			// used only sometimes
};

struct Airway {
    struct ScreenPos	p1;
    struct ScreenPos	p2;
};

typedef struct Airway AIRWAY;
typedef struct ScreenPos EXIT;
typedef struct ScreenPos BEACON;
typedef struct ScreenPos AIRPORT;

struct Scenario {
    const char*	name;
    uint8_t	width;
    uint8_t	height;
    uint8_t	update_secs;
    uint8_t	newplane_time;
    uint8_t	num_exits;
    uint8_t	num_lines;
    uint8_t	num_beacons;
    uint8_t	num_airports;
    const EXIT*		exit;
    const AIRWAY*	line;
    const BEACON*	beacon;
    const AIRPORT*	airport;
};

struct Plane {
    uint8_t	status;
    uint8_t	plane_no;
    uint8_t	plane_type;
    uint8_t	orig_no;
    uint8_t	orig_type;
    uint8_t	dest_no;
    uint8_t	dest_type;
    uint8_t	altitude;
    uint8_t	new_altitude;
    uint8_t	dir:4;
    uint8_t	new_dir:4;
    bool	delayd;
    uint8_t	delayd_no;
    uint8_t	xpos;
    uint8_t	ypos;
    uint16_t	fuel;
};

DECLARE_VECTOR_TYPE (PlaneList, struct Plane);

struct Displacement {
    int8_t	dx;
    int8_t	dy;
};

//}}}-------------------------------------------------------------------
//{{{ Externs

extern unsigned _clck, _safe_planes, _start_time;
extern const struct Scenario* _sp;
extern struct PlaneList _air, _ground;
extern const struct Displacement _displacement [MAXDIR];
extern char _cmdtext [64];
extern char _errtext [64];

//}}}-------------------------------------------------------------------
//{{{ Functions

// graphics.c
void init_gr (void);
void draw_all (void);
int getAChar (void);
_Noreturn void loser (const struct Plane* p, const char* s, ...);
_Noreturn void quitter (void);

// input.c
void getcommand(void);

// move.c
void update_planes (void);
const char* command (const struct Plane *pp);
char plane_name (const struct Plane *p);
int addplane (void);
struct Plane* findplane (unsigned n);

// scenarios.c
int load_scenario (const char* s);
const char* default_scenario (void);
const char* okay_scenario (const char* s);
void list_scenarios (void);

// scores.c
void save_score (bool listOnly);

//}}}-------------------------------------------------------------------
