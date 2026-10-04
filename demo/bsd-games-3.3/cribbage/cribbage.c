// Copyright (c) 2016 by Mike Sharov <msharov@users.sourceforge.net>
// This file is free software, distributed under the BSD license.

#define _XOPEN_SOURCE_EXTENDED 1
#include "config.h"

//{{{ Constants and types ----------------------------------------------

enum {
    HUMAN,
    COMPUTER,
    NPLAYERS
};
enum {
    NCARDS	= NRANKS*NSUITS,
    PLAYHAND	= 4,
    MAXHAND	= PLAYHAND+NPLAYERS,
    WINSCORE	= 61,
    MAXGAMES	= 7
};
enum {
    BLINES	= 2,
    LINESCORE	= WINSCORE/BLINES,
    BLINE_LENGTH= LINESCORE+LINESCORE/5-1,
    WBOARD_LINES= 2+2+1+NPLAYERS*BLINES,
    WBOARD_COLS	= BLINE_LENGTH+4,
    WTABLE_LINES= 6,
    WTABLE_COLS	= WBOARD_COLS,
    WMSG_COLS	= WBOARD_COLS
};
enum {
    color_None,
    color_Board,
    color_HiddenCard,
    color_BlackSuit,
    color_RedSuit,
    color_SelectedBlackSuit,
    color_SelectedRedSuit
};

struct Hand {
    uint8_t	sz;
    card_t	c[PLAYHAND*2];
};

struct Player {
    struct Hand	hand;
    struct Hand	table;
    uint8_t	score;
    uint8_t	lastScore;
    uint8_t	gameswon;
};

//}}}-------------------------------------------------------------------
//{{{ Local variables

static WINDOW*	_wmsg	= NULL;
static WINDOW*	_wtable	= NULL;
static WINDOW*	_wboard	= NULL;

static struct Player	_p [NPLAYERS] = {{{0,{0}},{0,{0}},0,0,0}};
static struct Hand	_crib = {0,{0}};
static struct Hand	_play = {0,{0}};
static card_t		_starter = 0;
static uint8_t		_count = 0;
static uint8_t		_dealer = HUMAN;	// Crib owner
static uint8_t		_last_player = HUMAN;	// Who played last card
static uint8_t		_curcard = 0;

//}}}-------------------------------------------------------------------
//{{{ Prototypes

static enum CardRank card_rank (card_t c);
static enum CardSuit card_suit (card_t c);
static unsigned card_value (card_t c);
static unsigned n_permutations (unsigned handsz);
static void swap_cards (card_t* c1, card_t* c2);
static int compare_cards (const void* v1, const void* v2);
static void sort_hand (struct Hand* h);
static void move_card (struct Hand* from, unsigned ic, struct Hand* to);
static void deal_cards (void);
static void reverse_hand (card_t* hand, unsigned handsz);
static void next_hand_permutation (struct Hand* hand);
static struct Hand hand_with_starter (const struct Hand* hand);
static void record_score (unsigned who, unsigned pts);
static unsigned score_fifteens (const struct Hand* hand);
static unsigned score_pairs (const struct Hand* hand);
static unsigned score_runs (const struct Hand* hand);
static unsigned score_flush (const struct Hand* hand);
static unsigned score_nobs (const struct Hand* hand);
static unsigned score_hand (const struct Hand* h, bool isCrib, bool explain);
static unsigned score_play (card_t lastCard, bool printMessage);
static unsigned n_playable_cards (const struct Hand* h);
static void score_last_played_card (void);
static const char* player_pronoun (unsigned p);
static void draw_peg (uint8_t score, unsigned line);
static void draw_board (void);
static void draw_card (unsigned l, unsigned c, card_t v, bool selected);
static void draw_hidden_card (unsigned l, unsigned c);
static void draw_table (void);
static void print_msg (const char* msg, ...);
static void draw_screen (void);
static void create_windows (void);
static void move_windows (void);
static unsigned select_card (void);
static void computer_discard (void);
static unsigned computer_select_play (void);
static unsigned winning_player (void);
static bool run_play (void);

//}}}-------------------------------------------------------------------
//{{{ Card operations

static enum CardRank card_rank (card_t c)
    { return c/NSUITS; }
static enum CardSuit card_suit (card_t c)
    { return c%NSUITS; }
static unsigned card_value (card_t c)
    { return min_u (card_rank(c)+1, 10); }
static unsigned n_permutations (unsigned handsz)
    { return handsz <= 1 ? 1 : handsz*n_permutations(handsz-1); }

static inline void swap_cards (card_t* c1, card_t* c2)
    { swap_u8 (c1, c2); }

static int compare_cards (const void* v1, const void* v2)
{
    return sign (*(const card_t*)v1 - *(const card_t*)v2);
}

// Sort a hand of n cards
static void sort_hand (struct Hand* h)
{
    qsort (h->c, h->sz, sizeof(card_t), compare_cards);
}

static void move_card (struct Hand* from, unsigned ic, struct Hand* to)
{
    assert (to->sz < ArraySize(to->c) && ic < from->sz);
    to->c[to->sz++] = from->c[ic];
    memmove (&from->c[ic], &from->c[ic+1], (--from->sz-ic)*sizeof(card_t));
}

static void deal_cards (void)
{
    card_t deck [NCARDS] = {0};
    // Fill and shuffle the deck
    iota_u8 (ArrayBlock (deck));
    random_shuffle_u8 (ArrayBlock (deck));
    // Deal six to each player
    uint8_t decksz = NCARDS;
    for (unsigned i = 0; i < NPLAYERS; ++i) {
	_p[i].hand.sz = 0;
	_p[i].table.sz = 0;
	for (unsigned c = 0; c < MAXHAND; ++c)
	    _p[i].hand.c[_p[i].hand.sz++] = deck[--decksz];
	sort_hand (&_p[i].hand);
    }
    _crib.sz = 0;
    // Deal starter card
    _starter = deck[--decksz];
}

static void reverse_hand (card_t* hand, unsigned handsz)
{
    for (card_t *first = hand, *last = hand+handsz; first < last;)
	swap_cards (first++, --last);
}

// Algorithm from https://en.wikipedia.org/wiki/Permutation
static void next_hand_permutation (struct Hand* hand)
{
    // Find last ordered pair
    unsigned k = hand->sz;
    for (unsigned i = 1; i < hand->sz; ++i)
	if (hand->c[i-1] < hand->c[i])
	    k = i-1;
    // last permutation (if starting with sorted)
    if (k == hand->sz)
	return reverse_hand (hand->c, hand->sz);
    // Find widest ordered pair
    unsigned l = k+1;
    for (unsigned i = l; i < hand->sz; ++i)
	if (hand->c[k] < hand->c[i])
	    l = i;
    swap_cards (&hand->c[k], &hand->c[l]);
    reverse_hand (hand->c+k+1, hand->sz-(k+1));
}

static struct Hand hand_with_starter (const struct Hand* hand)
{
    struct Hand ch = *hand;
    ch.c[ch.sz++] = _starter;
    sort_hand (&ch);
    return ch;
}

//}}}-------------------------------------------------------------------
//{{{ Scoring

static void record_score (unsigned who, unsigned pts)
{
    if (!pts)
	return;
    _p[who].lastScore = _p[who].score;
    _p[who].score += pts;
}

static unsigned score_fifteens (const struct Hand* hand)
{
    // Fifteen is scored for each unique combination of cards, making
    // things rather difficult for the computer, as I was unable to find
    // an algorithm to pick all unique P of N. Hence, permutation of
    // the hand with previous match lookup is used. handsz is always
    // either 4 or 5, so performance is not an issue.
    struct Hand p;	// will hold each permutation
    p.sz = hand->sz;
    memcpy (p.c, hand->c, hand->sz*sizeof(card_t));
    struct Hand p15 [16];	// each permutation found (size arbitrary)
    unsigned n15 = 0;
    for (unsigned i = 0, n = n_permutations(hand->sz); i < n; ++i) {
	next_hand_permutation (&p);
	for (unsigned j = 0, sum = 0; j < hand->sz && sum < 15; ++j) {
	    if ((sum += card_value(p.c[j])) == 15) {
		// Permutations of the matching subset do
		// not count, so store and match it sorted.
		p15[n15].sz = j+1;
		memcpy (p15[n15].c, p.c, p15[n15].sz*sizeof(card_t));
		sort_hand (&p15[n15]);
		unsigned k = 0;
		for (; k < n15; ++k)
		    if (p15[n15].sz == p15[k].sz && 0 == memcmp (p15[n15].c, p15[k].c, p15[n15].sz*sizeof(card_t)))
			break;
		if (k == n15 && ++n15 >= ArraySize(p15))
		    return 2*n15;	// p15 is full, so can't continue
	    }
	}
    }
    return 2*n15;	// Each 15 is worth 2 points
}

static unsigned score_pairs (const struct Hand* hand)
{
    uint8_t counts [NRANKS] = {0};
    for (unsigned i = 0; i < hand->sz; ++i)
	++counts[card_rank(hand->c[i])];
    static const uint8_t c_CountScore[] = { 0, 0, 2, 6, 12 };
    unsigned score = 0;
    for (unsigned i = 0; i < ArraySize(counts); ++i)
	score += c_CountScore[counts[i]];
    return score;
}

static unsigned score_runs (const struct Hand* hand)
{
    uint8_t counts [NRANKS+1] = {0};
    for (unsigned i = 0; i < hand->sz; ++i)
	++counts[card_rank(hand->c[i])];
    unsigned score = 0;
    for (unsigned i = 0, runl = 0, mult = 1; i < ArraySize(counts); ++i) {
	if (counts[i]) {
	    mult *= counts[i];
	    ++runl;
	} else {
	    if (runl >= 3)
		score += runl*mult;
	    runl = 0;
	    mult = 1;
	}
    }
    return score;
}

static unsigned score_flush (const struct Hand* hand)
{
    uint8_t counts [NSUITS] = {0};
    for (unsigned i = 0; i < hand->sz; ++i)
	++counts[card_suit(hand->c[i])];
    unsigned score = 0;
    for (unsigned i = 0; i < ArraySize(counts); ++i) {
	if (i == card_suit(_starter) && counts[i] == 4)
	    --counts[i];		// 4 flush only counts in hand
	score = max_u (score, counts[i]);
    }
    return score < 4 ? 0 : score;	// flush is 4 or 5
}

static unsigned score_nobs (const struct Hand* hand)
{
    for (unsigned i = 0; i < hand->sz; ++i)
	if (hand->c[i] != _starter
		&& card_rank(hand->c[i]) == rank_Jack
		&& card_suit(hand->c[i]) == card_suit(_starter))
	    return 1;
    return 0;
}

static unsigned score_hand (const struct Hand* h, bool isCrib, bool explain)
{
    unsigned fifteens = score_fifteens (h);
    unsigned pairs = score_pairs (h);
    unsigned runs = score_runs (h);
    unsigned flushes = score_flush (h);
    if (isCrib && flushes == 4)	// the crib only scores 5 flush
	flushes = 0;
    unsigned nobs = score_nobs (h);
    unsigned score = fifteens + pairs + runs + flushes + nobs;
    if (explain) {
	print_msg ("%u ", score);
	if (score) {
	    char sep = '(';
	    if (fifteens)
		print_msg ("%c15-%u", sep, fifteens), sep = ',';
	    if (pairs)
		print_msg ("%cP-%u", sep, pairs), sep = ',';
	    if (runs)
		print_msg ("%cR-%u", sep, runs), sep = ',';
	    if (flushes)
		print_msg ("%cF-%u", sep, flushes), sep = ',';
	    if (nobs)
		print_msg ("%cNobs", sep);
	    print_msg (")");
	}
	print_msg ("\n");
    }
    return score;
}

static unsigned score_play (card_t lastCard, bool printMessage)
{
    struct Hand p = _play;
    p.c[p.sz++] = lastCard;

    // Compute count and points based on it
    unsigned cnt = _count + card_value(lastCard);
    if (cnt > 31)
	return 0;
    unsigned score = 0;
    if (cnt == 15)
	score += 2;
    else if (cnt == 31)
	++score;
    if (printMessage)
	print_msg ("%u", cnt);

    // Score pairs.
    unsigned pairlen = 0;
    while (pairlen < p.sz && card_rank(p.c[p.sz-pairlen-1]) == card_rank(lastCard))
	++pairlen;
    if (pairlen >= 2) {
	static const uint8_t c_PairScore[] = { 2, 6, 12 };
	score += c_PairScore[pairlen-2];
	if (printMessage) {
	    static const char* c_PairName[] = {"pair", "pair royal", "double pair royal"};
	    print_msg (" with a %s\n", c_PairName[pairlen-2]);
	}
    } else {	// can not score both a pair and a run
	uint8_t rnkc [NRANKS] = {0};
	for (unsigned i = 0; i < p.sz; ++i)
	    ++rnkc[card_rank(p.c[i])];
	// Run must contain the last card, may go up or down, and stops at duplicates
	unsigned runf = card_rank(lastCard), runl = runf+1;
	while (runf > 0 && rnkc[runf] == 1 && rnkc[runf-1] >= 1)
	    --runf;
	while (runl < NRANKS && rnkc[runl-1] == 1 && rnkc[runl] >= 1)
	    ++runl;
	if (runl-runf >= 3) {
	    score += runl-runf;
	    if (printMessage)
		print_msg (" with a run of %u\n", runl-runf);
	} else if (printMessage)
	    print_msg ("\n");
    }
    return score;
}

static unsigned n_playable_cards (const struct Hand* h)
{
    unsigned n = 0;
    for (unsigned i = 0; i < h->sz; ++i)
	n += (_count + card_value(h->c[i]) <= 31);
    return n;
}

static void score_last_played_card (void)
{
    _count = 0;
    _play.sz = 0;
    print_msg ("%s get 1 for last. Count reset.\n", player_pronoun(_last_player));
    record_score (_last_player, 1);
}

//}}}-------------------------------------------------------------------
//{{{ Drawing

static const char* player_pronoun (unsigned p)
{
    static const char c_Name[NPLAYERS][4] = {"You", "I"};
    return c_Name[p];
}

static void draw_peg (uint8_t score, unsigned line)
{
    unsigned x = 0, y = line;
    if (score > LINESCORE)
	++y;
    if (score > 0 && score < WINSCORE) {
	uint8_t lsc = score;
	if (score > LINESCORE)
	    lsc -= LINESCORE;
	uint8_t lo = lsc + (lsc-1)/5;	// 5 adjusts for gaps
	if (score > LINESCORE)
	    lo = 1+BLINE_LENGTH - lo;	// second line backwards
	x += lo;
    }
    mvwaddch (_wboard, y, x+1, '*');
}

static void draw_board (void)
{
    werase (_wboard);
    box (_wboard, 0, 0);
    static const uint8_t c_LineLocs[] = { 2, 3, 5, 6 };
    for (unsigned i = 0; i < ArraySize(c_LineLocs); ++i) {
	mvwhline (_wboard, c_LineLocs[i], 2, '.', BLINE_LENGTH);
	for (unsigned j = 5; j < BLINE_LENGTH; j += 6)
	    mvwaddch (_wboard, c_LineLocs[i], 2+j, ' ');
    }
    for (unsigned i = 0; i < NPLAYERS; ++i) {
	draw_peg (_p[i].score, c_LineLocs[i*BLINES]);
	draw_peg (_p[i].lastScore, c_LineLocs[i*BLINES]);
    }
    mvwprintw (_wboard, 1, 2, "Your score: %2u", _p[HUMAN].score);
    mvwprintw (_wboard, 7, 2, "My score:   %2u", _p[COMPUTER].score);
    mvwprintw (_wboard, 1, getmaxx(_wboard)-13, "%u games won", _p[HUMAN].gameswon);
    mvwprintw (_wboard, 7, getmaxx(_wboard)-13, "%u games won", _p[COMPUTER].gameswon);
    wnoutrefresh (_wboard);
}

static void draw_card (unsigned l, unsigned c, card_t v, bool selected)
{
    unsigned rank = v/NSUITS, suit = v%NSUITS, color = color_BlackSuit + suit%2 + selected*2;
    wattr_set (_wtable, A_NORMAL, color, NULL);
    mvwaddch (_wtable, l, c, rank < NRANKS ? "A23456789TJQK"[rank] : (int)ACS_CKBOARD);
    mvwaddch (_wtable, l, c+1, ' ');
    mvwaddch (_wtable, l+1, c, " ."[selected]);
    mvwadd_wchw (_wtable, l+1, c+1, get_card_suit_char (suit), A_NORMAL, color);
}

static void draw_hidden_card (unsigned l, unsigned c)
{
    wattr_set (_wtable, A_NORMAL, color_HiddenCard, NULL);
    mvwaddch (_wtable, l, c, ACS_ULCORNER);
    mvwaddch (_wtable, l, c+1, ACS_URCORNER);
    mvwaddch (_wtable, l+1, c, ACS_LLCORNER);
    mvwaddch (_wtable, l+1, c+1, ACS_LRCORNER);
}

static void draw_table (void)
{
    werase (_wtable);
    // Draw human side
    for (unsigned i = 0; i < _p[HUMAN].hand.sz; ++i)
	draw_card (0, i*3, _p[HUMAN].hand.c[i], _curcard == i);
    for (unsigned i = 0; i < _p[HUMAN].table.sz; ++i)
	draw_card (3, i*3, _p[HUMAN].table.c[i], false);
    // Draw computer side
    for (unsigned i = 0; i < _p[COMPUTER].hand.sz; ++i)
	draw_hidden_card (0, getmaxx(_wtable)-2-i*3);
    for (unsigned i = 0; i < _p[COMPUTER].table.sz; ++i)
	draw_card (3, getmaxx(_wtable)-2-i*3, _p[COMPUTER].table.c[i], false);
    // Draw crib
    if (_crib.sz == PLAYHAND && !_p[HUMAN].hand.sz && !_p[COMPUTER].hand.sz) {
	for (unsigned i = 0; i < _crib.sz; ++i)
	    draw_card (0, (22+2*_dealer)-3*i, _crib.c[i], false);
    } else {
	for (unsigned i = 0; i < _crib.sz; ++i)
	    draw_hidden_card (0, (22+2*_dealer)-3*i);
    }
    // Draw deck in the deal and starter otherwise
    if (_p[HUMAN].hand.sz > PLAYHAND)
	draw_hidden_card (getmaxy(_wtable)-3, getmaxx(_wtable)/2-1);
    else
	draw_card (getmaxy(_wtable)-3, getmaxx(_wtable)/2-1, _starter, false);
    wnoutrefresh (_wtable);
}

static void print_msg (const char* msg, ...)
{
    va_list args;
    va_start (args, msg);
    vw_printw (_wmsg, msg, args);
    va_end (args);
    wnoutrefresh (_wmsg);
}

static void draw_screen (void)
{
    wnoutrefresh (_wmsg);
    draw_board();
    draw_table();
    doupdate();
}

static void create_windows (void)
{
    initialize_curses();
    static const struct color_pair c_Pairs[] = {
	{ COLOR_YELLOW,	COLOR_BLUE	},	// color_Board
	{ COLOR_WHITE,	COLOR_BLUE	},	// color_HiddenCard
	{ COLOR_BLACK,	COLOR_WHITE	},	// color_BlackSuit
	{ COLOR_RED,	COLOR_WHITE	},	// color_RedSuit
	{ COLOR_BLACK,	COLOR_CYAN	},	// color_SelectedBlackSuit
	{ COLOR_RED,	COLOR_CYAN	}	// color_SelectedRedSuit
    };
    init_pairs (ArrayBlock (c_Pairs));
    _wboard = newwin (WBOARD_LINES, WBOARD_COLS, LINES-WBOARD_LINES, (COLS-WBOARD_COLS)/2);
    wbkgdset (_wboard, A_BOLD| COLOR_PAIR(color_Board));
    _wtable = newwin (WTABLE_LINES, WTABLE_COLS, getbegy(_wboard)-WTABLE_LINES, (COLS-WTABLE_COLS)/2);
    keypad (_wtable, true);
    _wmsg = newwin (getbegy(_wtable), WMSG_COLS, 0, (COLS-WMSG_COLS)/2);
    scrollok (_wmsg, true);
    wmove (_wmsg, getmaxy(_wmsg)-1, 0);
}

static void move_windows (void)
{
    erase();
    refresh();
    mvwin (_wboard, getbegy(_wboard), (COLS-WBOARD_COLS)/2);
    mvwin (_wtable, getbegy(_wtable), (COLS-WTABLE_COLS)/2);
    mvwin (_wmsg, getbegy(_wmsg), (COLS-WMSG_COLS)/2);
    draw_screen();
}

static unsigned select_card (void)
{
    unsigned nplayable = n_playable_cards (&_p[HUMAN].hand);
    if (!nplayable)
	return _p[HUMAN].hand.sz;
    unsigned maxcard = _p[HUMAN].hand.sz;
    if (_curcard >= maxcard)
	_curcard = 0;
    if (!maxcard)
	return 0;
    for (;;) {
	while (_count + card_value(_p[HUMAN].hand.c[_curcard]) > 31)
	    _curcard = (_curcard+1)%maxcard;
	if (nplayable == 1)
	    return _curcard;
	draw_screen();
	int k = wgetch (_wtable);
	if (k == 'q' || k == KEY_F(10))
	    exit (EXIT_SUCCESS);
	if (k == KEY_RESIZE)
	    move_windows();
	if ((k == 'j' || k == KEY_LEFT) && _curcard)
	    --_curcard;
	else if ((k == 'k' || k == KEY_RIGHT) && _curcard+1u < maxcard)
	    ++_curcard;
	else if (k == ' ' || k == '\n' || k == KEY_ENTER)
	    return _curcard;
    }
}

//}}}-------------------------------------------------------------------
//{{{ Computer AI

static void computer_discard (void)
{
    // Check all combinations of discards and pick one giving highest scoring hand
    unsigned c1 = 0, c2 = 1, maxscore = 0;
    for (unsigned i = 0; i < 6; ++i) {
	for (unsigned j = 0; j < 6; ++j) {
	    if (i == j)
		continue;
	    struct Hand chand = {0,{0}};
	    for (unsigned k = 0; k < 6; ++k)
		if (k != i && k != j)
		    chand.c[chand.sz++] = _p[COMPUTER].hand.c[k];
	    unsigned score = score_hand (&chand, false, false);
	    if (score > maxscore) {
		maxscore = score;
		c1 = i;
		c2 = j;
	    }
	}
    }
    // Zero score hand; discard two random cards.
    if (!maxscore) {
	c1 = nrand(6);
	do { c2 = nrand(6); } while (c1 == c2);
    }
    if (c1 < c2)
	--c2;	// adjust for removal order
    move_card (&_p[COMPUTER].hand, c1, &_crib);
    move_card (&_p[COMPUTER].hand, c2, &_crib);
}

static unsigned computer_select_play (void)
{
    // Check the score obtained from playing each card
    unsigned psc [PLAYHAND] = {0}, best [PLAYHAND] = {0}, nbest = 0;
    for (unsigned i = 0; i < _p[COMPUTER].hand.sz; ++i) {
	if (_count + card_value (_p[COMPUTER].hand.c[i]) > 31)
	    continue; // keep at 0, unplayable
	psc[i] = score_play (_p[COMPUTER].hand.c[i], false)+1;

	// Accumulate best score indexes
	if (psc[i] >= psc[best[nbest]]) {
	    if (psc[i] > psc[best[nbest]])
		nbest = 0;
	    best[nbest++] = i;
	}
    }

    // Check if there are no playable cards
    if (!nbest || best[0] >= _p[COMPUTER].hand.sz || !psc[best[0]])
	return _p[COMPUTER].hand.sz;

    if (nbest > 1) {
	// Due to large number of 10-value cards it is desirable
	// to avoid playing counts 10 less than a scoring value.
	uint8_t counts [PLAYHAND], nbad = 0;
	bool badchoice [PLAYHAND] = { false };
	for (unsigned i = 0; i < nbest; ++i) {
	    counts[i] = _count + card_value (_p[COMPUTER].hand.c[best[i]]);
	    if (counts[i] == 5 || counts[i] == 21) {
		badchoice[i] = true;
		++nbad;
	    }
	}
	if (nbad < nbest) {
	    unsigned* d = &best[0];
	    for (unsigned i = 0; i < nbest; ++i)
		if (!badchoice[i])
		    *d++ = best[i];
	    nbest -= nbad;
	}
    }

    // Randomly choose a remaining card
    return best [nrand(nbest)];
}

//}}}-------------------------------------------------------------------

static unsigned winning_player (void)
{
    unsigned winner = 0;
    for (; winner < NPLAYERS && _p[winner].score < WINSCORE; ++winner) {}
    return winner;
}

static bool run_play (void)
{
    // The deal
    print_msg ("\nStarting new play. %s deal.\n", player_pronoun(_dealer));
    deal_cards();
    computer_discard();
    print_msg ("Discard 2 cards\n");
    while (_p[HUMAN].hand.sz > PLAYHAND) {
	unsigned sc = select_card();
	move_card (&_p[HUMAN].hand, sc, &_crib);
    }
    sort_hand (&_crib);
    if (card_rank(_starter) == rank_Jack) {
	print_msg ("%s get one for \"his heels\"\n", player_pronoun(_dealer));
	record_score (_dealer, 1);
    }
    print_msg ("\n");

    // The play
    _play.sz = 0;
    for (uint8_t turn = !_dealer; _p[HUMAN].hand.sz + _p[COMPUTER].hand.sz && winning_player() >= NPLAYERS; turn = !turn) {
	if (!n_playable_cards(&_p[HUMAN].hand) && !n_playable_cards(&_p[COMPUTER].hand))
	    score_last_played_card();
	unsigned sc = (turn == HUMAN ? select_card() : computer_select_play());
	print_msg ("%s say ", player_pronoun(turn));
	if (sc < _p[turn].hand.sz && _count + card_value(_p[turn].hand.c[sc]) <= 31) {
	    card_t c = _p[turn].hand.c[sc];
	    move_card (&_p[turn].hand, sc, &_p[turn].table);
	    unsigned pts = score_play (c, true);
	    _count += card_value (c);
	    _play.c[_play.sz++] = c;
	    _last_player = turn;
	    record_score (turn, pts);
	} else
	    print_msg ("go\n");
    }
    if (winning_player() >= NPLAYERS)
	score_last_played_card();
    _count = 0;

    // The show
    print_msg ("\nPlay completed. Press space for show.\n");
    draw_screen();
    wgetch (_wtable);
    for (unsigned i = 0, turn = !_dealer; i < NPLAYERS && winning_player() >= NPLAYERS; ++i, turn = !turn) {
	struct Hand ch = hand_with_starter (&_p[turn].table);
	print_msg ("%s score ", player_pronoun(turn));
	unsigned pts = score_hand (&ch, false, true);
	if (turn == _dealer) {
	    print_msg (" + crib score ");
	    ch = hand_with_starter (&_crib);
	    pts += score_hand (&ch, true, true);
	}
	record_score (turn, pts);
    }

    // Display winner, if any, and pause
    unsigned winner = winning_player();
    if (winner < NPLAYERS) {
	print_msg ("%s won this game.\n", player_pronoun(winner));
	++_p[winner].gameswon;
	if (_p[!winner].score <= WINSCORE/2) {
	    print_msg ("%s skunked %s!\n", player_pronoun(winner), player_pronoun(!winner));
	    ++_p[winner].gameswon;
	}
	if (_p[HUMAN].gameswon + _p[COMPUTER].gameswon >= MAXGAMES)
	    print_msg ("\n%s won the match.\n", player_pronoun(_p[HUMAN].gameswon < _p[COMPUTER].gameswon));
    }
    draw_screen();
    wgetch (_wtable);

    // Reset scores if winner
    _dealer = !_dealer;
    if (winner < NPLAYERS) {
	_dealer = !winner;
	for (unsigned i = 0; i < NPLAYERS; ++i)
	    _p[i].score = _p[i].lastScore = 0;
    }
    return _p[HUMAN].gameswon + _p[COMPUTER].gameswon >= MAXGAMES;
}

int main (void)
{
    create_windows();
    for (_dealer = nrand (NPLAYERS); !run_play();) {}
    return EXIT_SUCCESS;
}
