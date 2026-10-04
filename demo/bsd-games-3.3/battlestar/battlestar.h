// This file is free software, distributed under the BSD license.

#include "config.h"
#include <pwd.h>
#include <signal.h>
#include <time.h>

//{{{ Game constants ---------------------------------------------------

#define	BATTLESTAR_SCOREFILE	_PATH_GAME_STATE "battlestar.scores"
#define BATTLESTAR_SAVE_NAME	"battlestar"
#define SCOREFILE_MAGIC		"btlstr"

// well known rooms
enum LocationId {
    MAIN_HANGAR		= 1,
    HANGAR_GALLERY	= 3,
    LAUNCH_ROOM		= 5,
    STARFIGHTER_ROOM	= 7,
    STATEROOM_CLOSET	= 8,
    AMULET_ROOM		= 13,
    LASER_ROOM		= 20,
    LUXURIOUS_STATEROOM	= 22,
    KITCHEN_STAIRWELL	= 25,
    KITCHEN_DOOR	= 30,
    DINING_ROOM_DOOR	= 31,
    OUTSIDE_BATTLESTAR	= 32,
    ORBITING_PLANET	= 68,
    FIRST_ISLAND_LAND	= 89,
    DOCK		= 93,
    ABOVE_SEA_CAVE	= 114,
    POOLS		= 126,
    WIDER_CANYON	= 133,
    SECRET_THICKET	= 144,
    TIDE_POOLS		= 145,
    ALONG_THE_SHORE	= 159,
    SEA_CAVE_ENTRANCE	= 160,
    COAST_ROAD_TURN	= 162,
    BRIDGE_OVER_LAGOON	= 172,
    CAVE_DOOR		= 189,
    GARDEN		= 197,
    IN_THE_LAGOON	= 201,
    ALONG_HIGH_CLIFFS	= 214,
    ROAD_TURNAROUND	= 215,
    BUNGALOW_PORCH	= 217,
    BUNGALOW_BEDROOM	= 218,
    END_OF_THE_ROAD	= 224,
    CAVE_STREAM_BANK	= 229,
    CAVE_ENTRANCE	= 231,
    HUGE_CHASM		= 232,
    CLIFF_WATERFALL	= 242,
    MINE_CHAMBER	= 266,
    GODDESS_THRONE_ROOM	= 268,
    FINAL		= 275,
    NUMOFROOMS		= FINAL,
    // Non-world locations
    INVENTORY,
    WEARING
};

// word types
enum WordType {
    VERB,
    OBJECT,
    NOUNS,
    PREPS,
    ADJS,
    CONJ
};

// words numbers
enum ObjectId {
    KNIFE, SWORD, LAND, WOODSMAN, TWO_HANDED,
    CLEAVER, BROAD, MAIL, HELM, SHIELD,
    MAID, STARFIGHTER, LAMPON, SHOES, DRENIAN,
    PAJAMAS, ROBE, AMULET, MEDALION, TALISMAN,
    DEADWOOD, MALLET, LASER, BATHGOD, GODDESS,
    GRENADE, CHAIN, ROPE, LEVIS, MACE,
    SHOVEL, HALBERD, COMPASS, CRASH, ELF,
    FOOT, COINS, MATCHES, MAN, PAPAYAS,
    PINEAPPLE, KIWI, COCONUTS, MANGO, RING,
    POTION, BRACELET, GIRL, GIRLTALK, DARK_LORD,
    TIMER, TIMER1, TIMER2, CHAR, BOMB,
    NATIVE_GIRL, HORSE, CAR, POT, BAR,
    BLOCK, NUMOFOBJECTS
};

// non-objects below
enum CommandId {
    UP = 1000, DOWN, AHEAD, BACK, RIGHT,
    LEFT, TAKE, USE, LOOK, QUIT,
    NORTH, SOUTH, EAST, WEST, SU,
    DROP, TAKEOFF, DRAW, PUTON, WEARIT,
    PUT, INVEN, EVERYTHING, AND, KILL,
    THROW_OBJECT, LAUNCH, LANDIT, LIGHT, FOLLOW,
    KISS, LOVE, GIVE, SMITE, SHOOT,
    ON, OFF, TIME, SLEEP, DIG,
    EAT, SWIM, DRINK, DOOR, SAVE,
    RIDE, DRIVE, SCORE, BURY, JUMP,
    KICK, OPEN, VERBOSE, BRIEF, AUXVERB
};

// injuries
enum Injury {
    ABRASIONS, LACERATIONS, MINOR_PUNCTURE, MINOR_AMPUTATION, SPRAINED_WRIST,
    BROKEN_ANKLE, BROKEN_ARM, BROKEN_RIBS, BROKEN_LEG, BROKEN_BACK,
    HEAVY_BLEEDING, FRACTURED_SKULL, BROKEN_NECK, NUMOFINJURIES
};

// game_states bits
enum EGameState {
    CANTLAUNCH, LAUNCHED, CANTSEE, CANTMOVE, DUG,
    OPENED_KITCHEN_DOOR, OPENED_CAVE_DOOR, UNCOVERED_SEA_CAVE, ROPE_IN_PIT, IS_NIGHT,
    MET_GIRL, IS_WIZARD, MATCH_LIGHT, IS_VERBOSE, LOVED_GODDESS
};

// Number of times room description shown.
enum { ROOMDESC = 3 };

// Fundamental constants
enum {
    LINELENGTH	= 81,
    CYCLE	= 100,
    // Initial variable values
    TANKFULL	= 250,
    TORPEDOES	= 10,
    MAXWEIGHT	= 60,
    MAXCUMBER	= 10,
    MAXSCORES	= 10
};

enum EDayOrNight { TODAY, TONIGHT };

// These are flags for objects in ObjectInfo. OBJ_PLURAL means
// that the object short name is plural; OBJ_AN that it begins with a
// vowel sound so should be preceded by "an" instead of "a"; OBJ_PERSON
// that it is a living person; OBJ_NONOBJ that it is not an object (to
// which any game action can be applied) at all (e.g. footsteps, asteroids).
// Any individual object has at most one of OBJ_PERSON and OBJ_NONOBJ.
enum {
    OBJ_PLURAL	= 1<<0,
    OBJ_AN	= 1<<1,
    OBJ_PERSON	= 1<<2,
    OBJ_NONOBJ	= 1<<3,
};

//}}}-------------------------------------------------------------------
//{{{ Types

typedef uint32_t gametime_t;
typedef uint16_t location_t;

struct ObjectInfo {
    const char*	shdesc;
    const char*	desc;
    uint16_t	weight;
    uint8_t	cumber;
    uint8_t	flags;
};

struct room {
    const char*	name;
    const char*	day_desc;
    const char*	night_desc;
    struct {
	location_t north;
	location_t east;
	location_t south;
	location_t west;
	location_t up;
	location_t down;
	location_t flyhere;
	location_t access;
    }		dir;
};

// current input line
enum {
    WORDLEN	= 13,
    NWORDS	= 8	       // words per line
};
struct Word {
    char	word [WORDLEN];
    uint8_t	type;
    uint16_t	value;
};

struct GameScore {
    uint16_t	ego;
    uint16_t	pleasure;
    uint16_t	power;
};

//}}}-------------------------------------------------------------------
//{{{ Game data

extern const struct room location [NUMOFROOMS+1];
extern const struct ObjectInfo c_objinfo [NUMOFOBJECTS];
extern const char ouch [NUMOFINJURIES][24];

//}}}-------------------------------------------------------------------
//{{{ Global state variables

extern gametime_t ourtime;
extern gametime_t rythmn;
extern gametime_t ate;
extern gametime_t snooze;
extern location_t position;
extern uint16_t direction;
extern uint16_t game_states;
extern uint16_t player_injuries;
extern struct GameScore game_score;
extern uint8_t fuel;
extern uint8_t torps;
extern uint8_t matchcount;
extern uint8_t godready;
extern uint8_t win;
extern uint8_t beenthere [(NUMOFROOMS+1+7)/8];

//}}}-------------------------------------------------------------------
//{{{ Inlines for state access

// game_states access
inline static bool game_state (enum EGameState f)
    { return game_states & (1u << f); }
inline static void set_game_state (enum EGameState f)
    { game_states |= (1u << f); }
inline static void clear_game_state (enum EGameState f)
    { game_states &= ~(1u << f); }

// injury accessors
inline static bool has_injury (enum Injury i)
    { return (player_injuries >> i) & 1; }
inline static unsigned is_injured (void)
    { unsigned n = 0; for (unsigned i = 0; i < NUMOFINJURIES; ++i) if (has_injury(i)) ++n; return n; }
inline static bool is_fatally_injured (void)
    { return player_injuries & ((1u<<FRACTURED_SKULL)|(1u<<HEAVY_BLEEDING)|(1u<<BROKEN_NECK)); }
inline static void suffer_injury (enum Injury i)
    { player_injuries |= 1u << i; }
inline static void cure_injury (enum Injury i)
    { player_injuries &= ~(1u << i); }
inline static void cure_all_injuries (void)
    { player_injuries = 0; }

// location visited marker accessors
inline static bool visited_location (enum LocationId l)
    { return beenthere[l/8] & (1u<<(l%8)); }
inline static void mark_location_visited (enum LocationId l)
    { beenthere[l/8] |= (1u<<(l%8)); }
inline static unsigned locations_visited (void)
    { unsigned n = 0; for (unsigned i = 0; i < NUMOFROOMS+1; ++i) if (visited_location(i)) ++n; return n; }

// These macros yield words to use with objects (followed but not preceded
// by spaces, or with no spaces if the expansion is the empty string).

inline static bool is_plural_object (uint16_t n)
    { return c_objinfo[n].flags & OBJ_PLURAL; }
inline static const char* A_OR_AN (uint16_t n)
    { return c_objinfo[n].flags & OBJ_AN ? "an " : "a "; }
inline static const char* A_OR_AN_OR_THE (uint16_t n)
    { return is_plural_object(n) ? "the " : A_OR_AN(n); }
inline static const char* A_OR_AN_OR_BLANK (uint16_t n)
    { return is_plural_object(n) ? "" : A_OR_AN(n); }
inline static const char* IS_OR_ARE (uint16_t n)
    { return is_plural_object(n) ? "are " : "is "; }

inline static bool is_outside (void)
    { return position > ORBITING_PLANET && position < 246 && position != BUNGALOW_BEDROOM; }

//}}}-------------------------------------------------------------------
//{{{ Extern prototypes

// battlestar.c
bool save (void);
_Noreturn void die (void);
_Noreturn void live (void);
void print_score (void);

// cmd.c
void bury(void);
void chime(void);
void crash(void);
int process_command(void);
void dig(void);
void dooropen(void);
int draw(void);
void drink(void);
int drive(void);
int drop(const char *);
int eat(void);
int follow(void);
void get_player_command (const char* prompt);
int give(void);
int jump(void);
void kiss(void);
bool land (void);
bool launch (void);
void light(void);
void love(void);
bool moveplayer (location_t thatway, enum CommandId token);
void murder(void);
void news(void);
int put(void);
int puton(void);
int ride(void);
int shoot(void);
int take (uint16_t fromloc);
int takeoff(void);
int throw_object (const char *);
int use(void);
int visual(void);
int wearit(void);
bool zzz(void);

// loc.c
void write_location_short_description (void);
void write_location_long_description (void);
location_t relative_destination (enum CommandId relway);
const char* truedirec (enum CommandId way, char option);
enum CommandId compass_direction (enum CommandId way);

// obj.c
void cleanup_objects (void);
bool object_is_at (enum ObjectId oid, location_t l);
void create_object_at (enum ObjectId oid, location_t l);
void remove_object_from (enum ObjectId oid, location_t l);
void move_object_to (enum ObjectId oid, location_t f, location_t t);
unsigned count_objects_at (location_t l);
void printobjs (void);
void convert (enum EDayOrNight to);
void place_default_objects (void);
uint16_t saved_objects_checksum (uint16_t sum);
int read_objects_array (int fd);
int write_objects_array (int fd);
unsigned player_max_cumber (void);
unsigned player_max_weight (void);
unsigned player_encumber (void);
unsigned player_carrying (void);
unsigned player_melee_damage (void);
const char* melee_damage_message (unsigned dmg);
unsigned player_melee_damage_taken (void);

// parse.c
void fight (enum ObjectId enemy, unsigned strength);

//}}}-------------------------------------------------------------------
