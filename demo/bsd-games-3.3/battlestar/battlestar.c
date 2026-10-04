// This file is free software, distributed under the BSD license.

#include "battlestar.h"
#include <sys/stat.h>
#include <sys/uio.h>

//{{{ Global variables -------------------------------------------------

gametime_t ourtime = 0;
gametime_t rythmn = 0;
gametime_t ate = 0;
gametime_t snooze = 3*CYCLE/2;
location_t position = LUXURIOUS_STATEROOM;
uint16_t direction = NORTH;
uint16_t game_states = 0;
uint16_t player_injuries = 0;
struct GameScore game_score = {};
uint8_t fuel = TANKFULL;
uint8_t torps = TORPEDOES;
uint8_t matchcount = 20;
uint8_t godready = 0;
uint8_t win = 1;
uint8_t beenthere [(NUMOFROOMS+1+7)/8] = {};

//}}}-------------------------------------------------------------------

static bool restore (void);
static uint16_t savegame_checksum (void);
static const char* player_rating (const struct GameScore* score);
static unsigned total_score (const struct GameScore* score);
static int compare_scores (const void* v1, const void* v2);
static void save_score (void);
static void on_crash (int sig);
static void install_crash_handler (void);

//----------------------------------------------------------------------

int main (void)
{
    initialize_curses();
    endwin();
    install_crash_handler();
    atexit (cleanup_objects);
    if (!restore()) {
	place_default_objects();
	puts (BOLD_ON "\n"
	    "		===========================	\n"
	    "		    B A T T L E S T A R		\n"
	    "		===========================	\n"
	    BOLD_OFF "\n"
	    "	First Adventure game written by His Lordship,\n"
	    "	     the honorable Admiral D.W. Riggle");
    }
    for (;;) {
	news();
	if (game_state (LAUNCHED))
	    crash();	// decrements fuel & crashes when empty
	if (game_state (MATCH_LIGHT)) {
	    puts("Your match splutters out.");
	    clear_game_state (MATCH_LIGHT);
	}
	if (!game_state (CANTSEE) || object_is_at (LAMPON, INVENTORY) || object_is_at (LAMPON, position)) {
	    if (!visited_location (position) || game_state (IS_VERBOSE)) {
		mark_location_visited (position);
		write_location_long_description();
	    } else
		write_location_short_description();
	    printobjs();
	} else
	    puts ("It's too dark to see anything in here!");
	do {
	    get_player_command (BOLD_ON "> " BOLD_OFF);
	} while (process_command() != 0);
    }
}

//{{{ Save and restore -------------------------------------------------

//{{{2 s_save_array

#define WVAR(v)	{ &v, sizeof(v) }
#define WARR(v)	{ &v[0], sizeof(v) }

static struct iovec s_save_array[] = {
    WVAR (ourtime),
    WVAR (rythmn),
    WVAR (ate),
    WVAR (snooze),
    WVAR (position),
    WVAR (direction),
    WVAR (game_states),
    WVAR (player_injuries),
    WVAR (game_score),
    WVAR (fuel),
    WVAR (torps),
    WVAR (matchcount),
    WVAR (godready),
    WVAR (win),
    WARR (beenthere)
};
//}}}2

static uint16_t savegame_checksum (void)
{
    uint16_t sum = 0;
    for (unsigned i = 0; i < ArraySize(s_save_array); ++i)
	sum = bsdsum ((const uint8_t*) s_save_array[i].iov_base, s_save_array[i].iov_len, sum);
    return saved_objects_checksum (sum);
}

struct save_header { char magictext[6]; uint16_t sum; };

static bool restore (void)
{
    char savename [PATH_MAX];
    player_saved_game_file (ArrayBlock(savename), BATTLESTAR_SAVE_NAME);
    int fd = open (savename, O_RDONLY);
    if (fd < 0)
	return false;

    struct save_header header;
    if (sizeof(header) != read (fd, &header, sizeof(header))
	|| 0 >= readv (fd, s_save_array, ArraySize(s_save_array))
	|| 0 >= read_objects_array (fd)) {
	printf ("Error: reading '%s': %s\n", savename, strerror(errno));
	close (fd);
	exit (EXIT_FAILURE);
    }
    close (fd);

    if (memcmp (header.magictext, "btlstr", sizeof(header.magictext)) != 0
	|| header.sum != savegame_checksum()
	|| position > NUMOFROOMS || direction < NORTH || direction > WEST) {
	printf ("Error: saved game '%s' is corrupt. Please delete it.\n", savename);
	exit (EXIT_FAILURE);
    }
    unlink (savename);
    return true;
}

bool save (void)
{
    char savename [PATH_MAX];
    player_saved_game_file (ArrayBlock(savename), BATTLESTAR_SAVE_NAME);

    int fd = creat (savename, S_IRUSR| S_IWUSR);
    if (fd < 0) {
	printf ("Error: unable to create save file '%s': %s\n", savename, strerror(errno));
	return false;
    }
    struct save_header header = {{'b','t','l','s','t','r'}, savegame_checksum()};
    if (sizeof(header) != write (fd, &header, sizeof(header))
	|| 0 >= writev (fd, s_save_array, ArraySize(s_save_array))
	|| 0 >= write_objects_array (fd)
	|| 0 > close (fd)) {
	printf ("Error writing save file '%s': %s\n", savename, strerror(errno));
	close (fd);
	return false;
    }
    return true;
}

//}}}-------------------------------------------------------------------
//{{{ Scoring

_Noreturn void die (void)
{
    puts ("\nYou die.\n");
    print_score();
    save_score();
    exit (EXIT_SUCCESS);
}

_Noreturn void live (void)
{
    puts ("\nYou win!\n");
    print_score();
    save_score();
    exit (EXIT_SUCCESS);
}

void print_score (void)
{
    printf ("\tPLEASURE\tPOWER\t\tEGO\n");
    printf ("\t%3u\t\t%3u\t\t%3u\n\n", game_score.pleasure, game_score.power, game_score.ego);
    printf ("This gives you the rating of %s in %u turns.\n", player_rating (&game_score), ourtime);
    unsigned nvisited = locations_visited();
    printf ("You have visited %u out of %u rooms this run (%u%%).\n", nvisited, NUMOFROOMS, nvisited * 100 / NUMOFROOMS);
}

static const char* player_rating (const struct GameScore* score)
{
    unsigned msc = max_i (max_i (score->pleasure, score->power), score->ego);
    if (msc == score->pleasure) {
	if (msc < 5)		return "novice";
	else if (msc < 20)	return "junior voyeur";
	else if (msc < 35)	return "Don Juan";
	else			return "Marquis De Sade";
    } else if (msc == score->power) {
	if (msc < 5)		return "serf";
	else if (msc < 8)	return "Samurai";
	else if (msc < 13)	return "Klingon";
	else if (msc < 22)	return "Darth Vader";
	else			return "Sauron the Great";
    } else {
	if (msc < 5)		return "Polyanna";
	else if (msc < 10)	return "philanthropist";
	else if (msc < 20)	return "Tattoo";
	else			return "Mr.Roarke";
    }
}

//----------------------------------------------------------------------

struct Score {
    struct GameScore score;
    char name [10];
};

static unsigned total_score (const struct GameScore* score)
    { return score->pleasure + score->power + score->ego; }

static int compare_scores (const void* v1, const void* v2)
{
    const struct Score *s1 = v1, *s2 = v2;
    return sign (total_score (&s2->score) - total_score (&s1->score));
}

static void save_score (void)
{
    struct Score scores [MAXSCORES] = {};
    read_score_file (BATTLESTAR_SCOREFILE, SCOREFILE_MAGIC, scores, sizeof(scores));

    // Check each score and zero if invalid
    for (struct Score *s = scores, *send = &scores[ArraySize(scores)]; s < send; ++s)
	if (!s->name[0] || s->name[sizeof(s->name)-1])
	    memset (s, 0, sizeof(*s));
    // Resort to account for the above zeroing
    qsort (ArrayBlock(scores), sizeof(scores[0]), compare_scores);

    // Add this game's score, if it is high enough
    struct Score* lowscore = &scores[ArraySize(scores)-1];
    const char* username = player_name();
    if (total_score (&lowscore->score) < total_score (&game_score)) {
	lowscore->score = game_score;
	snprintf (ArrayBlock (lowscore->name), "%s", username);
	// Resort the new score
	qsort (ArrayBlock(scores), sizeof(scores[0]), compare_scores);
	// And write the score file
	write_score_file (BATTLESTAR_SCOREFILE, SCOREFILE_MAGIC, scores, sizeof(scores));
    }

    // List top scores
    if (!scores[0].name[0])
	return;
    puts ("\n-#--Name-----Rating-----------Sco--Ple-Pow-Ego");
    for (unsigned i = 0; i < ArraySize(scores) && scores[i].name[0]; ++i) {
	if (game_score.pleasure == scores[i].score.pleasure
		&& game_score.power == scores[i].score.power
		&& game_score.ego == scores[i].score.ego
		&& 0 == strcmp (username, scores[i].name))
	    printf (BOLD_ON);
	printf ("%2u: %-8s %-16s %3u  %3u %3u %3u\n" BOLD_OFF, i+1, scores[i].name, player_rating (&scores[i].score), total_score (&scores[i].score), scores[i].score.pleasure, scores[i].score.power, scores[i].score.ego);
    }
}

//}}}-------------------------------------------------------------------
//{{{ Crash handler

// This will be set to on_fatal_signal in common/ui.c,
// which is what initialize_curses installs.
static void (*s_old_handler)(int) = NULL;

static void on_crash (int sig)
{
    static volatile sig_atomic_t s_once = 0;
    if (!s_once) {
        s_once = 1;
	save();
    }
    s_old_handler (sig);
}

static void install_crash_handler (void)
{
    // Install crashing signal handler to save the game
    static const uint8_t c_crash_sig[] = { SIGILL, SIGBUS, SIGABRT, SIGFPE, SIGSYS, SIGSEGV, SIGHUP };
    for (unsigned i = 0; i < ArraySize(c_crash_sig); ++i) {
        struct sigaction previous;
        if (sigaction (c_crash_sig[i], NULL, &previous) < 0) {
            perror ("read signal handler");
            exit (EXIT_FAILURE);
        }
        s_old_handler = previous.sa_handler;
        install_signal_handler (c_crash_sig[i], on_crash);
    }
}

//}}}-------------------------------------------------------------------
