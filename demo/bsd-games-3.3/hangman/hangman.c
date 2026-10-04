// Copyright (c) 1983 The Regents of the University of California.
// Copyright (c) 1983 Ken Arnold
// This file is free software, distributed under the BSD license.

#include "config.h"
#include <sys/stat.h>

//----------------------------------------------------------------------

enum {
    NLETTERS	= 'Z'-'A'+1,
    PW_LINES	= 2,
    PW_COLS	= 60,
    GW_LINES	= 10,
    GW_COLS	= PW_COLS,
    STATS_COL	= GW_COLS - 23,
    MINLEN	= 6,
    MAXLEN	= 16,
    MAXERRS	= 7
};
enum {
    color_None,
    color_UnguessedLetter,
    color_WrongLetter,
    color_RightLetter
};
enum {
    letter_Unguessed,
    letter_Wrong,
    letter_Right
};

//----------------------------------------------------------------------

static uint8_t _guessed [NLETTERS] = { letter_Unguessed };
static char _word [MAXLEN] = "";
static char _known [MAXLEN] = "";
static char _lastWord [MAXLEN] = "";
static unsigned _errors = 0;
static unsigned _wordnum = 0;
static unsigned _minlen = MINLEN;
static float _average = 0;
static const char* _dict_name = _PATH_WORDLIST;
static WINDOW* _wgame = NULL;
static WINDOW* _wprompt = NULL;

//----------------------------------------------------------------------

static void endgame(void);
static void process_guess (char c);
static void getword(void);
static void draw (void);

static inline float current_average (void)
    { return (_average * (_wordnum - 1) + _errors) / _wordnum; }

//----------------------------------------------------------------------

int main (int argc, char* const argv[])
{
    for (int ch; (ch = getopt (argc, argv, "d:m:")) != -1;) {
	if (ch == 'd')
	    _dict_name = optarg;
	else if (ch == 'm') {
	    _minlen = atoi(optarg);
	    if (_minlen < 2) {
		printf ("Error: minimum word length too short\n");
		return EXIT_FAILURE;
	    }
	} else {
	    printf ("Usage: hangman [-d wordlist] [-m minlen]\n");
	    return EXIT_FAILURE;
	}
    }
    initialize_curses();
    _wprompt = newwin (PW_LINES, PW_COLS, LINES-PW_LINES, 0);
    keypad (_wprompt, true);
    _wgame = newwin (GW_LINES, GW_COLS, LINES-PW_LINES-GW_LINES, 0);
    static const struct color_pair c_Pairs[] = {
	{ COLOR_BLUE,	COLOR_BLACK	},	// color_UnguessedLetter
	{ COLOR_RED,	COLOR_BLACK	},	// color_WrongLetter
	{ COLOR_GREEN,	COLOR_BLACK	}	// color_RightLetter
    };
    init_pairs (ArrayBlock (c_Pairs));
    wbkgdset (_wgame, COLOR_PAIR(color_RightLetter));
    for (;;) {
	getword();
	++_wordnum;
	_errors = 0;
	while (_errors < MAXERRS && strchr(_known, '-')) {
	    draw();
	    int c = wgetch (_wprompt);
	    if (c == ' ')
		break;
	    if (c == '\n')
		return EXIT_SUCCESS;
	    if (c >= 'A' && c <= 'Z')
		c += 'a'-'A';
	    if (c >= 'a' && c <= 'z')
		process_guess(c);
	}
	endgame();
    }
    return EXIT_SUCCESS;
}

static void draw (void)
{
    werase (_wgame);
    // Draw gallows
    static const char c_NoosePic[][12] = {
	"  ______   ",
	"  |    |   ",
	"  |        ",
	"  |        ",
	"  |        ",
	"  |        ",
	"__|_____   ",
	"|      |___",
	"|_________|",
    };
    for (unsigned i = 0; i < ArraySize(c_NoosePic); ++i)
	mvwaddstr (_wgame, i, 3, c_NoosePic[i]);

    // Draw the hanging man with as many parts as errors
    static const struct {
	uint8_t	y, x;
	char	c;
    } c_ManParts [MAXERRS] = {
	{2, 10, 'O'},
	{3, 10, '|'},
	{4, 10, '|'},
	{5, 9, '/'},
	{3, 9, '/'},
	{3, 11, '\\'},
	{5, 11, '\\'}
    };
    for (unsigned i = 0; i < ArraySize(c_ManParts); ++i)
	mvwaddch (_wgame, c_ManParts[i].y, c_ManParts[i].x, i < _errors ? c_ManParts[i].c : ' ');

    mvwprintw (_wgame, 0, STATS_COL, "Word #: %u", _wordnum);
    mvwprintw (_wgame, 1, STATS_COL, "Current Average: %0.3f", current_average());
    mvwprintw (_wgame, 2, STATS_COL, "Overall Average: %0.3f", _average);
    mvwaddstr (_wgame, 4, STATS_COL, "Letter: guess");
    mvwaddstr (_wgame, 5, STATS_COL, "Space:  next word");
    mvwaddstr (_wgame, 6, STATS_COL, "Enter:  quit");

    werase (_wprompt);
    mvwprintw (_wprompt, 0, 1, " Word: %s", _known);
    if (_lastWord[0])
	mvwprintw (_wprompt, 0, STATS_COL, "Last: %s", _lastWord);
    mvwaddstr (_wprompt, 1, 1, "Guess: ");
    for (unsigned i = 0; i < ArraySize(_guessed); ++i)
	waddch (_wprompt, COLOR_PAIR(color_UnguessedLetter+_guessed[i])|('a'+i));

    wrefresh (_wgame);
}

static void process_guess (char c)
{
    bool correct = false;
    for (unsigned i = 0; _word[i]; ++i) {
	if (_word[i] == c) {
	    _known[i] = c;
	    correct = true;
	}
    }
    unsigned l = c - 'a';
    if (_guessed[l] == letter_Unguessed) {
	_errors += !correct;
	_guessed[l] = letter_Wrong + correct;
    }
    // Flash the typed letter
    draw();
    mvwaddch (_wprompt, 1, sizeof("Guess: ")+l, A_REVERSE| COLOR_PAIR(color_UnguessedLetter+_guessed[l])|(c));
    wrefresh (_wprompt);
    usleep (250000);
    flushinp();
}

// endgame: do what's necessary at the end of the game
static void endgame (void)
{
    draw();
    wmove (_wprompt, 1, 1);
    if (strchr (_known, '-')) {
	wattr_set (_wprompt, A_BOLD, color_WrongLetter, NULL);
	wprintw (_wprompt, "Sorry, the word was \"%s\"\n", _word);
    } else {
	wattr_set (_wprompt, A_BOLD, color_RightLetter, NULL);
	wprintw (_wprompt, "You got it!\n");
    }
    wattr_set (_wprompt, A_NORMAL, color_None, NULL);
    wclrtoeol (_wprompt);
    wrefresh (_wprompt);
    sleep (1);
    flushinp();
    memset (_guessed, letter_Unguessed, sizeof(_guessed));
    _average = current_average();
    strncpy (_lastWord, _word, sizeof(_lastWord));
}

static bool read_word_is_valid (void)
{
    size_t wl = strlen(_word)-1;
    if (wl < _minlen || _word[wl] != '\n')
	return false;
    for (size_t i = 0; i < wl; ++i)
	if (_word[i] < 'a' || _word[i] > 'z')
	    return false;
    _word[wl] = '\0';
    return true;
}

// Get a valid word out of the dictionary file
static void getword (void)
{
    static FILE* _dictfp = NULL;
    static off_t _dict_size = 0;
    if (!_dictfp) {
	if (!(_dictfp = fopen (_dict_name, "r"))) {
	    cleanup_curses();
	    printf ("Error: unable to open dictionary file %s\n", _dict_name);
	    exit (EXIT_FAILURE);
	}
	static struct stat sbuf;
	fstat (fileno(_dictfp), &sbuf);
	_dict_size = sbuf.st_size;
    }
    do {
	fseek (_dictfp, nrand (_dict_size), SEEK_SET);
	if (!fgets (_word, ArraySize(_word), _dictfp))
	    continue;	// In the middle of some word, skip to end of line
	if (_word[strlen(_word)-1] != '\n')
	    continue;	// _word too long, try another location
	if (!fgets (_word, ArraySize(_word), _dictfp))
	    continue;	// The next word will be read from the beginning
    } while (!read_word_is_valid());
    memset (_known, '-', strlen(_word));
    _known[strlen(_word)] = 0;
}
