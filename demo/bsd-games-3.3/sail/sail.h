// Copyright (c) 1983 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "config.h"

// load.l and load.r, should match loadname[]
enum {
    L_EMPTY,
    L_GRAPE,
    L_CHAIN,
    L_ROUND,
    L_DOUBLE,
    L_EXPLODE
};

// ready.l and ready.r, these are bits, except R_EMPTY
enum {
    R_EMPTY,	// not loaded and not loading
    R_LOADING	= 1<<0,	// loading
    R_DOUBLE	= 1<<1,	// loading double
    R_LOADED	= 1<<2,	// loaded
    R_INITIAL	= 1<<3	// loaded initial
};

enum {
    HULL,
    RIGGING
};

struct BP {
    uint16_t	turnsent;
    uint8_t	mensent;
    uint8_t	toship;
};

struct snag {
    uint8_t sn_count;
    uint8_t sn_turn;
};

struct Sides {
    uint8_t l, r;
};

enum { NSCENE = 31 };

enum {
    NSHIP = 10,
    NBP = 3
};

enum {
    N_American,
    N_British,
    N_Spanish,
    N_French,
    N_Japanese,
    NNATION
};
enum {
    dir_None,
    dir_N,
    dir_NE,
    dir_E,
    dir_SE,
    dir_S,
    dir_SW,
    dir_W,
    dir_NW
};
enum {
    ship_Driftwood,
    ship_Line,
    ship_Line2,
    ship_Frigate,
    ship_Corvette,
    ship_Brig
};
enum {
    qual_Dead,
    qual_Mutinous,
    qual_Green,
    qual_Mundane,
    qual_Crack,
    qual_Elite
};

struct ShipSpecs {
    uint8_t	sclass;
    uint8_t	qual;	// crew quality
    uint8_t	hull;	// health
    uint8_t	bs;	// base speed
    uint8_t	fs;	// full speed
    uint8_t	ta;	// turning ability
    uint8_t	guns;
    struct Sides gun;
    struct Sides car;	// carronades
    uint8_t	crew[3];
    int8_t	rig[4];
};

struct ShipStatus {
    short	col;
    short	row;
    uint8_t	index;
    uint8_t	drift;
    uint8_t	nfoul;
    uint8_t	ngrap;
    char	RH;
    char	RG;
    char	RR;
    uint8_t	FS;
    bool	struck;
    uint8_t	explode;
    uint8_t	sink;
    uint8_t	dir;
    uint8_t	loadwith;
    uint8_t	stern;
    uint8_t	pcrew;
    bool	captured;
    uint8_t	pcrewqual;
    uint8_t	capnation;
    char	movebuf[6+8];
    struct snag	foul [NSHIP];
    struct snag	grap [NSHIP];
    struct Sides load;
    struct Sides ready;
    struct BP	OBP [NBP];
    struct BP	DBP [NBP];
};

struct Ship {
    const char*		shipname;
    uint8_t		nationality;
    uint8_t		shiprow;
    uint8_t		shipcol;
    uint8_t		shipdir;
    struct ShipSpecs	specs;
    struct ShipStatus	status;
};
DECLARE_VECTOR_TYPE (ShipList, struct Ship);

struct ScenarioShip {
    const char		shipname[16];
    uint8_t		nationality;
    uint8_t		shiprow;
    uint8_t		shipcol;
    uint8_t		shipdir;
    struct ShipSpecs	specs;
};

struct Scenario {
    const char*		name;
    uint8_t		winddir;
    uint8_t		windspeed;
    uint8_t		windchange;
    uint8_t		vessels;
    const struct ScenarioShip* ship;
};
extern struct Scenario _scenarios [NSCENE];

struct ScenarioSelector {
    uint8_t	scenario;
    uint8_t	ship;
    struct Sides iniload;
};

struct windeffects {
    uint8_t A:4;
    uint8_t B:4;
    uint8_t C:4;
    uint8_t D:4;
};
extern const struct windeffects c_WindEffectTable[7][6];

struct Tables {
    uint8_t H:4;
    uint8_t G:4;
    uint8_t C:4;
    uint8_t R:4;
};
extern const struct Tables c_RigTable[11][6];
extern const struct Tables c_HullTable[11][6];

extern const char c_AMMO[9][4];
extern const char c_HDT[9][10];
extern const char c_HDTrake[9][10];
extern const char c_QUAL[9][5];
extern const char c_MT[9][3];

extern const char c_CountryName [NNATION][9];
extern const char* const c_ClassName [7];
extern const char* const c_DirectionName [9];
extern const char c_QualityName [6][9];
extern const char c_LoadLetter[];

extern const char c_ShotRange[];

extern const char c_dr[], c_dc[];

extern const struct Scenario* _cc;	// the current scenario
extern struct ShipList _ships;	// active ships

extern int _winddir;
extern int _windspeed;
extern int _turn;

extern int _loaded, _fired, _changed, _repaired;
extern int _dont_adjust;
extern int _player;
extern struct Ship* _ms;	// memorial structure, &cc->ship[player]

// condition codes for leave()
enum {
    LEAVE_QUIT,
    LEAVE_CAPTURED,
    LEAVE_HURRICAN
};

// ai.c
int dr_turn (void);

// player.c
int pl_main(void);
_Noreturn void play(void);

// display.c
void initscreen(void);
_Noreturn void leave(int);
void Signal (const char* fmt, const struct Ship* s, ...) PRINTFLIKE(1,3);
void makesignal (const struct Ship *from, const char *fmt, const struct Ship *ship, ...);
void makemsg (const struct Ship *from, const char *fmt, ...) PRINTFLIKE(2,3);
void Msg (const char*, ...) PRINTFLIKE(1,2);
int sgetch(const char *, struct Ship *, int);
void sgetstr(const char *, char *, int);
void draw_screen(void);
void centerview(void);
void upview(void);
void downview(void);
void leftview(void);
void rightview(void);
void parties(struct Ship *to, int *crew, int isdefense, int buf);
struct ScenarioSelector select_scenario (void);


// ship.c
void table (struct Ship* from, struct Ship* on, int rig, int shot, int hittable, int roll);
void Cleansnag (struct Ship* from, struct Ship* to, int all, int flag);
unsigned maxturns (const struct Ship* ship, char *af);
int maxmove (const struct Ship* ship, int dir, int fs);
bool meleeing (const struct Ship* from, const struct Ship* to);
bool boarding (const struct Ship* from, bool isdefense);
void unboard (struct Ship* ship, const struct Ship* to, bool isdefense);
int range (const struct Ship* from, const struct Ship* to);
struct Ship* closestenemy (const struct Ship* from, int side, bool anyship);
int gunsbear (const struct Ship* from, const struct Ship* to);
int portside (const struct Ship* from, const struct Ship* on, int quick);
int colours (const struct Ship* sp);
void foul_ship_with (struct Ship *s, unsigned w);
void unfoul_ship_from (struct Ship *s, unsigned fi, bool all);
void ungrapple_ship_from (struct Ship *s, unsigned fi, bool all);
void grapple_ship_with (struct Ship *s, unsigned w);
void ship_set_defensive_parties (struct Ship *s, unsigned side, unsigned turnsent, unsigned toship, unsigned mensent);
void ship_set_offensive_parties (struct Ship *s, unsigned side, unsigned turnsent, unsigned toship, unsigned mensent);

// Inline accessors

static inline unsigned dieroll (void) { return nrand(6)+1; }

static inline uint8_t grappled (const struct Ship* a) { return a->status.ngrap; }
static inline uint8_t fouled (const struct Ship* a) { return a->status.nfoul; }
static inline uint8_t snagged (const struct Ship* a) { return grappled(a) + fouled(a); }

static inline uint8_t grappled2 (const struct Ship* a, const struct Ship* b)
    { return a->status.grap[b->status.index].sn_count; }
static inline uint8_t fouled2 (const struct Ship* a, const struct Ship* b)
    { return a->status.foul[b->status.index].sn_count; }
static inline uint8_t snagged2 (const struct Ship* a, const struct Ship* b)
    { return grappled2(a, b) + fouled2(a, b); }

static inline int Xgrappled2 (const struct Ship* a, const struct Ship* b)
    { return a->status.grap[b->status.index].sn_turn < _turn-1 ? grappled2(a, b) : 0; }
static inline int Xfouled2 (const struct Ship* a, const struct Ship* b)
    { return a->status.foul[b->status.index].sn_turn < _turn-1 ? fouled2(a, b) : 0; }
static inline int Xsnagged2 (const struct Ship* a, const struct Ship* b)
    { return Xgrappled2(a, b) + Xfouled2(a, b); }

static inline void cleangrapple (struct Ship* a, struct Ship* b, int c)
    { return Cleansnag (a, b, c, 1); }
static inline void cleanfoul (struct Ship* a, struct Ship* b, int c)
    { return Cleansnag (a, b, c, 2); }
static inline void cleansnag (struct Ship* a, struct Ship* b, int c)
    { return Cleansnag (a, b, c, 3); }

static inline char sterncolour (const struct Ship* sp)
    { return sp->status.stern+'0'-(sp->status.captured?10:0); }
static inline unsigned sternrow (const struct Ship* sp)
    { return sp->status.row + c_dr[sp->status.dir]; }
static inline unsigned sterncol (const struct Ship* sp)
    { return sp->status.col + c_dc[sp->status.dir]; }

static inline uint8_t shipnationality (const struct Ship* sp)
    { return sp->status.captured ? sp->status.capnation : sp->nationality; }
static inline uint8_t shipcrewqual (const struct Ship* sp)
    { return sp->status.captured ? sp->status.pcrewqual : sp->specs.qual; }

static inline char readyname (uint8_t r)
    { return r & R_LOADING ? '*' : (r & R_INITIAL ? '!' : ' '); }

static inline struct Ship* SHIP (unsigned s)
    { return &_ships.d[s]; }

#define foreachship(i)	vector_foreach (struct Ship, i, _ships)
