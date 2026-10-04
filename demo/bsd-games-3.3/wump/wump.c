// Copyright (c) 1989 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

// A very new version of the age old favorite Hunt-The-Wumpus game that has
// been a part of the BSD distribution of Unix for longer than us old folk
// would care to remember.

#include "config.h"
#include <sys/wait.h>

//{{{ Local types ------------------------------------------------------

// some defines to spec out what our wumpus cave should look like
enum {
    MAX_SHOT_PATH = 6,
    MIN_N_ROOMS	= 10,
    N_ROOMS	= 20,
    MAX_N_ROOMS	= 99,
    MIN_N_DOORS	= 2,
    N_DOORS	= 3,
    MAX_N_DOORS	= 6,
    N_ARROWS	= 5,
    N_PITS	= 3,
    N_BATS	= 3,
};

//----------------------------------------------------------------------

struct Room {
    uint8_t	tunnel [MAX_N_DOORS];
    bool	has_a_pit;
    bool	has_a_bat;
};

static struct Cave {
    struct Room	room [MAX_N_ROOMS];
    uint8_t	player_loc;	// player location
    uint8_t	wumpus_loc;	// The Bad Guy location
    uint8_t	pits;		// # pits in cave
    uint8_t	bats;		// # bats
    uint8_t	rooms;		// # rooms in cave
    uint8_t	doors;		// tunnels per room
    uint8_t	arrows;		// arrows in quiver
} cave = {
    .pits = N_PITS,
    .bats = N_BATS,
    .rooms = N_ROOMS,
    .doors = N_DOORS,
    .arrows = N_ARROWS
};

//----------------------------------------------------------------------

static bool bats_nearby (void);
static void cave_init(void);
static void display_room_stats(void);
static bool do_move (uint8_t destroom);
static bool do_shoot (const uint8_t* path, uint8_t pathsz);
static void instructions (void);
static void move_wump(void);
static bool pit_nearby (void);
static bool room_has_door_to (uint8_t from, uint8_t to);
static bool validate_cave_parameters (void);
static bool wump_nearby (void);

static inline const char* plural (int n) { return n == 1 ? "" : "s"; }

//----------------------------------------------------------------------

#ifndef NDEBUG
static bool g_Debug = false;
#define DEBUG_PRINTF(...)	if (g_Debug) { printf (__VA_ARGS__); }
#else
#define DEBUG_PRINTF(...)
#endif

//}}}-------------------------------------------------------------------
//{{{ main

int main (int argc, char* const* argv)
{
    srandrand();

    for (int c; (c = getopt (argc, argv, "a:b:hp:r:t:d")) != -1;) {
	switch (c) {
	    case 'a': cave.arrows = atoi (optarg); break;
	    case 'b': cave.bats = atoi (optarg); break;
	#ifndef NDEBUG
	    case 'd': g_Debug = true; break;
	#endif
	    case 'p': cave.pits = atoi (optarg); break;
	    case 'r': cave.rooms = atoi (optarg); break;
	    case 't': cave.doors = atoi (optarg); break;
	    case '?':
	    default:
		printf ("Usage: wump [-h] [-a arrows] [-b bats] [-p pits] [-r rooms] [-t tunnels]\n");
		return EXIT_FAILURE;
	}
    }

    if (!validate_cave_parameters())
	return EXIT_FAILURE;
    cave_init();

    // and we're OFF!  da dum, da dum, da dum, da dum...
    printf ("\n" BOLD_ON "Hunt the Wumpus!" BOLD_OFF "\n"
	    "\n"
	    "You're in a cave with %u rooms and %u tunnels leading from each room.\n"
	    "There are %u bat%s and %u pit%s scattered throughout the cave, and your\n"
	    "quiver holds %u custom super anti-evil Wumpus arrows. Good luck.\n",
	    cave.rooms, cave.doors, cave.bats, plural(cave.bats),
	    cave.pits, plural(cave.pits), cave.arrows);

    for (;;) {
	display_room_stats();

	printf (BOLD_ON "ms?>" BOLD_OFF " ");
	char answer [16];
	if (!fgets (ArrayBlock(answer), stdin))
	    break;

	if (answer[0] == 'q')
	    break;
	else if (answer[0] == '?')
	    instructions();
	else if (answer[0] == 'm') {
	    uint8_t dest = atoi (&answer[1]);
	    if (!dest || (!room_has_door_to (cave.player_loc, dest-1) && dest-1 != cave.rooms)) {	// dest == cave.rooms results in entering a random magic tunnel
		printf ("   > Move where? Try saying 'm%u'.\n", cave.room[cave.player_loc].tunnel[0]+1);
		continue;
	    }
	    if (do_move (dest-1))
		break;	// returns true if game is over
	} else if (answer[0] == 's') {
	    uint8_t path [MAX_SHOT_PATH];
	    uint8_t pathsz = 0;
	    const char* nlist = &answer[1];
	    for (char* numend; pathsz < ArraySize(path) && (path[pathsz] = strtoul (nlist, &numend, 10)); nlist = numend)
		--path[pathsz++];
	    if (!pathsz) {
		printf ("   > Shoot into which room? Try saying 's%u'.\n", cave.room[cave.player_loc].tunnel[0]+1);
		continue;
	    }
	    if (do_shoot (path, pathsz))
		break;	// returns true if game is over
	} else
	    printf ("   > I don't understand. Try '?' for help.\n");
    }
    // Finish the prompt line before returning control to the calling shell.
    putchar ('\n');
    return EXIT_SUCCESS;
}

static void display_room_stats(void)
{
    // Routine will explain what's going on with the current room, as well
    // as describe whether there are pits, bats, & wumpii nearby. It's
    // all pretty mindless, really.
    printf ("\nYou are in room %u of the cave, and have %u arrow%s left.\n", cave.player_loc+1, cave.arrows, plural(cave.arrows));

    if (bats_nearby())
	printf (BOLD_ON "*rustle* *rustle*" BOLD_OFF " (must be bats nearby)\n");
    if (pit_nearby())
	printf (BOLD_ON "*whoosh*" BOLD_OFF " (I feel a draft from some pits).\n");
    if (wump_nearby())
	printf (BOLD_ON "*sniff*" BOLD_OFF " (I can smell the " BOLD_ON "evil Wumpus" BOLD_OFF" nearby!)\n");

    printf ("There are tunnels to rooms %u", cave.room[cave.player_loc].tunnel[0]+1);

    for (unsigned i = 1; i < cave.doors-1u; i++)
	if (cave.room[cave.player_loc].tunnel[i] <= cave.rooms)
	    printf (", %u", cave.room[cave.player_loc].tunnel[i]+1);
    printf (", and %u.\n", cave.room[cave.player_loc].tunnel[cave.doors-1]+1);
}

// Returns true if game is over
static bool do_move (uint8_t destroom)
{
    if (destroom >= cave.rooms) {
	destroom = nrand(cave.rooms);
	printf ("\nWith a jaunty step you enter the magic tunnel. As you do, you\n"
		"notice that the walls are shimmering and glowing. Suddenly you feel\n"
		"a very curious, warm sensation and find yourself in room %u!!\n", destroom+1);
    }
    cave.player_loc = destroom;
    for (bool just_moved_by_bats = false;;) {
	if (cave.player_loc == cave.wumpus_loc) {	// uh oh...
	    printf (BOLD_ON "*ROAR* *chomp* *snurfle* *chomp*!" BOLD_OFF "\n"
		"Much to the delight of the Wumpus, you walked right into his mouth,\n"
		"making you one of the easiest dinners he's ever had! For you, however,\n"
		"it's a rather unpleasant death. The only good thing is that it's been\n"
		"so long since the evil Wumpus cleaned his teeth that you immediately\n"
		"passed out from the stench!\n");
	    return true;
	}
	if (cave.room[cave.player_loc].has_a_pit) {
	    if (!nrand(6)) {
		printf ("Without conscious thought you grab for the side of the cave and manage\n"
			"to grasp onto a rocky outcrop. Beneath your feet stretches the limitless\n"
			"depths of a bottomless pit! Rock crumbles beneath your feet!\n");
		return false;
	    } else {
		printf (BOLD_ON "*AAAUUUUGGGGGHHHHHhhhhhhhhhh...*" BOLD_OFF "\n"
			"The whistling sound and updraft as you walked into this room of the\n"
			"cave apparently wasn't enough to clue you in to the presence of the\n"
			"bottomless pit. You have a lot of time to reflect on this error as\n"
			"you fall many miles to the core of the earth. Look on the bright side;\n"
			"you can at least find out if Jules Verne was right...\n");
		return true;
	    }
	}
	if (cave.room[cave.player_loc].has_a_bat) {
	    printf (BOLD_ON "*flap*  *flap*  *flap*" BOLD_OFF "  (humongous bats pick you up and move you%s!)\n", just_moved_by_bats ? " again" : "");
	    cave.player_loc = nrand (cave.rooms);
	    just_moved_by_bats = true;
	} else
	    return false;
    }
}

// Implement shooting arrows. Arrows are shot by the player indicating
// a space-separated list of rooms that the arrow should pass through;
// if any of the rooms they specify are not accessible via tunnel from
// the room the arrow is in, it will instead fly randomly into another
// room. If the player hits the wumpus, this routine will indicate
// such. If it misses, this routine will *move* the wumpus one room.
// If it's the last arrow, the player then dies... Returns true if the
// player has won or died, false if nothing has happened.
//
static bool do_shoot (const uint8_t* path, uint8_t pathsz)
{
    uint8_t arrow_loc = cave.player_loc;
    for (unsigned i = 0; i < pathsz; ++i) {
	uint8_t destroom = path[i];
	if (room_has_door_to (arrow_loc, destroom))
	    arrow_loc = destroom;
	else if (destroom >= cave.rooms) {
	    puts ("A faint gleam tells you the arrow has gone through a magic tunnel!");
	    arrow_loc = nrand (cave.rooms);
	} else {
	    printf (BOLD_ON "*thunk*" BOLD_OFF " The arrow can't find a way from %u to %u\n", arrow_loc+1, destroom+1);
	    arrow_loc = cave.room[arrow_loc].tunnel[nrand(cave.doors)];
	    if (arrow_loc == cave.player_loc)
		printf ("\tIt flies back into your room!\n");
	    else
		printf ("\tIt flies randomly into room %u!\n", arrow_loc+1);
	}

	// now we've gotten into the new room let us see if El Wumpo is
	// in the same room ... if so we've a HIT and the player WON!
	if (arrow_loc == cave.wumpus_loc) {
	    printf (BOLD_ON "*thwock!* *groan* *crash*" BOLD_OFF "\n\n"
		    "A horrible roar fills the cave, and you realize, with a smile, that you\n"
		    "have slain the evil Wumpus and won the game! You don't want to tarry for\n"
		    "long, however, because not only is the Wumpus famous, but the stench of\n"
		    "dead Wumpus is also quite well known, a stench plenty enough to slay the\n"
		    "mightiest adventurer at a single whiff!!\n");
	    return true;
	} else if (arrow_loc == cave.player_loc) {
	    printf ("\n" BOLD_ON "*Thwack!*" BOLD_OFF " A sudden piercing feeling informs you that the ricochet\n"
		    "of your wild arrow has resulted in it wedging in your side, causing\n"
		    "extreme agony. The evil " BOLD_ON "Wumpus" BOLD_OFF", with its psychic powers, realizes this\n"
		    "and immediately rushes to your side, not to help, alas, but to " BOLD_ON "EAT YOU!\n"
		    "(*CHOMP*)" BOLD_OFF "\n");
	    return true;
	}

	// See if the arrow can fly farther; longer shots are more dangerous
	if (pathsz > 2) {
	    if (!nrand(2*MAX_SHOT_PATH-pathsz)) {
		printf ("Your bowstring breaks! *twaaaaaang*\nThe arrow is weakly shot and can go no further!\n");
		break;
	    } else if (!nrand(2*MAX_SHOT_PATH)) {
		printf ("The arrow wavers in its flight and and can go no further!\n");
		break;
	    }
	}
    }

    // The arrow has been used up; check if it was the last
    if (!--cave.arrows) {
	printf ("\nYou turn and look at your quiver, and realize with a sinking feeling\n"
		"that you've just shot your last arrow (figuratively, too). Sensing this\n"
		"with its psychic powers, the evil Wumpus rampagees through the cave, finds\n"
		"you, and with a mighty *ROAR* eats you alive!\n");
	return true;
    }

    // The Wumpus will occasionally be spooked and will move
    if (!nrand (cave.arrows)) {
	move_wump();
	if (cave.wumpus_loc == cave.player_loc) {
	    printf (BOLD_ON "*ROAR* *chomp* *snurfle* *chomp*!" BOLD_OFF "\n"
		"Spooked by your arrow, the Wumpus randomly wandered into your room, and,\n"
		"in a fit of rage, immediately ate you. The only good thing is that it's been\n"
		"so long since the evil Wumpus cleaned his teeth that you immediately\n"
		"passed out from the stench!\n");
	    return true;
	}
    }
    return false;
}

//}}}-------------------------------------------------------------------
//{{{ Cave data access

static bool bats_nearby(void)
{
    // check for bats in the immediate vicinity
    for (uint8_t i = 0; i < cave.doors; ++i)
	if (cave.room[cave.room[cave.player_loc].tunnel[i]].has_a_bat)
	    return true;
    return false;
}

static bool pit_nearby(void)
{
    // check for pits in the immediate vicinity
    for (uint8_t i = 0; i < cave.doors; ++i)
	if (cave.room[cave.room[cave.player_loc].tunnel[i]].has_a_pit)
	    return true;
    return false;
}

static bool wump_nearby (void)
{
    // check for a wumpus within TWO caves of where we are
    for (uint8_t i = 0; i < cave.doors; ++i) {
	if (cave.room[cave.player_loc].tunnel[i] == cave.wumpus_loc)
	    return true;
	for (uint8_t j = 0; j < cave.doors; ++j)
	    if (cave.room[cave.room[cave.player_loc].tunnel[i]].tunnel[j] == cave.wumpus_loc)
		return true;
    }
    return false;
}

static void move_wump (void)
{
    cave.wumpus_loc = cave.room[cave.wumpus_loc].tunnel[nrand(cave.doors)];
}

//}}}-------------------------------------------------------------------
//{{{ Cave initialization

static bool validate_cave_parameters (void)
{
    if (cave.rooms < MIN_N_ROOMS) {
	puts ("No self-respecting wumpus would live in such a small cave!");
	return false;
    }
    if (cave.rooms > MAX_N_ROOMS) {
	puts ("Even wumpii can't furnish caves that large!");
	return false;
    }
    if (cave.doors < MIN_N_DOORS) {
	puts ("Wumpii like extra doors in their caves!");
	return false;
    }
    if (cave.doors > MAX_N_DOORS || cave.doors > cave.rooms - (cave.rooms / 4)) {
	puts ("Too many tunnels! The cave collapsed!\n(Fortunately, the wumpus escaped!)");
	return false;
    }
    if (cave.bats > cave.rooms / 2) {
	puts ("The wumpus refused to enter the cave, claiming it was too crowded!");
	return false;
    }
    if (cave.pits > cave.rooms / 2) {
	puts ("The wumpus refused to enter the cave, claiming it was too dangerous!");
	return false;
    }
    return true;
}

static bool room_has_door_to (uint8_t from, uint8_t to)
{
    for (uint8_t i = 0; i < cave.doors; ++i)
	if (cave.room[from].tunnel[i] == to)
	    return true;
    return false;
}

static unsigned get_free_door_slot (uint8_t rid)
{
    for (unsigned i = 0; i < cave.doors; ++i)
	if (cave.room[rid].tunnel[i] >= cave.rooms)
	    return i;
    return UINT_MAX;
}

static int uint8_compare (const void* va, const void* vb)
{
    uint8_t a = *(const uint8_t*)va, b = *(const uint8_t*)vb;
    return (b < a) - (a < b);
}

static void cave_init (void)
{
    // Reset all doors
    for (unsigned i = 0; i < cave.rooms; ++i)
	for (unsigned j = 0; j < cave.doors; ++j)
	    cave.room[i].tunnel[j] = UINT8_MAX;

    // Build fully connected network first
    // Create connected set by shuffling all room numbers
    // and back-connecting to the connected set.
    uint8_t connected [cave.rooms];
    iota_u8 (connected, cave.rooms);
    random_shuffle_u8 (connected, cave.rooms);
    for (unsigned i = 1; i < cave.rooms; ++i) {
	uint8_t from = connected[i];
	uint8_t to;
	unsigned tods;
	do {
	    to = connected[nrand(i)];
	    tods = get_free_door_slot (to);
	} while (room_has_door_to(from, to) || tods >= cave.doors);
	cave.room[from].tunnel[0] = to;
	cave.room[to].tunnel[tods] = from;
    }

    // Randomly fill the remaining tunnel slots
    for (unsigned i = 0; i < cave.rooms; ++i) {
	for (unsigned j = 1; j < cave.doors; ++j) {
	    uint8_t from = connected[i];
	    if (cave.room[from].tunnel[j] < cave.rooms)
		continue; // already used
	    // Find the first available slot
	    uint8_t to = UINT8_MAX;
	    unsigned tods = UINT_MAX;
	    for (unsigned k = i+1; k < cave.rooms; ++k) {
		to = connected[k];
		tods = get_free_door_slot (to);
		if (!room_has_door_to(from, to) && tods < cave.doors)
		    break;
		to = UINT8_MAX;
	    }
	    if (to < cave.rooms) {
		cave.room[from].tunnel[j] = to;
		cave.room[to].tunnel[tods] = from;
	    } else // last room may end up with two free slots; make them one-way
		cave.room[from].tunnel[j] = connected[nrand(from)];
	}
    }

    // Now that we're done, sort the tunnels in each of the
    // rooms to make it easier on the intrepid adventurer.
    for (unsigned i = 0; i < cave.rooms; ++i)
	qsort (cave.room[i].tunnel, cave.doors, sizeof(cave.room[i].tunnel[0]), uint8_compare);

    // In debug mode, print the layout
    for (unsigned i = 0; i < cave.rooms; ++i) {
	DEBUG_PRINTF ("<room %u  has tunnels to ", i+1);
	for (unsigned j = 0; j < cave.doors; ++j)
	    DEBUG_PRINTF ("%u ", cave.room[i].tunnel[j]+1);
	DEBUG_PRINTF (">\n");
    }

    // Place bats
    for (unsigned i = 0; i < cave.bats; ++i) {
	uint8_t batroom;
	do {
	    batroom = nrand (cave.rooms);
	} while (cave.room[batroom].has_a_bat);
	cave.room[batroom].has_a_bat = true;
	DEBUG_PRINTF ("<bat in room %u>\n", batroom+1);
    }

    // Place pits
    for (unsigned i = 0; i < cave.pits; ++i) {
	uint8_t pitroom;
	do {
	    pitroom = nrand (cave.rooms);
	} while (cave.room[pitroom].has_a_pit);
	cave.room[pitroom].has_a_pit = true;
	DEBUG_PRINTF ("<pit in room %u>\n", pitroom+1);
    }

    // Place wumpus anywhere
    cave.wumpus_loc = nrand (cave.rooms);
    DEBUG_PRINTF ("<wumpus in room %u>\n", cave.wumpus_loc+1);

    // Place player in any empty room
    do {
	cave.player_loc = nrand (cave.rooms);
    } while (cave.player_loc == cave.wumpus_loc || cave.room[cave.player_loc].has_a_bat);
}

//}}}-------------------------------------------------------------------
//{{{ Instructions

static void instructions (void)
{
    puts ("\n\t" BOLD_ON "Welcome to the game of Hunt the Wumpus." BOLD_OFF "\n"
	"\n"
	"The " BOLD_ON "Wumpus" BOLD_OFF " typically lives in a cave of twenty rooms, with each room\n"
	"having three tunnels connecting it to other rooms in the cavern. Caves\n"
	"may vary, however, depending on options specified when starting the game.\n"
	"\n"
	"The game has the following hazards for intrepid adventurers:\n"
	"\n"
	"  Pits	 -- If you fall into one of the bottomless pits, you find yourself\n"
	"	    slung back out on the far side of the Earth and in very poor\n"
	"	    shape to continue your quest since you're dead.\n"
	"\n"
	"  Bats	 -- As with any other cave, the Wumpus cave has bats in residence.\n"
	"	    These are a bit more potent, however, and if you stumble into\n"
	"	    one of their rooms they will rush up and carry you elsewhere\n"
	"	    in the cave.\n"
	"\n"
	"  " BOLD_ON "Wumpus" BOLD_OFF " -- If you happen to walk into the room the Wumpus is in you'll\n"
	"	    find that he has quite an appetite for young adventurous humans!\n");

    const char* lines = getenv ("LINES");
    if (lines && atoi(lines) < 35) {
	printf ("--enter for more--");
	getchar();
    }
    puts ("The " BOLD_ON "Wumpus" BOLD_OFF " is not bothered by the hazards since he has sucker feet and\n"
	"is too big for a bat to lift. He is a sedentary creature by nature,\n"
	"but if you try to shoot him and miss, there is a chance that he'll up\n"
	"and move himself into another room.\n"
	"\n"
	"Each turn you may either move or shoot a crooked arrow. Move by specifying\n"
	"'m' command and the destination room. Shoot by giving the 's' command a\n"
	"list of rooms for the deadly shaft to fly through. For example: m9, s8 4.\n"
	"\n"
	"If your path for the arrow is incorrect, however, it will flail about in\n"
	"the room it can't understand and randomly pick a tunnel to continue\n"
	"through. You might just end up shooting yourself in the foot if you're\n"
	"not careful!  On the other hand, if you shoot the " BOLD_ON "Wumpus" BOLD_OFF " you've " BOLD_ON "WON!" BOLD_OFF"\n"
	"\n"
	"Good luck.");
}

//}}}-------------------------------------------------------------------
