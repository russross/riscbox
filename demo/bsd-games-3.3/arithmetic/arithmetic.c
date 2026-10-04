// This file is free software, distributed under the BSD license.
//
// By Eamonn McManus, Trinity College Dublin <emcmanus@cs.tcd.ie>.
//
// The operation of this program mimics that of the standard Unix
// game 'arithmetic'. I've made it as close as I could manage
// without examining the source code.

#include "config.h"
#include <signal.h>

//----------------------------------------------------------------------

enum {
    MAXRANGE	= 10000,
    QUESTIONS_PER_ROUND	= 20
};
static const char c_defaultkeys[] = "+-";

static const char* _keys	= c_defaultkeys;
static unsigned _nkeys		= sizeof(c_defaultkeys)-1;
static unsigned _rangemax	= 10;
static unsigned _nright		= 0;
static unsigned _nwrong		= 0;
static time_t _qstart		= 0;

//----------------------------------------------------------------------

// Pick a problem and ask it
static unsigned print_problem (void)
{
    unsigned right = nrand (_rangemax)+1;
    unsigned n2 = nrand (_rangemax)+1;
    unsigned left = 0, result = 0;
    char op = _keys [nrand (_nkeys)];
    switch (op) {
	case '+': left = n2; result = left + right; break;
	case '-': result = n2; left = right + result; break;
	case 'x': left = n2; result = left * right; break;
	case '/': result = n2; left = right * result; break;
    }
    printf ("%d %c %d = ", left, op, right);
    fflush (stdout);
    return result;
}

// Print score. Original 'arithmetic' had a delay after printing it.
static void showstats (void)
{
    if (_nright + _nwrong <= 0)
	return;
    printf ("\nRights %u; Wrongs %u; Score %u%%\n", _nright, _nwrong, 100 * _nright / (_nright + _nwrong));
    if (_nright > 0) {
	time_t qtime = time(NULL) - _qstart;
	printf ("Total time %ld seconds; %.1f seconds per problem\n", qtime, (float) qtime / _nright);
    }
}

//----------------------------------------------------------------------

static void process_args (int argc, char* const* argv)
{
    for (int ch; (ch = getopt (argc, argv, "r:o:")) != -1;) {
	switch (ch) {
	    case 'o':
		_keys = optarg;
		_nkeys = 0;
		for (const char* p = _keys; *p; ++p, ++_nkeys) {
		    if (!strchr ("+-x/", *p)) {
			printf ("I'm sorry, I don't know how to '%c'.\n", *p);
			exit (EXIT_FAILURE);
		    }
		}
		break;
	    case 'r':
		_rangemax = atoi (optarg);
		if (!_rangemax)
		    puts ("So you don't want to play then?");
		else if (_rangemax < 10)
		    puts ("Try again when you're smarter.");
		else if (_rangemax > MAXRANGE)
		    puts ("For the hundredth time, Spock, I won't play with you.");
		else
		    break;
		exit (EXIT_FAILURE);
	    default:
		puts ("Usage: arithmetic [-o +-x/] [-r range]");
		exit (EXIT_SUCCESS);
	}
    }
}

// Handle interrupt character. Print score and exit.
static _Noreturn void intr (int sig UNUSED)
{
    showstats();
    exit (EXIT_SUCCESS);
}

//----------------------------------------------------------------------

// Select keys from +-x/ to be asked addition, subtraction, multiplication,
// and division problems. More than one key may be given. The default is
// +-. Specify a range to confine the operands to 0 - range. Default upper
// bound is 10. After every QUESTIONS_PER_ROUND questions, statistics on
// the performance so far are printed.
//
int main (int argc, char* const* argv)
{
    process_args (argc, argv);
    srandrand();
    signal (SIGINT, intr);
    _qstart = time (NULL);

    // Now ask the questions.
    for (;;) {
	unsigned result = print_problem();
	//
	// Keep looping until the correct answer is given,
	// or until EOF, 'q', or interrupt.
	//
	for (;;) {
	    char line [16];
	    if (!fgets (line, sizeof (line), stdin)) {
		printf ("\n");
		return EXIT_SUCCESS;
	    }
	    if (line[0] == 'q') {
		showstats();
		return EXIT_SUCCESS;
	    }
	    unsigned answer = atoi (line);
	    if (!answer)
		printf ("Please type a number, or 'q' to quit.\n");
	    else if (answer == result) {
		++_nright;
		printf (BOLD_ON "Right!" BOLD_OFF "\n");
		break;
	    } else {
		++_nwrong;
		printf (BOLD_ON "Wrong." BOLD_OFF "\n");
	    }
	}
	if (_nright >= QUESTIONS_PER_ROUND) {
	    showstats();
	    printf ("\nPress RETURN for next round, or 'q' to quit.\n");
	    int c = getchar();
	    if (c <= 0 || c == EOF || c == 'q')
		break;

	    // Restart statistics for the next round
	    _nright = 0;
	    _nwrong = 0;
	    _qstart = time (NULL);
	}
    }
    return EXIT_SUCCESS;
}
