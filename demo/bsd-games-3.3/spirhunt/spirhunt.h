// Copyright (c) 1980 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#pragma once
#include "config.h"
#include <float.h>

//{{{ Galactic parameters ----------------------------------------------

enum {
    NQUADS	= 8,		// dimension of galaxy in quadrants
    NSECTS	= 10,		// dimensions of quadrant in sectors
    NINHAB	= NQUADS*NQUADS/2, // half the quadrants are inhabited
    MAXBASES	= NQUADS+1,	// maximum number of starbases in galaxy
    QUAD_STARS	= NSECTS-1,	// maximum stars per quadrant
    SUPERNOVA,			// stars value for supernova quadrants
    QUAD_HOLES	= 1,		// maximum black holes per quadrant
    QUAD_PIRATES = NSECTS-1,	// maximum pirates per quadrant
    MAXEVENTS	= NQUADS*3,	// max number of concurrently pending events
    LRSCAN_RANGE= 1,
    VISCAN_RANGE= 2		// visual scan range without sensors
};

struct xy {
    uint8_t x;
    uint8_t y;
};

#define QUAD_FMT	"%d%d"
#define SECT_FMT	"%d%d"
#define FULLCOORD_FMT	QUAD_FMT "." SECT_FMT

struct quad {			// definition for each quadrant
    uint8_t	systemname:5;	// starsystem name index into Systemname
    uint8_t	bases:1;	// number of bases in this quadrant
    uint8_t	holes:1;	// number of black holes in this quadrant
    bool	distressed:1;
    uint8_t	pirates:4;	// number of pirates in this quadrant
    uint8_t	stars:4;	// number of stars in this quadrant
};

// ascii names of systems
extern const char Systemname[NINHAB][16];

// quadrant definition
extern struct quad Quad[NQUADS][NQUADS];

// defines for sector map (below)
enum SectorContents {
    EMPTY	= '.',
    STAR	= '*',
    BASE	= '#',
    YOURSHIP	= 'Y',
    PIRATE	= 'P',
    INHABIT	= '@',
    HOLE	= ' '
};

//}}}-------------------------------------------------------------------
//{{{ Events

enum EventType {
    E_NONE,
    E_LRTB,	// long range tractor beam
    E_PATSB,	// pirate attacks starbase
    E_PDESB,	// pirate destroys starbase
    E_ISSUE,	// distress call is issued
    E_ENSLV,	// pirates enslave a quadrant
    E_REPRO,	// a pirate is reproduced
    E_FIXDV,	// fix a device
    E_ATTACK,	// pirate attack during rest period
    E_SNAP,	// take a snapshot for time warp
    E_SNOVA,	// supernova occurs
    NEVENTS = 12,		// number of different event types
    E_GHOST	= 1<<6,		// ghost of a distress call if ssradio out
    E_HIDDEN	= E_GHOST<<1,	// event that is unreportable because ssradio out
    E_EVENT	= E_GHOST-1	// mask to get event code
};

struct event {
    float	date;		// trap date
    struct xy	quad;		// quadrant coordinates
    uint8_t	evcode;		// enum EventType
    uint8_t	systemname;	// starsystem name
};

// systemname conventions:
//      1 -> NINHAB     index into Systemname table for reported distress calls
//
// evcode conventions:
//      1 -> NEVENTS-1  event type
//      + E_HIDDEN      unreported (SSradio out)
//      + E_GHOST       actually already expired
//      0               unallocated

extern struct event Event[MAXEVENTS];	// dynamic event list; one entry per pending event

//}}}-------------------------------------------------------------------
//{{{ Pirates

struct Pirate {
    struct xy	sect;		// sector coordinates
    uint16_t	power;		// power left
};

//}}}-------------------------------------------------------------------
//{{{ Ship information

// condition codes
enum { GREEN, DOCKED, YELLOW, RED };

enum {
    NBANKS	= 6,	// number of plaser banks
    NTORPEDOES	= 3	// number of torpedo tubes
};

enum ShipDevice {
    WARP,	// warp engines
    SRSCAN,	// short range scanners
    LRSCAN,	// long range scanners
    PLASER,	// plaser control
    TORPED,	// torpedo control
    IMPULSE,	// impulse engines
    SHIELD,	// shield control
    COMPUTER,	// on board computer
    SSRADIO,	// subspace radio
    LIFESUP,	// life support systems
    CLOAK,	// cloaking device
    NDEV	// max number of devices
};
extern const char DeviceName [NDEV][16];

// information regarding the state of the starship
struct Ship_struct {
    struct xy	quad;		// quadrant coord
    struct xy	sect;		// sector coord
    uint16_t	energy;		// starship's energy
    uint16_t	shield;		// energy in shields
    uint16_t	crew;		// ship's complement
    uint16_t	brigfree;	// space left in brig
    float	reserves;	// life support reserves
    uint8_t	warp;		// warp factor
    uint8_t	torped;		// torpedoes
    uint8_t	cond;		// condition code
    uint8_t	distressed;	// number of distress calls
    bool	cloaked;	// set if cloaking device on
    bool	cloakgood;	// set if we have moved
    bool	shldup;		// shield up flag
};
extern struct Ship_struct Ship;

//}}}-------------------------------------------------------------------
//{{{ Game-related information

#define SPIRHUNT_SAVE_NAME	"spirhunt"

#define	SPIRHUNT_SCOREFILE	_PATH_GAME_STATE "spirhunt.scores"
#define SCOREFILE_MAGIC		"spirhu"
enum { MAXSCORES = 10 };

struct Game_struct {
    uint16_t	pirates_killed;	// number of pirates killed
    uint16_t	deaths;		// number of deaths onboard your ship
    uint16_t	stars_killed;	// number of stars killed
    uint16_t	captives;	// total number of captives taken
    uint8_t	bases_killed;	// number of starbases destroyed
    uint8_t	killed;		// set if you were killed
    uint8_t	killinhab;	// number of inhabited starsystems killed
    uint8_t	helps;		// number of help calls
};
extern struct Game_struct Game;

// per move information
struct Move_struct {
    float	time;		// time used this move
    uint8_t	newquad:2;	// set if just entered this quadrant
    bool	free:1;		// set if a move is free
    bool	endgame:1;	// end of game flag
    bool	shldchg:1;	// set if shields changed this move
    bool	resting:1;	// set if this move is a rest
};
extern struct Move_struct Move;

// parametric information
struct Param_struct {
    uint16_t	crew;		// size of ship's complement
    uint16_t	brigfree;	// max possible number of captives
    uint16_t	energy;		// starship's energy
    uint16_t	energylow;	// low energy mark (cond YELLOW)
    uint16_t	shield;		// energy in shields
    uint16_t	shupengy;	// energy to put up shields
    uint16_t	stopengy;	// energy to do emergency stop
    uint16_t	cloakenergy;	// cloaking device energy per day
    uint16_t	pirate_crew;	// number of pirates in a crew
    uint16_t	piratepwr;	// pirate initial power
    uint8_t	bases;		// number of starbases
    uint8_t	pirates;	// number of pirates
    uint8_t	torped;		// torpedoes carried
    uint8_t	warptime;	// time chewer multiplier
    float	date;
    float	time;		// time left
    float	reserves;	// life support reserves
    float	resource;	// galactic resources
    float	dockfac;	// docked repair time factor
    float	hitfac;		// pirate attack factor
    float	plasfac;	// pirate plaser power eater factor
    float	regenfac;	// regeneration factor
    float	eventdly [NEVENTS]; // event time multipliers
};
extern const struct Param_struct Param;

// other information kept in a snapshot
struct Now_struct {
    float	date;
    float	time;		// time left
    float	resource;	// galactic resources
    uint8_t	bases;		// number of starbases
    struct xy	base [MAXBASES];// locations of starbases
};
extern struct Now_struct Now;

// Other stuff, not dumped in a snapshot
struct Etc_struct {
    struct Pirate	pirate [QUAD_PIRATES];	// sorted pirate list
    struct xy		starbase;		// starbase in current quadrant
    struct xy		inhabited;
    struct xy		blackholes [QUAD_HOLES];
    struct xy		stars [QUAD_STARS];
};
extern struct Etc_struct Etc;

struct Snapshot_struct {
    struct Now_struct	now;
    struct quad		quads [NQUADS][NQUADS];
    struct event	events [MAXEVENTS];
};
extern struct Snapshot_struct Snapshot;

// "you lose" codes
enum LoseReason {
    L_STILLPLAYING,
    L_NOTIME,	// ran out of time
    L_NOENGY,	// ran out of energy
    L_DSTRYD,	// destroyed by a pirate
    L_SUICID,	// destroyed in a nova
    L_SNOVA,	// destroyed in a supernova
    L_NOLIFE,	// life support termination
    L_NOHELP,	// you could not be rematerialized
    L_TOOFAST,	// pretty stupid going at warp 10
    L_STAR,	// ran into a star
    L_DSTRCT,	// self destructed
    L_NOCREW	// you ran out of crew
};

// Movement in a straight line as an iterator
struct line_iterator {
    struct xy	p;
    int16_t	d;
    uint8_t	dl;
    uint8_t	ds;
    bool	xmajor;
    int8_t	xs;
    int8_t	ys;
};

// Command lists for getcodpar
typedef void (*cmdfun)(int);
struct cvntab {
    const char*	abrev;
    const char*	full;
    cmdfun	value;
    int		value2;
};

//}}}-------------------------------------------------------------------
//{{{ Prototypes

// events.c
int events (bool in_time_warp);
bool output_hidden_distress_calls (void);
struct event* schedule (enum EventType type, float offset, uint8_t x, uint8_t y, uint8_t sysname);
void unschedule (struct event* e);
struct event* next_event_of_type (enum EventType type);
struct event* xsched (enum EventType ev, unsigned factor, uint8_t x, uint8_t y, uint8_t sysname);
void xresched (struct event* e, enum EventType ev, unsigned factor);
inline static void reschedule (struct event* e, float offset)
    { e->date = Now.date + offset; }

// kill.c
void kill_pirate (int ix, int iy);
void kill_starbase (int qx, int qy);
void kill_starsystem (int x, int y, int f);
void kill_distress_call (unsigned x, unsigned y, bool f);
void nova(int, int);
void snova(int, int);

// move.c
void setwarp (void);
void impulse (void);
void dowarp (int);
void autover (void);
struct line_iterator make_line_iterator (struct xy from, struct xy to);
void advance_line_iterator (struct line_iterator* li);
const char* systemname (const struct quad* q) PURE;
enum SectorContents sector_contents (uint8_t x, uint8_t y) PURE;
struct quad* current_quad (void) PURE;
unsigned sector_distance (struct xy a, struct xy b) constexpr;
unsigned pirates_remaining (void) PURE;
inline static struct xy absolute_sector_coord (struct xy quad, struct xy sect)
    { struct xy ac = { quad.x*NSECTS + sect.x, quad.y*NSECTS + sect.y }; return ac; }

// pirates.c
void sort_pirates (void);
struct Pirate* pirate_at (unsigned x, unsigned y);
void attack (int);
void capture (void);
struct Pirate* select_pirate (void);

// ship.c
bool device_works (enum ShipDevice d);
void damage_report (void);
void damage_device (enum ShipDevice dev, float dam);
bool device_damaged (enum ShipDevice d);
void announce_device_damage (enum ShipDevice dev);
void dock (void);
void undock (void);
void rest (void);
unsigned plaser_effectiveness (unsigned dist);
void plasers (void);
void torped (void);
void shield (void);
void cloak (void);
void help (void);
void destruct (void);

// spirhunt.c
void initquad (bool docked);
struct xy random_empty_sector (void);
void checkcond (void);
bool save (void);
void save_game (void);
void myreset (void);
_Noreturn void win (void);
_Noreturn void lose (enum LoseReason r);
float fnrand (void);

// ui.c
void create_windows (void);
void draw_screen (void);
void animate_plaser (struct xy from, struct xy to);
void animate_torpedo (struct xy from, struct xy to);
void animate_nova (struct xy nl);
void print_msg (const char* msg, ...) PRINTFLIKE(1,2);
int print_score (void);
void main_command (void);
int getintpar (const char *s);
float getfltpar (const char *s);
bool getynpar (const char *s);
const struct cvntab *getcodpar (const char *s, const struct cvntab *tab);
void getstrpar (const char *s, char *r, unsigned l);
struct xy getdest (const char* prompt);
void draw_and_sleep (unsigned seconds);
inline static void delay (unsigned t)
    { usleep (1000000*t/120); }	// Make delay a fraction of 60Hz to minimize stutter

//}}}-------------------------------------------------------------------
