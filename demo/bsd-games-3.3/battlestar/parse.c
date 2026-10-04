// This file is free software, distributed under the BSD license.

#include "battlestar.h"

//{{{ c_vocabulary -----------------------------------------------------

static const struct Word c_vocabulary[] = {
    {",",		CONJ,		AND		},
    {"a",		VERB,		AHEAD		},
    {"ahead",		VERB,		AHEAD		},
    {"all",		OBJECT,		EVERYTHING	},
    {"amulet",		NOUNS,		AMULET		},
    {"ancient",		ADJS,		0		},
    {"and",		CONJ,		AND		},
    {"attack",		VERB,		SMITE		},
    {"b",		VERB,		BACK		},
    {"back",		VERB,		BACK		},
    {"bar",		OBJECT,		BAR		},
    {"blast",		VERB,		SHOOT		},
    {"blaster",		OBJECT,		LASER		},
    {"block",		OBJECT,		BLOCK		},
    {"blue",		ADJS,		0		},
    {"body",		NOUNS,		MAID		},
    {"bomb",		OBJECT,		BOMB		},
    {"book",		OBJECT,		MATCHES		},
    {"bracelet",	OBJECT,		BRACELET	},
    {"brief",		VERB,		BRIEF		},
    {"broadsword",	OBJECT,		BROAD		},
    {"buckle",		VERB,		PUT		},
    {"bury",		VERB,		BURY		},
    {"car",		OBJECT,		CAR		},
    {"carry",		VERB,		DRAW		},
    {"chain",		OBJECT,		CHAIN		},
    {"chase",		VERB,		FOLLOW		},
    {"cleaver",		OBJECT,		CLEAVER		},
    {"climb",		VERB,		AUXVERB		},
    {"coat",		OBJECT,		MAIL		},
    {"coconut",		OBJECT,		COCONUTS	},
    {"coconuts",	OBJECT,		COCONUTS	},
    {"coins",		OBJECT,		COINS		},
    {"compass",		OBJECT,		COMPASS		},
    {"cut",		VERB,		SMITE		},
    {"d",		VERB,		DOWN		},
    {"dented",		ADJS,		0		},
    {"diamond",		OBJECT,		BLOCK		},
    {"dig",		VERB,		DIG		},
    {"door",		NOUNS,		DOOR		},
    {"down",		VERB,		DOWN		},
    {"draw",		VERB,		DRAW		},
    {"drink",		VERB,		DRINK		},
    {"drive",		VERB,		DRIVE		},
    {"drop",		VERB,		DROP		},
    {"eat",		VERB,		EAT		},
    {"elf",		OBJECT,		ELF		},
    {"everything",	OBJECT,		EVERYTHING	},
    {"fight",		VERB,		KILL		},
    {"fine",		OBJECT,		SWORD		},
    {"follow",		VERB,		FOLLOW		},
    {"fuck",		VERB,		LOVE		},
    {"get",		VERB,		TAKE		},
    {"girl",		NOUNS,		NATIVE_GIRL	},
    {"give",		VERB,		GIVE		},
    {"goddess",		NOUNS,		GODDESS		},
    {"gold",		ADJS,		0		},
    {"golden",		ADJS,		0		},
    {"grenade",		OBJECT,		GRENADE		},
    {"gun",		OBJECT,		LASER		},
    {"hack",		VERB,		SMITE		},
    {"halberd",		OBJECT,		HALBERD		},
    {"helmet",		OBJECT,		HELM		},
    {"horse",		OBJECT,		HORSE		},
    {"i",		VERB,		INVEN		},
    {"inven",		VERB,		INVEN		},
    {"jewels",		OBJECT,		POT		},
    {"jump",		VERB,		JUMP		},
    {"kerosene",	ADJS,		0		},
    {"kick",		VERB,		KICK		},
    {"kill",		VERB,		KILL		},
    {"kingly",		ADJS,		0		},
    {"kiss",		VERB,		KISS		},
    {"kiwi",		OBJECT,		KIWI		},
    {"knife",		OBJECT,		KNIFE		},
    {"l",		VERB,		LEFT		},
    {"lamp",		OBJECT,		LAMPON		},
    {"land",		VERB,		LANDIT		},
    {"lantern",		OBJECT,		LAMPON		},
    {"laser",		OBJECT,		LASER		},
    {"launch",		VERB,		LAUNCH		},
    {"left",		VERB,		LEFT		},
    {"levis",		OBJECT,		LEVIS		},
    {"light",		VERB,		LIGHT		},
    {"lo",		VERB,		LOOK		},
    {"look",		VERB,		LOOK		},
    {"love",		VERB,		LOVE		},
    {"mace",		OBJECT,		MACE		},
    {"maid",		OBJECT,		MAID		},
    {"maid's",		OBJECT,		MAID		},
    {"mail",		OBJECT,		MAIL		},
    {"make",		VERB,		AUXVERB		},
    {"mallet",		OBJECT,		MALLET		},
    {"man",		NOUNS,		MAN		},
    {"mango",		OBJECT,		MANGO		},
    {"match",		OBJECT,		MATCHES		},
    {"matches",		OBJECT,		MATCHES		},
    {"medallion",	NOUNS,		MEDALION	},
    {"mount",		VERB,		RIDE		},
    {"move",		VERB,		AUXVERB		},
    {"native",		NOUNS,		NATIVE_GIRL	},
    {"off",		PREPS,		OFF		},
    {"old",		ADJS,		0		},
    {"on",		PREPS,		ON		},
    {"open",		VERB,		OPEN		},
    {"ostrich",		ADJS,		0		},
    {"pajamas",		OBJECT,		PAJAMAS		},
    {"pants",		OBJECT,		LEVIS		},
    {"papaya",		OBJECT,		PAPAYAS		},
    {"papayas",		OBJECT,		PAPAYAS		},
    {"pineapple",	OBJECT,		PINEAPPLE	},
    {"pistol",		OBJECT,		LASER		},
    {"plumed",		ADJS,		0		},
    {"points",		VERB,		SCORE		},
    {"pot",		OBJECT,		POT		},
    {"potion",		OBJECT,		POTION		},
    {"pull",		VERB,		DRAW		},
    {"purple",		ADJS,		0		},
    {"put",		VERB,		PUT		},
    {"q",		VERB,		QUIT		},
    {"quit",		VERB,		QUIT		},
    {"r",		VERB,		RIGHT		},
    {"ride",		VERB,		RIDE		},
    {"right",		VERB,		RIGHT		},
    {"ring",		OBJECT,		RING		},
    {"robe",		OBJECT,		ROBE		},
    {"rope",		OBJECT,		ROPE		},
    {"rusty",		ADJS,		0		},
    {"save",		VERB,		SAVE		},
    {"scabbard",	OBJECT,		SWORD		},
    {"score",		VERB,		SCORE		},
    {"sheathe",		VERB,		WEARIT		},
    {"shield",		OBJECT,		SHIELD		},
    {"shoes",		OBJECT,		SHOES		},
    {"shoot",		VERB,		SHOOT		},
    {"shovel",		OBJECT,		SHOVEL		},
    {"sleep",		VERB,		SLEEP		},
    {"slice",		VERB,		SMITE		},
    {"smite",		VERB,		SMITE		},
    {"stab",		VERB,		SMITE		},
    {"stallion",	OBJECT,		HORSE		},
    {"starfighter",	OBJECT,		STARFIGHTER	},
    {"start",		VERB,		DRIVE		},
    {"strap",		VERB,		PUT		},
    {"strike",		VERB,		LIGHT		},
    {"su",		VERB,		SU		},
    {"swim",		VERB,		SWIM		},
    {"swing",		VERB,		SMITE		},
    {"sword",		NOUNS,		SWORD		},
    {"take",		VERB,		TAKE		},
    {"talisman",	NOUNS,		TALISMAN	},
    {"the",		ADJS,		0		},
    {"throw",		VERB,		THROW_OBJECT	},
    {"tie",		VERB,		PUT		},
    {"time",		VERB,		TIME		},
    {"timer",		NOUNS,		TIMER		},
    {"to",		ADJS,		0		},
    {"two-handed",	OBJECT,		TWO_HANDED	},
    {"u",		VERB,		UP		},
    {"unlock",		VERB,		OPEN		},
    {"up",		VERB,		UP		},
    {"use",		VERB,		USE		},
    {"verbose",		VERB,		VERBOSE		},
    {"volare",		OBJECT,		CAR		},
    {"warhead",		OBJECT,		BOMB		},
    {"wear",		VERB,		WEARIT		},
    {"woodsman",	OBJECT,		DEADWOOD	},
    {"woodsman's",	OBJECT,		DEADWOOD	}
};

//}}}-------------------------------------------------------------------

// Current input line. These are used in command bodies in cmd.c
struct Word words [NWORDS] = {};
uint8_t wordcount = 0;
uint8_t wordnumber = 0;

//----------------------------------------------------------------------

static char* getcom (char* buf, int size, const char* prompt, const char* error)
{
    for (;;) {
	fputs (prompt, stdout);
	if (fgets (buf, size, stdin) == 0) {
	    if (feof(stdin))
		die();
	    clearerr(stdin);
	    continue;
	}
	while (isspace (*buf))
	    ++buf;
	if (*buf)
	    break;
	if (error)
	    puts(error);
    }
    // If we didn't get to the end of the line, don't read it in next time.
    if (buf[strlen(buf) - 1] != '\n') {
	int i;
	while ((i = getchar()) != '\n' && i != EOF)
	    continue;
    }
    return buf;
}

// shifts to UPPERCASE if flag > 0, lowercase if flag < 0,
// and leaves it unchanged if flag = 0
static char* getword (char* buf1, char* buf2)
{
    unsigned cnt = 1;
    while (isspace (*buf1))
	++buf1;
    if (*buf1 != ',') {
	if (!*buf1) {
	    *buf2 = 0;
	    return 0;
	}
	for (; cnt < WORDLEN && *buf1 && !isspace (*buf1) && *buf1 != ','; ++cnt)
	    *buf2++ = tolower (*buf1++);
	if (cnt == WORDLEN)
	    while (*buf1 && !isspace (*buf1))
		++buf1;
    } else
	*buf2++ = *buf1++;
    *buf2 = '\0';
    while (isspace (*buf1))
	++buf1;
    return *buf1 ? buf1 : NULL;
}

static const struct Word* lookup (const char* s)
{
    for (unsigned f = 0, l = ArraySize (c_vocabulary); f < l;) {
	unsigned m = (f+l)/2u;
	int c = strcmp (c_vocabulary[m].word, s);
	if (c == 0)
	    return &c_vocabulary[m];
	else if (c < 0)
	    f = m+1;
	else
	    l = m;
    }
    return NULL;
}

void get_player_command (const char* prompt)
{
    char mainbuf [LINELENGTH];
    char* next = getcom (ArrayBlock(mainbuf), prompt, "Please type in something.");
    for (wordcount = 0; next && wordcount < NWORDS - 1; ++wordcount)
	next = getword (next, words[wordcount].word);

    wordnumber = 0;	       // for process_command
    for (unsigned n = 0; n <= wordcount; ++n) {
	const struct Word* wp = lookup (words[n].word);
	if (!wp) {
	    words[n].type = CONJ;	// Something to make an error
	    words[n].value = AND;
	} else
	    words[n] = *wp;
    }
    for (unsigned n = 1; n < wordcount; ++n) {
	if (words[n].type == ADJS	// We never use adjectives for anything, so yank them all.
	    || (words[n].type == CONJ	// Remove AND followed by not OBJECT
		&& (n+1 == wordcount || words[n+1].type != OBJECT))) {
	    memmove (&words[n], &words[n+1], sizeof(words[n])*(wordcount-n-1));
	    --wordcount;
	    --n;
	}
    }
}

int process_command (void)
{
    int lflag = -1;
    while (wordnumber <= wordcount) {
	if (words[wordnumber].type != VERB && !(words[wordnumber].type == OBJECT && words[wordnumber].value == KNIFE)) {
	    printf ("%s: How's that?\n", (wordnumber == wordcount) ? words[0].word : words[wordnumber].word);
	    return -1;
	}

	switch (words[wordnumber].value) {
	    case AUXVERB:
		// Take the following word as the verb (e.g. "make love", "climb up").
		++wordnumber;
		continue;

	    case UP:
		if (position == FINAL) {
		    if (game_state (IS_WIZARD))
			puts ("Zap! A gust of wind lifts you up.");
		    else if (!game_state (ROPE_IN_PIT)) {
			puts ("There is no way up.");
			return -1;
		    }
		}
		if (!moveplayer (location[position].dir.up, AHEAD))
		    return -1;
		lflag = 0;
		break;

	    case DOWN:
		if (!moveplayer (location[position].dir.down, AHEAD))
		    return -1;
		lflag = 0;
		break;

	    case LEFT:
		if (!moveplayer (relative_destination (LEFT), LEFT))
		    return -1;
		lflag = 0;
		break;

	    case RIGHT:
		if (!moveplayer (relative_destination (RIGHT), RIGHT))
		    return -1;
		lflag = 0;
		break;

	    case AHEAD:
		if (!moveplayer (relative_destination (AHEAD), AHEAD))
		    return -1;
		lflag = 0;
		break;

	    case BACK:
		if (!moveplayer (relative_destination (BACK), BACK))
		    return -1;
		lflag = 0;
		break;

	    case SHOOT:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n) {
			if (object_is_at (n, position) && c_objinfo[n].shdesc) {
			    ++things;
			    words[wordnumber + 1].value = n;
			    wordnumber = shoot();
			}
		    }
		    if (!things)
			puts("Nothing to shoot at!");
		    ++wordnumber;
		    ++wordnumber;
		} else
		    shoot();
		break;

	    case TAKE:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n)
			if (object_is_at (n, position) && c_objinfo[n].shdesc) {
			    ++things;
			    words[wordnumber + 1].value = n;

			    // Some objects (type NOUNS) have special
			    // treatment in take(). For these we must
			    // set the type to NOUNS. However for SWORD
			    // and MAID all it does is find which of many
			    // objects is meant, so we need do nothing
			    // here. BATHGOD must become GODDESS as well.
			    // NOUNS with no special case must be included
			    // here to get the right error. DOOR cannot
			    // occur as an object so need not be included.

			    switch (n) {
				case BATHGOD:
				    words[wordnumber + 1].value = GODDESS;
				    // FALLTHROUGH
				case GODDESS:
				case AMULET:
				case MEDALION:
				case TALISMAN:
				case MAN:
				case TIMER:
				case NATIVE_GIRL:
				    words[wordnumber + 1].type = NOUNS;
				    break;
				default:
				    words[wordnumber + 1].type = OBJECT;
			    }
			    wordnumber = take (position);
			}
		    ++wordnumber;
		    ++wordnumber;
		    if (!things)
			puts("Nothing to take!");
		} else
		    take (position);
		break;

	    case DROP:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n) {
			if (object_is_at (n, INVENTORY)) {
			    ++things;
			    words[wordnumber + 1].value = n;
			    wordnumber = drop("Dropped");
			}
		    }
		    ++wordnumber;
		    ++wordnumber;
		    if (!things)
			puts("Nothing to drop!");
		} else
		    drop("Dropped");
		break;

	    case KICK:
	    case THROW_OBJECT:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    unsigned wv = words[wordnumber].value;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n) {
			if (object_is_at (n, INVENTORY) || (object_is_at (n, position) && c_objinfo[n].shdesc)) {
			    ++things;
			    words[wordnumber + 1].value = n;
			    wordnumber = throw_object (words[wordnumber].value == KICK ? "Kicked" : "Thrown");
			}
		    }
		    wordnumber += 2;
		    if (!things)
			printf ("Nothing to %s!\n", wv == KICK ? "kick" : "throw");
		} else
		    throw_object (words[wordnumber].value == KICK ? "Kicked" : "Thrown");
		break;

	    case TAKEOFF:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n) {
			if (object_is_at (n, WEARING)) {
			    ++things;
			    words[wordnumber + 1].value = n;
			    wordnumber = takeoff();
			}
		    }
		    wordnumber += 2;
		    if (!things)
			puts("Nothing to take off!");
		} else
		    takeoff();
		break;

	    case DRAW:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n) {
			if (object_is_at (n, WEARING)) {
			    ++things;
			    words[wordnumber + 1].value = n;
			    wordnumber = draw();
			}
		    }
		    wordnumber += 2;
		    if (!things)
			puts("Nothing to draw!");
		} else
		    draw();
		break;

	    case PUTON:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n) {
			if (object_is_at (n, position) && c_objinfo[n].shdesc) {
			    ++things;
			    words[wordnumber + 1].value = n;
			    wordnumber = puton();
			}
		    }
		    wordnumber += 2;
		    if (!things)
			puts("Nothing to put on!");
		} else
		    puton();
		break;

	    case WEARIT:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n) {
			if (object_is_at (n, INVENTORY)) {
			    ++things;
			    words[wordnumber + 1].value = n;
			    wordnumber = wearit();
			}
		    }
		    wordnumber += 2;
		    if (!things)
			puts("Nothing to wear!");
		} else
		    wearit();
		break;

	    case EAT:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n) {
			if (object_is_at (n, INVENTORY)) {
			    ++things;
			    words[wordnumber + 1].value = n;
			    wordnumber = eat();
			}
		    }
		    wordnumber += 2;
		    if (!things)
			puts("Nothing to eat!");
		} else
		    eat();
		break;

	    case PUT:
		put();
		break;

	    case INVEN:
		if (count_objects_at (INVENTORY)) {
		    puts("You are holding:\n");
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n)
			if (object_is_at (n, INVENTORY))
			    printf ("\t%s\n", c_objinfo[n].shdesc);
		    unsigned mw = player_max_weight();
		    unsigned carrying = player_carrying();
		    if (!mw)
			printf ("\n= %d kilogram%s (can't lift any weight%s)\n", carrying, (carrying == 1 ? "." : "s."), (carrying ? " or move with what you have" : ""));
		    else
			printf ("\n= %d kilogram%s (%d%%)\n", carrying, (carrying == 1 ? "." : "s."), carrying * 100 / mw);
		    unsigned mc = player_max_cumber();
		    if (!mc)
			printf ("Your arms can't pick anything up.\n");
		    else
			printf ("Your arms are %d%% full.\n", player_encumber() * 100 / mc);
		} else
		    puts("You aren't carrying anything.");

		if (count_objects_at (WEARING)) {
		    puts("\nYou are wearing:\n");
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n)
			if (object_is_at (n, WEARING))
			    printf ("\t%s\n", c_objinfo[n].shdesc);
		} else
		    puts("\nYou are stark naked.");
		if (is_injured()) {
		    puts("\nYou have suffered:\n");
		    for (unsigned n = 0; n < NUMOFINJURIES; ++n)
			if (has_injury (n))
			    printf ("\t%s\n", ouch[n]);
		    unsigned mw = player_max_weight();
		    printf ("\nYou can still carry up to %d kilogram%s\n", mw, (mw == 1 ? "." : "s."));
		} else
		    puts("\nYou are in perfect health.");
		++wordnumber;
		break;

	    case USE:
		lflag = use();
		break;

	    case OPEN:
		if (wordnumber < wordcount && words[wordnumber + 1].value == EVERYTHING) {
		    unsigned things = 0;
		    for (unsigned n = 0; n < NUMOFOBJECTS; ++n) {
			if (object_is_at (n, INVENTORY)) {
			    ++things;
			    words[wordnumber + 1].value = n;
			    dooropen();
			}
		    }
		    wordnumber += 2;
		    if (!things)
			puts("Nothing to open!");
		} else
		    dooropen();
		break;

	    case LOOK:
		if (!game_state (CANTSEE)
			|| object_is_at (LAMPON, INVENTORY)
			|| object_is_at (LAMPON, position)
			|| game_state (MATCH_LIGHT)) {
		    write_location_long_description();
		    printobjs();
		    if (game_state (MATCH_LIGHT)) {
			puts("\nYour match splutters out.");
			clear_game_state (MATCH_LIGHT);
		    }
		    if (!visited_location (position))
			mark_location_visited (position);
		} else
		    puts("I can't see anything.");
		return -1;
		break;

	    case SU:
		if (game_state (IS_WIZARD)) {
		    printf ("\nRoom (was %d) = ", position);
		    char buffer[10];
		    fgets (buffer, 10, stdin);
		    if (*buffer != '\n')
			sscanf (buffer, "%hu", &position);
		    printf ("\nDONE.\n");
		    return 0;
		} else
		    puts("You aren't a wizard.");
		break;

	    case SCORE:
		print_score();
		break;

	    case SAVE:
		if (save())
		    exit (EXIT_SUCCESS);
		break;

	    case VERBOSE:
		set_game_state (IS_VERBOSE);
		printf ("[Maximum verbosity]\n");
		break;

	    case BRIEF:
		clear_game_state (IS_VERBOSE);
		printf ("[Standard verbosity]\n");
		break;

	    case FOLLOW:
		lflag = follow();
		break;

	    case GIVE:
		give();
		break;

	    case KISS:
		kiss();
		break;

	    case LOVE:
		love();
		break;

	    case RIDE:
		lflag = ride();
		break;

	    case DRIVE:
		lflag = drive();
		break;

	    case LIGHT:
		light();
		break;

	    case LAUNCH:
		if (!launch())
		    return -1;
		else
		    lflag = 0;
		break;

	    case LANDIT:
		if (!land())
		    return -1;
		else
		    lflag = 0;
		break;

	    case TIME:
		chime();
		break;

	    case SLEEP:
		zzz();
		break;

	    case DIG:
		dig();
		break;

	    case JUMP:
		lflag = jump();
		break;

	    case BURY:
		bury();
		break;

	    case SWIM:
		puts("Surf's up!");
		break;

	    case DRINK:
		drink();
		break;

	    case QUIT:
		die();

	    default:
		puts("How's that?");
		return -1;
		break;

	}
	if (wordnumber < wordcount && words[wordnumber++].word[0] == ',')
	    continue;
	else
	    return lflag;
    }
    return lflag;
}

void fight (enum ObjectId enemy, unsigned strength)
{
    for (;;) {
	// The player gets to attack first
	get_player_command (BOLD_ON "fight! > " BOLD_OFF);
	if (words[wordnumber].value == KILL || words[wordnumber].value == SMITE) {
	    unsigned dmg = player_melee_damage();
	    strength -= min_u (strength, dmg);
	    puts (melee_damage_message (dmg));
	} else if (words[wordnumber].value == SHOOT) {
	    if (!object_is_at (LASER, INVENTORY))
		puts ("Unfortunately, you don't have a blaster handy.");
	    else if (enemy == DARK_LORD && strength > 50) {
		puts ("With his bare hand he deflects the laser blast and whips the pistol from you!");
		move_object_to (LASER, INVENTORY, position);
	    } else {
		printf ("The %s took a direct hit!\n", c_objinfo[enemy].shdesc);
		strength -= min_u (strength, 50);
	    }
	} else if (words[wordnumber].value == DROP || words[wordnumber].value == DRAW) {
	    process_command();
	    --ourtime;
	} else if (words[wordnumber].value == BACK) {
	    location_t destback = relative_destination (BACK);
	    if (enemy == DARK_LORD && strength > 30) {
		puts ("He throws you back against the rock and pummels your face.");
		if ((object_is_at (AMULET, INVENTORY) || object_is_at (AMULET, WEARING))
			&& (object_is_at (MEDALION, INVENTORY) || object_is_at (MEDALION, WEARING)))
		    puts ("Lifting the amulet from you, his power grows and the walls of the\n"
			    "earth tremble. When he touches the medallion, your chest explodes\n"
			    "and the foundations of the earth collapse.\n"
			    "The planet is consumed by darkness.");
		else
		    puts ("I'm afraid you have been killed.");
		die();
	    } else if (!destback || position == destback)
		puts ("You can't seem to find the way out.");
	    else {
		puts ("You escape stunned and disoriented from the fight.\n"
			"A victorious bellow echoes from the battlescene.");
		moveplayer (destback, BACK);
		return;
	    }
	} else
	    puts ("You don't have a chance; he is too quick.");

	// Check if he is killed
	if (!strength) {
	    // Dark lord must be killed twice
	    if (enemy == DARK_LORD && position == MINE_CHAMBER) {
		puts ("The Dark Lord succumbs under your onslaught and falls.\n"
			"With a sinister smile on his lips he makes an arcane gesture\n"
			"and vanishes. His thunderous voice echoes through the cavern:\n"
			"\"follow me at your peril, for I am now in the heart of darkness,"
			"where your mortal soul would fade from existence in a heartbeat\".");
		move_object_to (enemy, position, FINAL);
		game_score.power += 10;
		game_score.ego += 4;
		return;
	    }
	    remove_object_from (enemy, position);
	    if (enemy == WOODSMAN)
		create_object_at (DEADWOOD, position);
	    else
		puts ("A watery black smoke consumes his body and then vanishes with a peal of thunder!");
	    printf ("You have killed the %s.\n", c_objinfo[enemy].shdesc);
	    game_score.power += 2;
	    if (enemy == DARK_LORD) {
		game_score.power += 18;
		game_score.ego += 10;
	    }
	    return;
	}

	// Attacks take time
	++ourtime;
	snooze -= 5;	// and are tiring
	if (snooze - ourtime < 20)
	    puts ("You look tired! I hope you're able to fight.");

	// If not, then he hits back
	puts ("He attacks...");
	unsigned hurt = player_melee_damage_taken();
	if (!has_injury (hurt)) {
	    suffer_injury (hurt);
	    printf ("I'm afraid you have suffered %s.\n", ouch[hurt]);
	} else
	    puts ("You emerge unscathed.");
	if (is_fatally_injured()) {
	    puts ("Your injuries are fatal.");
	    die();
	}
    }
}
