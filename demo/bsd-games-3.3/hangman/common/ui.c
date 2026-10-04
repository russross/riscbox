// Copyright (c) 2016 by Mike Sharov <msharov@users.sourceforge.net>
// This file is free software, distributed under the BSD license.

#include "../config.h"
#include <signal.h>
#include <locale.h>

//----------------------------------------------------------------------

#if NCURSES_WIDECHAR
    static bool g_has_utf8 = false;
#endif

//----------------------------------------------------------------------

void cleanup_curses (void)
{
    if (!stdscr || isendwin())
	return;
    flushinp();
    endwin();
}

#define S(s)	(1<<(s))
enum {
    sigset_Quit = S(SIGINT)|S(SIGQUIT)|S(SIGTERM),
    exitcode_SignalBase = 128
};

// Block other handled signals while cleanup or saving is in progress.
void install_signal_handler (int sig, void (*handler)(int))
{
    struct sigaction action = { .sa_handler = handler };
    sigfillset (&action.sa_mask);
    if (sigaction (sig, &action, NULL) < 0) {
        perror ("install signal handler");
        exit (EXIT_FAILURE);
    }
}

static _Noreturn void on_fatal_signal (int sig)
{
    static volatile sig_atomic_t s_bOnce = 0;
    int exitcode = exitcode_SignalBase+sig;
    if (!s_bOnce) {
        s_bOnce = 1;
	cleanup_curses();
	if (S(sig) & sigset_Quit)
	    exitcode = EXIT_SUCCESS;	// terminated by user
	else
	    psignal (sig, "Fatal error");
	exit (exitcode);
    }
    _Exit (exitcode);
}
#undef S

void initialize_curses (void)
{
    // Install signal handlers to ensure cleanup
    static const uint8_t c_FatalSignals[] = {
	SIGINT, SIGQUIT, SIGTERM, SIGILL, SIGBUS,
	SIGABRT, SIGFPE, SIGSYS, SIGSEGV, SIGHUP
    };
    for (unsigned i = 0; i < ArraySize(c_FatalSignals); ++i)
	install_signal_handler (c_FatalSignals[i], on_fatal_signal);

    // Setup locale and check whether it has UTF8 output support
    #if NCURSES_WIDECHAR
	setlocale (LC_ALL, "");
	const char* lang = getenv ("LANG");
	if (lang && strstr (lang, "utf8"))
	    g_has_utf8 = true;
    #endif

    // Setup the screen for UI
    if (!initscr()) {
	puts ("Error: unable to initialize terminal graphics");
	exit (EXIT_FAILURE);
    }
    atexit (cleanup_curses);
    start_color();
    use_default_colors();
    noecho();
    cbreak();
    curs_set (0);
    keypad (stdscr, true);

    signal (SIGTSTP, SIG_IGN);	// Disable Ctrl+z
    srandrand();		// Set random seed
}

void init_pairs (const struct color_pair* cps, size_t ncps)
{
    for (unsigned i = 0; i < ncps; ++i)
	init_pair (i+1, cps[i].fg, cps[i].bg);
}

void erase_window (WINDOW* w, short color)
{
    werase (w);
    wattr_set (w, 0, color, NULL);
    for (int y = 0; y < getmaxy(w); ++y)
	mvwhline (w, y, 0, ' ', getmaxx(w));
}

void mvwadd_wchw (WINDOW* w, int l, int c, wchar_t wch, attr_t a, short color)
{
    #if NCURSES_WIDECHAR
	wchar_t wchzs[2] = { wch, 0 };
	cchar_t ch = {};
	setcchar (&ch, wchzs, a, color, NULL);
	mvwadd_wch (w, l, c, &ch);
    #else
	wmove (w, l, c);
	wattr_set (w, a, color, NULL);
	waddch (w, (char) wch);
    #endif
}

wchar_t get_card_suit_char (enum CardSuit suit)
{
    static const char c_suitc[4] = {'S','D','C','H'};
    #if NCURSES_WIDECHAR
	static const uint16_t c_suitw[4] = { 0x2660, 0x2666, 0x2663, 0x2665 };
	return g_has_utf8 ? c_suitw[suit] : (wchar_t) c_suitc[suit];
    #else
	return (wchar_t) c_suitc[suit];
    #endif
}
