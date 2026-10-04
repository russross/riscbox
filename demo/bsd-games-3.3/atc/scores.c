// Copyright (c) 1987 by Ed James <edjames@berkeley.edu>
// This file is free software, distributed under the BSD license.

#include "atc.h"

//----------------------------------------------------------------------

#define	_PATH_SCOREFILE	_PATH_GAME_STATE "atc.scores"
#define SCOREFILE_MAGIC	"atc\0\0"

enum {
    MAXSCORES	= 18,
    SECAMIN	= 60,
    MINAHOUR	= 60,
    HOURADAY	= 24,
    SECAHOUR	= (SECAMIN * MINAHOUR),
    SECADAY	= (SECAHOUR * HOURADAY)
};

struct Score {
    char	name [12];
    char	host [12];
    char	game [12];
    uint32_t	planes;
    uint32_t	time;
    uint32_t	real_time;
};

//----------------------------------------------------------------------

static unsigned DAY (unsigned s)	{ return s / SECADAY; }
static unsigned HOUR (unsigned s)	{ return (s % SECADAY) / SECAHOUR; }
static unsigned MIN (unsigned s)	{ return (s % SECAHOUR) / SECAMIN; }
static unsigned SEC (unsigned s)	{ return s % SECAMIN; }

static const char* timestr(int t)
{
    static char s [32];
    if (DAY(t) > 0)
	snprintf(ArrayBlock(s), "%dd+%02dhrs", DAY(t), HOUR(t));
    else if (HOUR(t) > 0)
	snprintf(ArrayBlock(s), "%d:%02d:%02d", HOUR(t), MIN(t), SEC(t));
    else if (MIN(t) > 0)
	snprintf(ArrayBlock(s), "%d:%02d", MIN(t), SEC(t));
    else if (SEC(t) > 0)
	snprintf(ArrayBlock(s), ":%02d", SEC(t));
    else
	*s = '\0';
    return s;
}

static void print_scores (const struct Score* score)
{
    printf ("%2s:  %-8s  %-8s  %-18s  %4s  %9s  %4s\n", "#", "name", "host", "game", "time", "real time", "planes safe");
    puts ("-------------------------------------------------------------------------------");
    for (unsigned i = 0; i < MAXSCORES && score->name[0]; ++i, ++score)
	printf ("%2d:  %-8s  %-8s  %-18s  %4d  %9s  %4d\n", i+1, score->name, score->host, score->game, score->time, timestr(score->real_time), score->planes);
}

static int compare_scores (const void *va, const void *vb)
{
    const struct Score* a = (const struct Score*) va;
    const struct Score* b = (const struct Score*) vb;
    return b->planes == a->planes ? b->time - a->time : b->planes - a->planes;
}

static bool score_valid (const struct Score* score)
{
    if (!score->name[0] || score->name[sizeof(score->name)-1]
	|| !score->host[0] || score->host[sizeof(score->host)-1]
	|| !score->game[0] || score->game[sizeof(score->game)-1]
	|| strchr (score->host, '.')
	|| score->planes > score->time
	|| score->real_time > SECADAY*7)
	return false;
    return true;
}

void save_score (bool reallySave)
{
    struct Score scores [MAXSCORES] = {{}};
    if (!read_score_file (_PATH_SCOREFILE, SCOREFILE_MAGIC, scores, sizeof(scores)))
	memset (scores, 0, sizeof(scores));
    // Check each score and zero if invalid
    for (unsigned i = 0; i < ArraySize(scores); ++i)
	if (!score_valid (&scores[i]))
	    memset (&scores[i], 0, sizeof(scores[i]));
    // Resort to account for the above zeroing
    qsort (scores, ArraySize(scores), sizeof(scores[0]), compare_scores);

    // Check if a new high score has been reached
    struct Score* lowscore = &scores[ArraySize(scores)-1];
    if (reallySave && (lowscore->planes < _safe_planes || (lowscore->planes == _safe_planes && lowscore->time < _clck))) {
	// Write the new high score
	memset (lowscore, 0, sizeof(*lowscore));
	snprintf (ArrayBlock(lowscore->name), "%s", player_name());
	gethostname (lowscore->host, sizeof(lowscore->host)-1);
	char* dot = strchr (lowscore->host, '.');
	if (dot)	// Record base hostname only
	    memset (dot, 0, sizeof(lowscore->host)-(dot-lowscore->host));
	strncpy (lowscore->game, _sp->name, sizeof(lowscore->game)-1);
	lowscore->planes = _safe_planes;
	lowscore->time = _clck;
	lowscore->real_time = time(NULL) - _start_time;
	if (!score_valid (lowscore))
	    return;	// Should be valid here, but check anyway
	// Resort to put it where it belongs
	qsort (scores, ArraySize(scores), sizeof(scores[0]), compare_scores);
	// And write it
	write_score_file (_PATH_SCOREFILE, SCOREFILE_MAGIC, scores, sizeof(scores));
    }
    print_scores (scores);
}
