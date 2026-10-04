// Copyright (c) 1987 by Ed James <edjames@berkeley.edu>
// This file is free software, distributed under the BSD license.

#include "atc.h"
#include <getopt.h>

//----------------------------------------------------------------------
// Global variables

unsigned _clck = 0, _safe_planes = 0, _start_time = 0;

const struct Scenario* _sp = NULL;

VECTOR (struct PlaneList, _air);
VECTOR (struct PlaneList, _ground);

const struct Displacement _displacement[MAXDIR] = {
    {0, -1},
    {1, -1},
    {1, 0},
    {1, 1},
    {0, 1},
    {-1, 1},
    {-1, 0},
    {-1, -1}
};

//----------------------------------------------------------------------

int main (int argc, char* const argv[])
{
    bool f_usage = false, f_list = false, f_showscore = false;
    for (int ch; (ch = getopt (argc, argv, "ls")) != -1;) {
	switch (ch) {
	    default:	f_usage = true; break;
	    case 'l':	f_list = true;	break;
	    case 's':	f_showscore = true; break;
	}
    }
    const char* scenarioname = default_scenario();
    if (optind < argc) {
	scenarioname = okay_scenario (argv[optind]);
	if (!scenarioname) {
	    printf ("'%s' is not available. ", argv[optind]);
	    f_list = true;
	}
    }
    if (f_usage)
	puts ("Usage: atc -[ls?] [scenario_name]");
    if (f_list)
	list_scenarios();
    if (f_showscore)
	save_score (false);
    if (f_usage | f_showscore | f_list)
	return EXIT_SUCCESS;
    if (!scenarioname || load_scenario (scenarioname) < 0)
	return EXIT_FAILURE;

    _start_time = time(NULL);

    init_gr();
    addplane();

    for (;;)
	getcommand();
}
