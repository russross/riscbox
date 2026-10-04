// This file is free software, distributed under the BSD license.

#include "config.h"

enum {
    color_None,
    color_Text,
    color_Field,
    color_Grass,
    color_Worm
};
enum {
    HEAD	= A_BOLD|COLOR_PAIR(color_Worm)|'@',
    BODY	= COLOR_PAIR(color_Worm)|'o',
    GRASS	= COLOR_PAIR(color_Grass)|'"',
    LENGTH	= 7,
    RUNLEN	= 8,
    XTIMER	= 120,
    YTIMER	= XTIMER*5/3,
    STOPTIMER	= -1
};

static WINDOW* tv = NULL;
static WINDOW* stw = NULL;

static struct body {
    unsigned x;
    unsigned y;
    struct body *prev;
    struct body *next;
} *head, *tail, goody;

static unsigned growing = 0;
static unsigned score = 0;
static unsigned start_len = LENGTH;
static unsigned visible_len = 0;

static void crash (void);
static unsigned max_length (void);
static void display (const struct body *, int);
static bool find_empty_spot (unsigned* y, unsigned* x);
static void life (void);
static void play (void);
static void prize (void);
static void newgrass (void);

int main (int argc, char **argv)
{
    initialize_curses();

    // Initialize starting parameters; length if given
    if (argc == 2)
	start_len = atoi(argv[1]);
    if (!start_len || start_len > max_length())
	start_len = LENGTH;

    static const struct color_pair c_Pairs[] = {
	{ COLOR_DEFAULT,COLOR_DEFAULT	},	// color_Text
	{ COLOR_YELLOW,	COLOR_BLACK	},	// color_Field
	{ COLOR_BLUE,	COLOR_BLACK	},	// color_Grass
	{ COLOR_GREEN,	COLOR_BLACK	}	// color_Worm
    };
    init_pairs (ArrayBlock (c_Pairs));
    stw = newwin (1, COLS - 1, 0, 0);
    wbkgdset (stw, COLOR_PAIR(color_Text));
    tv = newwin (LINES - 1, COLS - 1, 1, 0);
    wbkgdset (tv, COLOR_PAIR(color_Field));
    box (tv, 0, 0);
    mvwprintw (stw, 0, 0, " Worm");
    refresh();
    for (unsigned i = 0; i < max_length()/2; ++i)
	newgrass();
    wrefresh(stw);
    wrefresh(tv);
    life();		       // Create the worm
    prize();		       // Put up a goal
    play();
    cleanup_curses();
    printf("Your final score before quitting was %u\n", score);
    return EXIT_SUCCESS;
}

static struct body* newlink (void)
{
    struct body* l = (struct body*) malloc (sizeof (struct body));
    if (!l) {
	perror ("malloc");
	exit (EXIT_FAILURE);
    }
    return l;
}

static void life (void)
{
    head = newlink();
    head->x = start_len % (getmaxx(tv) - 5) + 2;
    head->y = getmaxy(tv) / 2;
    head->next = NULL;
    display(head, HEAD);
    struct body* bp = head;
    struct body* np = NULL;
    int j = 1;
    for (unsigned i = 0; i < start_len; ++i, bp = np) {
	np = newlink();
	np->next = bp;
	bp->prev = np;
	if ((bp->x <= 2 && j == 1) || (bp->x >= getmaxx(tv) - 4u && j == -1)) {
	    j *= -1;
	    np->x = bp->x;
	    np->y = bp->y + 1;
	} else {
	    np->x = bp->x - j;
	    np->y = bp->y;
	}
	display (np, BODY);
    }
    tail = np;
    tail->prev = NULL;
    visible_len = start_len + 1;
}

static void display(const struct body *pos, int chr)
{
    mvwaddch (tv, pos->y, pos->x, chr);
}

static bool find_empty_spot (unsigned* y, unsigned* x)
{
    unsigned n = 0;
    do {
	*y = nrand (getmaxy(tv) - 2) + 1;
	*x = nrand (getmaxx(tv) - 2) + 1;
	if (++n > 127)
	    return false;
    } while ((char) mvwinch (tv, *y, *x) != ' ');
    return true;
}

static void newgrass (void)
{
    unsigned y, x;
    if (find_empty_spot (&y, &x))
	waddch (tv, GRASS);
}

static void play (void)
{
    timeout (STOPTIMER);
    for (int lastch = 0;;) {
	int ch = getch();
	if (ch == ERR)
	    ch = lastch;
	unsigned x = head->x, y = head->y;
	switch (ch) {
	    case KEY_LEFT:
	    case 'h':	--x; timeout (XTIMER); break;
	    case KEY_RIGHT:
	    case 'l':	++x; timeout (XTIMER); break;
	    case KEY_UP:
	    case 'k':	--y; timeout (YTIMER); break;
	    case KEY_DOWN:
	    case 'j':	++y; timeout (YTIMER); break;
	    case 'q':	return;
	    default:	ch = lastch; break;
	}
	lastch = ch;
	if (x == head->x && y == head->y)
	    continue;
	if (growing == 0) {
	    display(tail, ' ');
	    tail->next->prev = NULL;
	    struct body* nh = tail->next;
	    free(tail);
	    tail = nh;
	    visible_len--;
	} else
	    growing--;
	display(head, BODY);
	char fc = (char) mvwinch(tv, y, x);
	if (isdigit(fc)) {
	    growing += fc - '0';
	    prize();
	    score += growing;
	} else if (fc == '"')
	    newgrass();
	else if (fc != ' ')
	    crash();
	struct body* nh = newlink();
	nh->next = NULL;
	nh->prev = head;
	head->next = nh;
	nh->y = y;
	nh->x = x;
	display(nh, HEAD);
	head = nh;
	++visible_len;
	wmove(tv, head->y, head->x);
	mvwprintw (stw, 0, getmaxx(stw)-36, "Length: %3u of %3u    Score: %3u", visible_len, max_length(), score);
	wrefresh (stw);
	wrefresh (tv);
    }
}

static unsigned max_length (void)
{
    return LINES*COLS/8u;
}

static void prize (void)
{
    if (visible_len == max_length() || !find_empty_spot (&goody.y, &goody.x)) {
	cleanup_curses();
	printf("\nYou won!\nYour final score was %u\n\n", score);
	exit (EXIT_SUCCESS);
    } else
	waddch (tv, A_BOLD| (nrand(9)+'1'));
    wrefresh (tv);
}

static void crash (void)
{
    cleanup_curses();
    printf("Well, you ran into something and the game is over.\n"
	    "Your final score was %u\n", score);
    exit (EXIT_SUCCESS);
}
