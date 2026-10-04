// Copyright (c) 1980 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "spirhunt.h"
#include <sys/uio.h>
#include <sys/stat.h>

//{{{ Local prototypes -------------------------------------------------

static void setup (void);
static void play (void);
static uint16_t sum_save_array (void);
static bool verify_locations (void);
static bool restore (void);
static void save_score (int score);

//}}}-------------------------------------------------------------------
//{{{ Global variables

//{{{2 DeviceName
const char DeviceName [NDEV][16] = {
    "warp drive",
    "near scanners",
    "far scanners",
    "plasers",
    "torpedoes",
    "impulse engines",
    "shield control",
    "computer",
    "radio",
    "life support",
    "cloak"
};
//}}}2
//{{{2 Systemname
const char Systemname[NINHAB][16] = {
    "ERROR",
    "Talos IV",
    "Rigel III",
    "Deneb VII",
    "Canopus V",
    "Icarus I",
    "Prometheus II",
    "Omega VII",
    "Elysium I",
    "Scalos IV",
    "Procyon IV",
    "Arachnid I",
    "Argo VIII",
    "Triad III",
    "Echo IV",
    "Nimrod III",
    "Nemisis IV",
    "Centarurus I",
    "Kronos III",
    "Spectros V",
    "Beta III",
    "Tranguli VI",
    "Pyris III",
    "Triachus",
    "Marcus XII",
    "Kaland",
    "Ardana",
    "Stratos",
    "Eden",
    "Arrikis",
    "Eridani IV",
    "Exo III"
};
//}}}2
//{{{2 Param

const struct Param_struct Param = {
    .crew	= 387,
    .brigfree	= 400,
    .energy	= 5000,
    .energylow	= 1000,
    .shield	= 1500,
    .shupengy	= 40,
    .stopengy	= 50,
    .cloakenergy= 1000,
    .pirate_crew= 200,
    .piratepwr	= 350,
    .bases	= MAXBASES,
    .pirates	= NQUADS*NQUADS/2,
    .torped	= 10,
    .warptime	= 10,
    .date	= 2000,
    .time	= 14,
    .reserves	= 10,
    .resource	= (NQUADS*NQUADS/2)*14,
    .dockfac	= 0.5,
    .hitfac	= 0.5,
    .plasfac	= 0.8,
    .regenfac	= 0.2,
    .eventdly = {
	[E_SNOVA] = 0.5,
	[E_LRTB]  =25.0,
	[E_PATSB] = 1.0,
	[E_PDESB] = 3.0,
	[E_ISSUE] = 1.0,
	[E_SNAP]  = 0.5,
	[E_ENSLV] = 0.5,
	[E_REPRO] = 2.0
    }
};
//}}}2

struct event	Event [MAXEVENTS] = {};	// dynamic event list; one entry per pending event
struct quad	Quad [NQUADS][NQUADS] = {};

struct Ship_struct	Ship = {};
struct Game_struct	Game = {};
struct Move_struct	Move = {};
struct Now_struct	Now = {};
struct Etc_struct	Etc = {};

struct Snapshot_struct	Snapshot = {};

//}}}-------------------------------------------------------------------

int main (void)
{
    initialize_curses();
    create_windows();
    if (!restore()) {
	print_msg ("\n   * * *   SPACE PIRATE HUNT   * * *\n\n");
	setup();
    }
    play();
    return EXIT_SUCCESS;
}

//{{{ setup ------------------------------------------------------------

static void setup (void)
{
    Now.resource = Param.resource;
    Ship.energy = Param.energy;
    Ship.torped = Param.torped;
    Ship.shield = Param.shield;
    Ship.reserves = Param.reserves;
    Ship.crew = Param.crew;
    Ship.brigfree = Param.brigfree;
    Ship.shldup = true;
    Ship.cond = GREEN;
    Ship.warp = 50;
    Now.date = Param.date + nrand (Param.date/20);
    xsched(E_SNOVA, 1, 0, 0, 0);
    xsched(E_LRTB, Param.pirates, 0, 0, 0);
    xsched(E_PATSB, 1, 0, 0, 0);
    xsched(E_ISSUE, 1, 0, 0, 0);
    xsched(E_SNAP, 1, 0, 0, 0);
    Ship.sect.x = nrand(NSECTS);
    Ship.sect.y = nrand(NSECTS);

    // setup stars
    uint8_t inhabited_left = NINHAB;
    for (unsigned i = 0; i < NQUADS; ++i) {
	for (unsigned j = 0; j < NQUADS; ++j) {
	    struct quad* q = &Quad[i][j];
	    q->bases = 0;
	    q->pirates = 0;
	    q->stars = nrand(QUAD_STARS);
	    q->holes = nrand(QUAD_HOLES);

	    // NINHAB of NQUADS*NQUADS are inhabited
	    if (!nrand(NQUADS*NQUADS/NINHAB) && inhabited_left)
		q->systemname = --inhabited_left;
	    else
		q->systemname = 0;
	}
    }

    // position starbases
    Now.bases = 2 + nrand(Param.bases-2);
    for (unsigned i = 0; i < Now.bases; ++i) {
	struct quad* q;
	unsigned ix, iy;
	for (;;) {
	    ix = nrand(NQUADS);
	    iy = nrand(NQUADS);
	    q = &Quad[iy][ix];
	    if (q->bases > 0)
		continue;
	    break;
	}
	q->bases = 1;
	Now.base[i].x = ix;
	Now.base[i].y = iy;
    }

    // start at random starbase
    Ship.quad = Now.base [nrand (Now.bases)];

    // position pirates
    unsigned npirates = Param.pirates/2 + nrand(Param.pirates/2);
    Now.time = Now.resource / npirates;
    for (unsigned i = npirates; i > 0;) {
	unsigned plump = nrand(4) + 1;
	if (plump > i)
	    plump = i;
	while (1) {
	    unsigned ix = nrand(NQUADS);
	    unsigned iy = nrand(NQUADS);
	    struct quad* q = &Quad[iy][ix];
	    if (q->pirates + plump > QUAD_PIRATES)
		continue;
	    q->pirates += plump;
	    i -= plump;
	    break;
	}
    }

    // initialize this quadrant
    print_msg ("%u pirates\n%u starbases ", pirates_remaining(), Now.bases);
    for (unsigned i = 0; i < Now.bases; ++i)
	print_msg ("%s " QUAD_FMT, i ? "," : "at", Now.base[i].x, Now.base[i].y);
    print_msg ("\n");
    initquad (false);
    attack(0);
    checkcond();
}

// Generate quadrant upon entering
//
// A quadrant is initialized from the information held in the Quad
// matrix. Basically, everything is just initialized randomly, except
// for the starship, which goes into a fixed sector.
//
// If there are pirates in the quadrant, the captain is informed that
// the condition is RED, and he is given a chance to put his shields
// up if the computer is working.
//
// The flag 'docked' is set if the ship will be docked on arrival.
//
void initquad (bool docked)
{
    struct quad* q = current_quad();

    // ignored supernova'ed quadrants (this is checked again later anyway
    if (q->stars == SUPERNOVA)
	return;

    // have we blundered into a battle zone w/ shields down?
    if (q->pirates > 0 && !docked && Ship.cond != RED && Ship.cond != CLOAK) {
	Ship.cond = RED;
	if (!Ship.shldup && !device_damaged (COMPUTER))
	    shield();
    }

    // randomly place in-quadrant entities
    for (unsigned i = 0; i < current_quad()->pirates; ++i) {
	Etc.pirate[i].sect = random_empty_sector();
	Etc.pirate[i].power = Param.piratepwr;
    }
    sort_pirates();
    if (q->bases > 0)
	Etc.starbase = random_empty_sector();
    if (q->systemname != 0)
	Etc.inhabited = random_empty_sector();
    for (unsigned i = 0; i < q->stars; ++i)
	Etc.stars[i] = random_empty_sector();
    for (unsigned i = 0; i < q->holes; ++i)
	Etc.blackholes[i] = random_empty_sector();

    Move.newquad = 1;
}

struct xy random_empty_sector (void)
{
    struct xy r;
    do {
	r.x = nrand(NSECTS);
	r.y = nrand(NSECTS);
    } while (sector_contents(r.x,r.y) != EMPTY);
    return r;
}

//}}}-------------------------------------------------------------------
//{{{ play

// INSTRUCTION READ AND MAIN PLAY LOOP
//
// Well folks, this is it. Here we have the guts of the game.
// This routine executes moves. It sets up per-move variables,
// gets the command, and executes the command. After the command,
// it calls events() to use up time, attack() to have pirates
// attack if the move was not free, and checkcond() to check up
// on how we are doing after the move.

void save_game (void)
{
    if (save())
	exit (EXIT_SUCCESS);
    print_msg ("Error: failed to save the game\n");
}

void myreset (void)
    { exit (EXIT_SUCCESS); }

static void play (void)
{
    for (;;) {
	draw_screen();
	Move.free = 1;
	Move.time = 0.0;
	Move.shldchg = 0;
	Move.newquad = 0;
	Move.resting = 0;
	main_command();
	events (0);
	attack (0);
	checkcond();
    }
}

// Check for Condition After a Move
//
// Various ship conditions are checked. First we check
// to see if we have already lost the game, due to running
// out of life support reserves, running out of energy,
// or running out of crew members. The check for running
// out of time is in events().
//
// If we are in automatic override mode (SUPERNOVA && Ship.cond == RED), we
// don't want to do anything else, lest we call autover recursively.
//
// In the normal case, if there is a supernova, we call
// autover() to help us escape. If after calling autover()
// we are still in the grips of a supernova, we get burnt up.
//
// If there are no pirates in this quadrant, we nullify any
// distress calls which might exist.
//
// We then set the condition code, based on the energy level
// and battle conditions.
//
void checkcond (void)
{
    // See if we are still alive and well
    if (Ship.reserves < 0.0)
	lose (L_NOLIFE);
    if (Ship.energy <= 0)
	lose (L_NOENGY);
    if (Ship.crew <= 0)
	lose (L_NOCREW);

    // Call in automatic override if appropriate
    if (current_quad()->stars == SUPERNOVA && Ship.cond != RED)
	autover();
    if (current_quad()->stars == SUPERNOVA)
	lose (L_SNOVA);

    // Cancel distress call if appropriate
    if (!current_quad()->pirates)
	kill_distress_call (Ship.quad.x, Ship.quad.y, 1);

    // set condition code
    if (Ship.cond == DOCKED)
	return;
    if (current_quad()->pirates > 0)
	Ship.cond = RED;
    else if (Ship.energy < Param.energylow)
	Ship.cond = YELLOW;
    else
	Ship.cond = GREEN;
    return;
}

//}}}-------------------------------------------------------------------
//{{{ Game save and restore

//{{{2 s_save_array

#define WVAR(v)	{ &v, sizeof(v) }
#define WARR(v)	{ &v[0], sizeof(v) }

static struct iovec s_save_array[] = {
    WVAR (Ship),
    WVAR (Now),
    WVAR (Etc),
    WVAR (Game),
    WVAR (Move),
    WARR (Quad),
    WARR (Event)
};
//}}}2

static uint16_t sum_save_array (void)
{
    uint16_t sum = 0;
    for (unsigned i = 0; i < ArraySize(s_save_array); ++i)
	sum = bsdsum ((const uint8_t*) s_save_array[i].iov_base, s_save_array[i].iov_len, sum);
    return sum;
}

struct save_header { char magictext[6]; uint16_t sum; };

bool save (void)
{
    char savename [PATH_MAX];
    player_saved_game_file (ArrayBlock(savename), SPIRHUNT_SAVE_NAME);

    int fd = creat (savename, S_IRUSR| S_IWUSR);
    if (fd < 0) {
	print_msg ("Error: unable to create save file '%s': %s\n", savename, strerror(errno));
	return false;
    }
    struct save_header header = {{'s','p','i','r','h','t'}, sum_save_array()};
    if (sizeof(header) != write (fd, &header, sizeof(header))
	|| 0 >= writev (fd, s_save_array, ArraySize(s_save_array))
	|| 0 > close (fd)) {
	print_msg ("Error writing save file '%s': %s\n", savename, strerror(errno));
	close (fd);
	return false;
    }
    return true;
}

static bool verify_locations (void)
{
    return true;
}

static bool restore (void)
{
    char savename [PATH_MAX];
    player_saved_game_file (ArrayBlock(savename), SPIRHUNT_SAVE_NAME);
    int fd = open (savename, O_RDONLY);
    if (fd < 0)
	return false;

    // Restore errors remain visible after the terminal screen is closed.
    struct save_header header;
    if (sizeof(header) != read (fd, &header, sizeof(header))
	|| 0 >= readv (fd, s_save_array, ArraySize(s_save_array))) {
	close (fd);
        cleanup_curses();
        fprintf (stderr, "Error: unable to read complete saved game '%s'\n", savename);
	exit (EXIT_FAILURE);
    }
    close (fd);
    if (memcmp (header.magictext, "spirht", sizeof(header.magictext)) != 0
	|| header.sum != sum_save_array()
	|| !verify_locations()) {
        cleanup_curses();
        fprintf (stderr, "Error: saved game '%s' is corrupt. Please delete it.\n", savename);
	exit (EXIT_FAILURE);
    }
    unlink (savename);
    return true;
}

//}}}-------------------------------------------------------------------
//{{{ Win and lose

// Signal game won
//
// This routine prints out the win message, arranges to print out
// your score, tells you if you have a promotion coming to you,
// cleans up the current input line, and exits.
//
// Pretty straightforward, although the promotion algorithm is
// pretty off the wall.

_Noreturn void win (void)
{
    Move.endgame = 1;
    print_msg ("\nCongratulations, you have saved the Galaxy\n");
    int score = print_score();
    cleanup_curses();
    save_score (score);
    exit (EXIT_SUCCESS);
}

// PRINT OUT LOSER MESSAGES
//
// The messages are printed out, the score is computed and
// printed, and the game is restarted. Oh yeh, any special
// actions which need be taken are taken.

_Noreturn void lose (enum LoseReason why)
{
    Game.killed = (why != L_NOTIME);
    static const char Losemsg[] = {
	"You got bored\0"
	"You ran out of time\0"
	"You ran out of energy\0"
	"You have been destroyed\0"
	"You destroyed yourself by nova'ing that star\0"
	"You have been caught in a supernova\0"
	"You just suffocated in outer space\0"
	"You could not be rematerialized\0"
	"Your ship was ticketed for speeding\0"
	"You have burned up in a star\0"
	"Well, you destroyed yourself, but it didn't do any good\0"
	"Your last crew member died"
    };
    print_msg ("\n%s\n", zstrn (Losemsg, why, 14));
    int score = print_score();
    cleanup_curses();
    save_score (score);
    exit (EXIT_SUCCESS);
}

//}}}-------------------------------------------------------------------
//{{{ Scoring

struct Score {
    uint32_t	score;
    char	name [12];
};

static int compare_scores (const void* v1, const void* v2)
    { return sign (((const struct Score*)v2)->score - ((const struct Score*)v1)->score); }

static void save_score (int score)
{
    struct Score scores [MAXSCORES] = {};
    read_score_file (SPIRHUNT_SCOREFILE, SCOREFILE_MAGIC, scores, sizeof(scores));

    // Check each score and zero if invalid
    for (struct Score *s = scores, *send = &scores[ArraySize(scores)]; s < send; ++s)
	if (!s->name[0] || s->name[sizeof(s->name)-1] || s->score > 10000000)
	    memset (s, 0, sizeof(*s));
    // Resort to account for the above zeroing
    qsort (ArrayBlock(scores), sizeof(scores[0]), compare_scores);

    // Make a record for this game's score
    struct Score game_score = {};
    game_score.score = max_i (score, 0);
    snprintf (ArrayBlock(game_score.name), "%s", player_name());

    // Add this game's score, if it is high enough
    struct Score* lowscore = &scores[ArraySize(scores)-1];
    if (game_score.name[0] && lowscore->score < game_score.score) {
	*lowscore = game_score;
	// Resort the new score
	qsort (ArrayBlock(scores), sizeof(scores[0]), compare_scores);
	// And write the score file
	write_score_file (SPIRHUNT_SCOREFILE, SCOREFILE_MAGIC, scores, sizeof(scores));
    }

    // List top scores
    if (!scores[0].score)
	return;
    puts ("\n-#--Name---------Score--");
    for (unsigned i = 0; i < ArraySize(scores) && scores[i].score; ++i) {
	if (game_score.score == scores[i].score && 0 == strcmp (game_score.name, scores[i].name))
	    printf (BOLD_ON);
	printf ("%2u: %-12s %5u\n" BOLD_OFF, i+1, scores[i].name, scores[i].score);
    }
}

//}}}-------------------------------------------------------------------
//{{{ Miscellaneous

float fnrand (void)
{
    float t = rand() & 077777;
    return t / 32767.0;
}

//}}}-------------------------------------------------------------------
