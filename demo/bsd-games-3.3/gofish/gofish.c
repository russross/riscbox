// This file is free software, distributed under the BSD license.

#include "config.h"
#include <sys/wait.h>

//----------------------------------------------------------------------

enum {
    RANKS	= 13,
    SUITS	= 4,
    HANDSIZE	= 7,
    NCARDS	= RANKS * SUITS
};
enum EPlayer {
    USER,
    COMPUTER,
    NPLAYERS
};
static enum EPlayer OTHER (enum EPlayer p) { return 1-p; }

enum {
    PANEL_LINES = 4,
    PANEL_COLS = 80
};
enum {
    color_None,
    color_Panel,
    color_CardBlack,
    color_CardRed,
};

//----------------------------------------------------------------------

static const char c_CardNames [RANKS][3] =
    { "A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K" };
static const char c_PlayerName [NPLAYERS][4] = {"You", "I"};

static uint8_t	_hand [NPLAYERS][RANKS] = {{0},{0}};
static uint8_t	_deck [NCARDS] = {0};
static uint8_t	_decksz = NCARDS;
static bool	_asked [NPLAYERS][RANKS] = {{false},{false}};

static WINDOW*	_wmsg = NULL;
static WINDOW*	_wpanel = NULL;

//----------------------------------------------------------------------

static void shuffle_deck_and_deal (void);
static void initialize_windows (void);
static unsigned parse_card_name (char k);
static unsigned makemove (enum EPlayer player);
static unsigned usermove (void);
static unsigned compmove (void);
static unsigned drawcard (enum EPlayer player);
static bool gofish (enum EPlayer player, unsigned askedfor);
static void goodmove (enum EPlayer player, unsigned m);
static void chkwinner (enum EPlayer player);
static void printplayer (enum EPlayer player);
static unsigned countbooks (enum EPlayer player);
static void instructions (void);

//----------------------------------------------------------------------

int main (void)
{
    initialize_curses();
    initialize_windows();
    shuffle_deck_and_deal();
    instructions();

    enum EPlayer player = nrand(NPLAYERS);
    printplayer (player);
    waddstr (_wmsg, "get to start.\n");
    for (;;) {
	unsigned m = makemove (player);
	if (_hand[OTHER(player)][m])
	    goodmove (player, m);
	else if (!gofish (player, m))
	    player = OTHER(player);
    }
    return EXIT_SUCCESS;
}

static void initialize_windows (void)
{
    if (_wmsg)
	delwin (_wmsg);
    else {
	static const struct color_pair c_Pairs[] = {
	    { COLOR_WHITE,	COLOR_BLUE	},	// color_Panel
	    { COLOR_BLACK,	COLOR_WHITE	},	// color_CardBlack
	    { COLOR_RED,	COLOR_WHITE	}	// color_CardRed
	};
	init_pairs (ArrayBlock (c_Pairs));
    }
    _wmsg = newwin (LINES-PANEL_LINES, PANEL_COLS, 0, 0);
    scrollok (_wmsg, true);
    idlok (_wmsg, true);
    keypad (_wmsg, true);
    wmove (_wmsg, getmaxy(_wmsg)-1, 0);
    if (_wpanel)
	delwin (_wpanel);
    _wpanel = newwin (PANEL_LINES, PANEL_COLS, LINES-PANEL_LINES, 0);
    wbkgdset (_wpanel, COLOR_PAIR(color_Panel));
}

static void shuffle_deck_and_deal (void)
{
    // Shuffle deck
    for (unsigned i = 0; i < NCARDS; ++i)
	_deck[i] = i % RANKS;
    random_shuffle_u8 (_deck, NCARDS);
    // Deal HANDSIZE cards to both players
    for (unsigned i = 0; i < HANDSIZE; ++i) {
	++_hand[USER][_deck[--_decksz]];
	++_hand[COMPUTER][_deck[--_decksz]];
    }
}

static unsigned parse_card_name (char k)
{
    if (k >= 'a' && k <= 'z')
	k -= 'a'-'A';
    for (unsigned i = 0; i < ArraySize(c_CardNames); ++i)
	if (k == c_CardNames[i][0])
	    return i;
    return ArraySize(c_CardNames);
}

static unsigned makemove (enum EPlayer player)
{
    if (player == USER)
	return usermove();
    else {
	unsigned m = compmove();
	_asked[COMPUTER][m] = true;
	wattr_on (_wmsg, A_BOLD, NULL);
	wprintw (_wmsg, "I ask you for %s. ", c_CardNames[m]);
	wattr_off (_wmsg, A_BOLD, NULL);
	return m;
    }
}

static void draw_panel (void)
{
    werase (_wpanel);

    // Draw player cards and books
    mvwaddstr (_wpanel, 0, 0, "Cards: ");
    unsigned cx = strlen("Cards: "), bx = cx, nBooks = 0;
    for (unsigned i = 0; i < RANKS; ++i) {
	if (!_hand[USER][i])
	    continue;
	if (_hand[USER][i] < SUITS) {
	    for (unsigned y = 0; y < _hand[USER][i]; ++y) {
		wcolor_set (_wpanel, color_CardBlack+(y%2), NULL);
		mvwaddstr (_wpanel, y, cx, c_CardNames[i]);
	    }
	    cx += strlen(c_CardNames[i])+1;
	} else {
	    ++nBooks;
	    wcolor_set (_wpanel, color_CardBlack, NULL);
	    mvwaddstr (_wpanel, 3, bx, c_CardNames[i]);
	    bx += strlen(c_CardNames[i])+1;
	}
    }
    wattr_set (_wpanel, A_NORMAL, color_None, NULL);
    if (nBooks)
	mvwaddstr (_wpanel, 3, 0, "Books: ");

    // Draw computer cards and books
    mvwprintw (_wpanel, 0, getmaxx(_wpanel)-18, "%2u cards in deck", _decksz);
    unsigned compCards = 0, compBooks = 0;
    for (unsigned i = 0; i < RANKS; ++i) {
	if (!_hand[COMPUTER][i])		continue;
	else if (_hand[COMPUTER][i] < SUITS)	++compCards;
	else					++compBooks;
    }
    mvwprintw (_wpanel, 1, getmaxx(_wpanel)-16, "I have %u cards", compCards);
    if (compBooks)
	mvwprintw (_wpanel, 2, getmaxx(_wpanel)-16, "I have %u books", compBooks);

    wrefresh (_wpanel);
}

static unsigned usermove (void)
{
    for (;;) {
	draw_panel();
	int k = wgetch (_wmsg);
	unsigned n = parse_card_name (k);

	if (k == 'x' || k == KEY_F(10))
	    exit (EXIT_SUCCESS);
	else if (k == '?' || k == KEY_F(1))
	    instructions();
	else if (k == KEY_RESIZE)
	    initialize_windows();
	else if (n >= ArraySize(c_CardNames))
	    waddstr (_wmsg, "I don't understand!\n");
	else if (_hand[USER][n] < 1)
	    wprintw (_wmsg, "You don't have any %s's!\n", c_CardNames[n]);
	else if (_hand[USER][n] >= SUITS)
	    wprintw (_wmsg, "You have already completed a book of %s's.\n", c_CardNames[n]);
	else {
	    wprintw (_wmsg, "\nYou ask me for %s. ", c_CardNames[n]);
	    return n;
	}
    }
}

static unsigned compmove (void)
{
    for (unsigned i = 0; i < RANKS; ++i) {
	if (_asked[USER][i] && _hand[COMPUTER][i] > 0 && _hand[COMPUTER][i] < SUITS) {
	    _asked[USER][i] = false;
	    return i;
	}
    }
    if (!nrand(3)) {
	unsigned i, max;
	for (i = 0;; ++i) {
	    if (_hand[COMPUTER][i] && _hand[COMPUTER][i] != SUITS) {
		max = i;
		break;
	    }
	}
	while (++i < RANKS)
	    if (_hand[COMPUTER][i] != SUITS && _hand[COMPUTER][i] > _hand[COMPUTER][max])
		max = i;
	return max;
    }
    for (;;) {
	for (unsigned i = 0; i < RANKS; ++i)
	    if (_hand[COMPUTER][i] && _hand[COMPUTER][i] != SUITS && !_asked[COMPUTER][i])
		return i;
	for (unsigned i = 0; i < RANKS; ++i)
	    _asked[COMPUTER][i] = false;
    }
}

static unsigned drawcard (enum EPlayer player)
{
    unsigned card = _deck[--_decksz];
    ++_hand[player][card];
    if (player == USER || _hand[player][card] == SUITS) {
	printplayer(player);
	wprintw (_wmsg, "drew %s", c_CardNames[card]);
	if (_hand[player][card] == SUITS) {
	    wprintw (_wmsg, " and made a book of %s's.\n", c_CardNames[card]);
	    chkwinner (player);
	} else
	    waddstr (_wmsg, ".\n");
    }
    return card;
}

static bool gofish (enum EPlayer player, unsigned askedfor)
{
    wattr_on (_wmsg, A_BOLD, NULL);
    printplayer (OTHER(player));
    waddstr (_wmsg, "say \"GO FISH!\"\n");
    wattr_off (_wmsg, A_BOLD, NULL);
    if (askedfor == drawcard (player)) {
	printplayer (player);
	waddstr (_wmsg, "drew the guess and get to ask again.\n");
	return true;
    }
    return false;
}

static void goodmove (enum EPlayer player, unsigned m)
{
    printplayer (OTHER(player));
    wprintw (_wmsg, "have %u %s%s.\n", _hand[OTHER(player)][m], c_CardNames[m], _hand[OTHER(player)][m] == 1 ? "" : "'s");

    _hand[player][m] += _hand[OTHER(player)][m];
    _hand[OTHER(player)][m] = 0;

    if (_hand[player][m] == SUITS) {
	printplayer (player);
	wprintw (_wmsg, "made a book of %s's!\n", c_CardNames[m]);
	chkwinner (player);
    }
    chkwinner (OTHER(player));

    printplayer (player);
    waddstr (_wmsg, "get another guess!\n");
}

static void chkwinner (enum EPlayer player)
{
    for (unsigned i = 0; i < RANKS; ++i)
	if (_hand[player][i] > 0 && _hand[player][i] < SUITS)
	    return;
    draw_panel();
    printplayer (player);
    waddstr (_wmsg, "don't have any more cards!\n\nMy books:");
    unsigned cb = countbooks (COMPUTER);
    waddstr (_wmsg, "Your books:");
    unsigned ub = countbooks (USER);
    wprintw (_wmsg, "\nI have %d, you have %d.\n", cb, ub);
    if (ub > cb) {
	waddstr (_wmsg, "\nYou win!!!\n");
	if (!nrand(64))
	    waddstr (_wmsg, "Cheater, cheater, pumpkin eater!\n");
    } else if (cb > ub) {
	waddstr (_wmsg, "\nI win!!!\n");
	if (!nrand(64))
	    waddstr (_wmsg, "Hah! Stupid peasant!\n");
    } else
	waddstr (_wmsg, "\nTie!\n");
    wgetch (_wmsg);
    exit (EXIT_SUCCESS);
}

static void printplayer (enum EPlayer player)
{
    wprintw (_wmsg, "%s ", c_PlayerName[player]);
}

static unsigned countbooks (enum EPlayer player)
{
    unsigned count = 0;
    for (unsigned i = 0; i < RANKS; ++i) {
	if (_hand[player][i] == SUITS) {
	    ++count;
	    wprintw (_wmsg, " %s", c_CardNames[i]);
	}
    }
    if (!count)
	waddstr (_wmsg, " none");
    waddch (_wmsg, '\n');
    return count;
}

static void instructions (void)
{
    waddstr (_wmsg,
	"\nThis is the traditional children's card game \"Go Fish\". We each get\n"
	"seven cards, and the rest of the deck is kept to be drawn from later. The\n"
	"object of the game is to collect \"books\", or all of the cards of a single\n"
	"value. For example, getting four 2's would give you a \"book of 2's\".\n"
	"\n"
	"We take turns asking each other for cards, but you can only ask me for\n"
	"a card value you already have. If I have any, I must give them to you,\n"
	"and you can keep asking. If you ask me for a card value I don't have,\n"
	"then I'll tell you to \"Go Fish!\", which means that you draw a card from\n"
	"the deck. If you draw the card you asked me for, you get to keep asking\n"
	"me for cards. If not, then it's my turn. The game stops when one of us\n"
	"runs out of cards, and the winner is the one with the most books.\n"
	"\n"
	"You ask for a card by typing its name (a234567891jqk), type x to quit,\n\n"
    );
}
