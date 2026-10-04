// This file is free software, distributed under the BSD license.

#include "battlestar.h"

//{{{ ouch -------------------------------------------------------------

const char ouch [NUMOFINJURIES][24] = {
    "some minor abrasions",
    "some minor lacerations",
    "a minor puncture wound",
    "a minor amputation",
    "a sprained wrist",
    "a fractured ankle",
    "a broken arm",
    "a few broken ribs",
    "a broken leg",
    "a broken back",
    "a large loss of blood",
    "a fractured skull",
    "a broken neck"
};

//}}}-------------------------------------------------------------------

// Parsed command line variables, in parse.c
extern struct Word words [NWORDS];
extern uint8_t wordcount;
extern uint8_t wordnumber;

//----------------------------------------------------------------------

bool moveplayer (location_t thataway, enum CommandId token)
{
    ++wordnumber;
    if ((!game_state (CANTMOVE) && !game_state (LAUNCHED))
	    || object_is_at (LAND, position)
	    || (fuel > 0 && game_state (LAUNCHED))) {
	if (!thataway) {
	    puts ("You can't go this way.");
	    return false;
	}
	position = thataway;
	direction = compass_direction (token);
	++ourtime;
    } else {
	if (game_state (CANTMOVE) && !game_state (LAUNCHED))
	    puts ("You aren't able to move; you better drop something.");
	else
	    puts ("You are out of fuel; now you will drift in space forever!");
    }
    return true;
}

void news (void)
{
    if (ourtime > 64 && position < OUTSIDE_BATTLESTAR) {
	puts ("An explosion of shuddering magnitude splinters bulkheads and\n"
		"ruptures the battlestar's hull. You are sucked out into the\n"
		"frozen void of space and killed.");
	die();
    }
    if (ourtime > 48 && position < OUTSIDE_BATTLESTAR)
	puts ("Explosions rock the battlestar.");
    if (ourtime > snooze) {
	puts ("You drop from exhaustion...");
	zzz();
    }
    if (ourtime > snooze - 5)
	puts ("You're getting tired.");
    if (ourtime > (rythmn + CYCLE)) {
	if (game_state (IS_NIGHT)) {
	    convert (TODAY);
	    if (is_outside() && ourtime - rythmn - CYCLE < 10) {
		puts ("Dew lit sunbeams stretch out from a watery sunrise and herald the dawn.\n"
			"You awake from a misty dream-world into stark reality.\nIt is day.");
	    }
	} else {
	    convert (TONIGHT);
	    remove_object_from (BATHGOD, POOLS);
	    if (is_outside() && ourtime - rythmn - CYCLE < 10) {
		puts ("The dying sun sinks into the ocean, leaving a blood-stained sunset.\n"
			"The sky slowly fades from orange to violet to black.\n"
			"A few stars flicker on, and it is night.\n"
			"The world seems completely different at night.");
	    }
	}
	rythmn = ourtime - ourtime % CYCLE;
    }
    if (!game_state (IS_WIZARD)
	    && (object_is_at (TALISMAN, INVENTORY) || object_is_at (TALISMAN, WEARING))
	    && (object_is_at (MEDALION, INVENTORY) || object_is_at (MEDALION, WEARING))
	    && (object_is_at (AMULET, INVENTORY) || object_is_at (AMULET, WEARING))) {
	set_game_state (IS_WIZARD);
	puts ("The three amulets glow and reenforce each other in power.\nYou are now a wizard.");
    }
    if (object_is_at (ELF, position)) {
	printf ("%s\n", c_objinfo[ELF].desc);
	fight (ELF, nrand(30));
    }
    if (object_is_at (DARK_LORD, position)) {
	printf ("%s\n", c_objinfo[DARK_LORD].desc);
	fight (DARK_LORD, 100);
    }
    if (object_is_at (WOODSMAN, position)) {
	printf ("%s\n", c_objinfo[WOODSMAN].desc);
	fight (WOODSMAN, 50);
    }
    switch (position) {
	case 246:
	case 257:	// entering a cave
	case 267:
	case 274:
	    set_game_state (CANTSEE);
	    break;
	case SEA_CAVE_ENTRANCE:
	case 216:	// leaving a cave
	case 230:
	case CAVE_ENTRANCE:
	case HUGE_CHASM:
	    clear_game_state (CANTSEE);
	    break;
    }
    if (object_is_at (GIRL, position))
	set_game_state (MET_GIRL);
    if (game_state (MET_GIRL) && CYCLE * 1.5 - ourtime < 10) {
	create_object_at (GIRLTALK, GARDEN);
	create_object_at (LAMPON, GARDEN);
	create_object_at (ROPE, GARDEN);
    }
    if (position == DOCK && (visited_location (position) || ourtime > CYCLE)) {
	remove_object_from (GIRL, DOCK);
	remove_object_from (MAN, DOCK);
    }
    if (game_state (MET_GIRL) && ourtime - CYCLE * 1.5 > 10) {
	remove_object_from (GIRLTALK, GARDEN);
	remove_object_from (LAMPON, GARDEN);
	remove_object_from (ROPE, GARDEN);
	clear_game_state (MET_GIRL);
    }
    if (object_is_at (DRENIAN, position)) {
	puts ("Oh my God, you're being shot at by an alien spacecraft!");
	fflush (stdout);
	sleep (1);
	if (!visual()) {
	    unsigned hurt = nrand (NUMOFINJURIES);
	    suffer_injury (hurt);
	    puts ("Laser blasts sear the cockpit, and the alien veers off in a victory roll.");
	    puts ("The starfighter shudders under a terrible explosion.");
	    printf ("I'm afraid you have suffered %s.\n", ouch[hurt]);
	} else
	    remove_object_from (DRENIAN, position);
    }
    if (is_fatally_injured()) {
	puts ("I'm afraid you have suffered fatal injuries.");
	die();
    }
    if (player_carrying() > player_max_weight() || player_encumber() > player_max_cumber())
	set_game_state (CANTMOVE);
    else
	clear_game_state (CANTMOVE);
}

void crash (void)
{
    --fuel;
    if (location[position].dir.flyhere && (!object_is_at (LAND, position) || fuel >= 2)) // need 2 fuel to land
	return;
    if (!location[position].dir.flyhere)
	puts ("You're flying too low. We're going to crash!");
    else {
	puts ("You're out of fuel. We'll have to crash land!");
	if (!location[position].dir.down) {
	    puts ("Your starfighter strikes the ground and explodes into fiery fragments.");
	    puts ("Thick black smoke billows up from the wreckage.");
	    die();
	}
	position = location[position].dir.down;
    }
    clear_game_state (LAUNCHED);
    create_object_at (CRASH, position);
    ourtime += nrand(CYCLE / 4);
    puts ("The starfighter explodes into the ground and you lose consciousness...");
    zzz();
    unsigned hurt1 = nrand (NUMOFINJURIES-MINOR_PUNCTURE)+MINOR_PUNCTURE;
    unsigned hurt2 = nrand (NUMOFINJURIES-MINOR_PUNCTURE)+MINOR_PUNCTURE;
    suffer_injury (hurt1);
    suffer_injury (hurt2);
    suffer_injury (ABRASIONS);	// abrasions
    suffer_injury (LACERATIONS);	// lacerations
    printf ("I'm afraid you have suffered %s and %s.\n", ouch[hurt1], ouch[hurt2]);
}

static bool is_wearable (enum ObjectId oid)
{
    static const uint16_t c_wearable[] = {
	AMULET, BRACELET, COMPASS, GRENADE, HELM,
	KNIFE, LASER, LEVIS, MAIL, MEDALION,
	PAJAMAS, RING, ROBE, ROPE, SHOES,
	SWORD, TALISMAN
    };
    for (unsigned i = 0; i < ArraySize (c_wearable); ++i)
	if (c_wearable[i] == oid)
	    return true;
    return false;
}

int wearit (void)
{
    int firstnumber = wordnumber;
    ++wordnumber;
    while (wordnumber <= wordcount && (words[wordnumber].type == OBJECT || words[wordnumber].type == NOUNS) && words[wordnumber].value != DOOR) {
	int value = words[wordnumber].value;
	if (value >= 0 && !c_objinfo[value].shdesc)
	    break;
	if (value < 0) {
	    puts ("Wear what?");
	    return firstnumber;
	}
	if (!is_wearable (value)) {
	    printf ("You can't wear %s%s!\n", A_OR_AN_OR_BLANK(value), c_objinfo[value].shdesc);
	    return firstnumber;
	}
	if (object_is_at (value, INVENTORY)) {
	    move_object_to (value, INVENTORY, WEARING);
	    ++ourtime;
	    printf ("You are now wearing %s%s.\n", A_OR_AN_OR_THE(value), c_objinfo[value].shdesc);
	} else if (object_is_at (value, WEARING))
	    printf ("You are already wearing the %s.\n", c_objinfo[value].shdesc);
	else
	    printf ("You aren't holding the %s.\n", c_objinfo[value].shdesc);
	if (wordnumber < wordcount - 1 && words[++wordnumber].value == AND)
	    ++wordnumber;
	else
	    return firstnumber;
    }
    puts ("Don't be ridiculous.");
    return firstnumber;
}

int put (void)
{			       // synonyms = {buckle, strap, tie}
    if (words[wordnumber + 1].value == ON) {
	words[++wordnumber].value = PUTON;
	words[wordnumber].type = VERB;
	return process_command();
    }
    if (words[wordnumber + 1].value == DOWN) {
	words[++wordnumber].value = DROP;
	words[wordnumber].type = VERB;
	return process_command();
    }
    puts ("I don't understand what you want to put.");
    return -1;
}

int draw (void)
{			       // synonyms = {pull, carry}
    return take (WEARING);
}

int use (void)
{
    ++wordnumber;
    if (words[wordnumber].value == AMULET
	    && object_is_at (AMULET, INVENTORY)
	    && position >= FIRST_ISLAND_LAND) {
	if (position == FINAL) {
	    puts ("The amulet won't work in here.");
	    return -1;
	}
	puts ("The amulet begins to glow.");
	if (object_is_at (MEDALION, INVENTORY)) {
	    puts ("The medallion comes to life too.");
	    if (position == ABOVE_SEA_CAVE) {
		set_game_state (UNCOVERED_SEA_CAVE);
		puts ("The waves subside and it is possible to descend to the sea cave now.");
		++ourtime;
		return -1;
	    }
	}
	puts ("A light mist falls over your eyes and the sound of purling water trickles in");
	puts ("your ears. When the mist lifts you are standing beside a cool stream.");
	if (position == CAVE_STREAM_BANK)
	    position = END_OF_THE_ROAD;
	else
	    position = CAVE_STREAM_BANK;
	++ourtime;
	clear_game_state (CANTSEE);
	return 0;
    } else if (words[wordnumber].value == COMPASS) {
	if (!object_is_at (COMPASS, INVENTORY) && !object_is_at (COMPASS, WEARING))
	    puts ("You don't have a compass.");
	else
	    printf ("Your compass points %s.\n", truedirec (NORTH, '-'));
    } else
	puts ("There is no apparent use.");
    return -1;
}

int follow (void)
{
    if (position == MINE_CHAMBER && object_is_at (DARK_LORD, FINAL)) {
	puts ("The Dark Lord leaps away and runs down secret tunnels and corridors.\n"
		"You chase him through the darkness and splash in pools of water.\n"
		"You have cornered him. His laser sword extends as he steps forward.");
	position = FINAL;
	fight (DARK_LORD, 75);
	create_object_at (TALISMAN, position);
	return 0;
    } else if (position == POOLS && object_is_at (GODDESS, POOLS)) {
	puts ("The goddess leads you down a steamy tunnel and into a high, wide chamber.\n"
		"She sits down on a throne.");
	move_object_to (GODDESS, POOLS, GODDESS_THRONE_ROOM);
	position = GODDESS_THRONE_ROOM;
	set_game_state (CANTSEE);
	return 0;
    } else
	puts ("There is no one to follow.");
    return -1;
}

void dig (void)
{
    if (!object_is_at (SHOVEL, INVENTORY)) {
	puts ("You don't have a shovel.");
	return;
    }
    puts ("Ok");
    ++ourtime;
    if (position != SECRET_THICKET)
	puts ("Nothing happens.");
    else if (!game_state (DUG)) {
	set_game_state (DUG);
	create_object_at (DEADWOOD, position);
	create_object_at (COMPASS, position);
	create_object_at (KNIFE, position);
	create_object_at (MACE, position);
    }
}

int jump (void)
{
    switch (position) {
	default:		puts ("Nothing happens."); return -1;
	case ALONG_THE_SHORE:
	case COAST_ROAD_TURN:
	case ALONG_HIGH_CLIFFS:
	case ROAD_TURNAROUND:	position = TIDE_POOLS;	break;
	case HUGE_CHASM:	position = FINAL;	break;
	case BRIDGE_OVER_LAGOON: position = IN_THE_LAGOON; break;
	case CLIFF_WATERFALL:	position = WIDER_CANYON; break;
	case HANGAR_GALLERY:	position = MAIN_HANGAR;	break;
    }
    puts ("Ahhhhhhh...");
    suffer_injury (BROKEN_LEG);
    suffer_injury (BROKEN_RIBS);
    suffer_injury (BROKEN_ARM);
    for (unsigned n = 0; n < NUMOFOBJECTS; ++n)
	move_object_to (n, INVENTORY, position);
    return 0;
}

void bury (void)
{
    if (!object_is_at (SHOVEL, INVENTORY)) {
	puts ("You aren't holding a shovel.");
	return;
    }
    static const uint16_t c_buryable[] = { MAID, DEADWOOD };
    while (words[++wordnumber].type != OBJECT && words[wordnumber].type != NOUNS && wordnumber < wordcount)
	continue;
    int value = words[wordnumber].value;
    if (words[wordnumber].type == NOUNS && (object_is_at (value, position) || value == MAID)) {
	switch (value) {
	    case MAID:
		words[wordnumber].type = OBJECT;
		for (unsigned i = 0; i < ArraySize(c_buryable); ++i)
		    if (object_is_at (c_buryable[i], INVENTORY) || object_is_at (c_buryable[i], position))
			value = c_buryable[i];
		break;
	    case TIMER:		game_score.power += 7; game_score.ego -= min_u (game_score.ego,10); // fallthrough
	    case AMULET:
	    case MEDALION:
	    case TALISMAN:	words[wordnumber].type = OBJECT; break;
	    default:		puts ("Wha..?"); break;
	}
    }
    if (words[wordnumber].type == OBJECT && position >= FIRST_ISLAND_LAND
	    && (object_is_at (value, INVENTORY) || object_is_at (value, position))) {
	puts ("Buried.");
	if (object_is_at (value, INVENTORY))
	    remove_object_from (value, INVENTORY);
	remove_object_from (value, position);
	for (unsigned i = 0; i < ArraySize(c_buryable); ++i) {
	    if (c_buryable[i] == value) {
		game_score.ego += 2;
		printf ("The %s should rest easier now.\n", c_objinfo[value].shdesc);
	    }
	}
    } else
	puts ("It doesn't seem to work.");
}

void drink (void)
{
    if (!object_is_at (POTION, INVENTORY)) {
	puts ("You have nothing to drink.");
	return;
    }
    puts ("The cool liquid runs down your throat but turns to fire and you choke.\n"
	"The heat reaches your limbs and tingles your spirit.\n"
	"You feel like falling asleep.");
    remove_object_from (POTION, INVENTORY);
    cure_all_injuries();
    ++ourtime;
    zzz();
}

int shoot (void)
{
    int firstnumber = wordnumber;
    if (!object_is_at (LASER, INVENTORY)) {
	puts ("You aren't holding a blaster.");
	return firstnumber;
    }
    ++wordnumber;
    while (wordnumber <= wordcount && words[wordnumber].type == OBJECT) {
	int value = words[wordnumber].value;
	printf ("%s:\n", c_objinfo[value].shdesc);
	if (object_is_at (value, position)) {
	    remove_object_from (value, position);
	    ++ourtime;
	    printf ("The %s explode%s\n", c_objinfo[value].shdesc, (is_plural_object(value) ? "." : "s."));
	    if (value == BOMB)
		die();
	} else
	    printf ("I don't see any %s around here.\n", c_objinfo[value].shdesc);
	if (wordnumber < wordcount - 1 && words[++wordnumber].value == AND)
	    ++wordnumber;
	else
	    return firstnumber;
    }
    // The blaster's only useful purpose is to destroy locked doors.
    if (wordnumber > wordcount || words[wordnumber].value != DOOR) {
	puts ("Shoot what?");
	return firstnumber;
    }
    ++ourtime;
    switch (position) {
	case CAVE_DOOR:
	case CAVE_ENTRANCE:
	    puts ("The door is unhinged.");
	    set_game_state (OPENED_CAVE_DOOR);
	    break;
	case KITCHEN_DOOR:
	    puts ("The wooden door splinters.");
	    set_game_state (OPENED_KITCHEN_DOOR);
	    break;
	case DINING_ROOM_DOOR:
	    puts ("The laser blast has no effect on the door.");
	    break;
	case LASER_ROOM:
	    puts ("The blast hits the door and it explodes into flame.\n"
		  "The magnesium burns so rapidly that we have no chance to escape.");
	    die();
	default:
	    puts ("Nothing happens.");
    }
    return firstnumber;
}

int take (location_t fromloc)
{
    int firstnumber = wordnumber;
    if (wordnumber < wordcount && words[wordnumber + 1].value == OFF) {
	++wordnumber;
	words[wordnumber].value = TAKEOFF;
	words[wordnumber].type = VERB;
	return process_command();
    } else {
	++wordnumber;
	while (wordnumber <= wordcount && words[wordnumber].type == OBJECT) {
	    int value = words[wordnumber].value;
	    printf ("%s:\n", c_objinfo[value].shdesc);
	    bool heavy = (player_carrying() + c_objinfo[value].weight) <= player_max_weight();
	    bool bulky = (player_encumber() + c_objinfo[value].cumber) <= player_max_cumber();
	    if ((object_is_at (value, fromloc) || game_state (IS_WIZARD)) && heavy && bulky && !object_is_at (value, INVENTORY)) {
		create_object_at (value, INVENTORY);
		++ourtime;
		if (object_is_at (value, fromloc))
		    printf ("Taken.\n");
		else
		    printf ("Zap! Taken from thin air.\n");
		remove_object_from (value, fromloc);
		if (value == MEDALION && win)
		    --win;
	    } else if (object_is_at (value, INVENTORY))
		printf ("You're already holding %s%s.\n", A_OR_AN_OR_BLANK(value), c_objinfo[value].shdesc);
	    else if (!object_is_at (value, fromloc))
		printf ("I don't see any %s around here.\n", c_objinfo[value].shdesc);
	    else if (!heavy)
		printf ("The %s %stoo heavy.\n", c_objinfo[value].shdesc, IS_OR_ARE(value));
	    else
		printf ("The %s %stoo cumbersome to hold.\n", c_objinfo[value].shdesc, IS_OR_ARE(value));
	    if (wordnumber < wordcount - 1 && words[++wordnumber].value == AND)
		++wordnumber;
	    else
		return firstnumber;
	}
    }

    if (wordnumber > wordcount || words[wordnumber].type != NOUNS) {
	puts ("You've got to be kidding.");
	return firstnumber;
    }

    // special cases with their own returns
    switch (words[wordnumber].value) {

	case SWORD:
	    if (object_is_at (SWORD, fromloc)) {
		words[wordnumber--].type = OBJECT;
		return take(fromloc);
	    }
	    if (object_is_at (TWO_HANDED, fromloc)) {
		words[wordnumber].value = TWO_HANDED;
		words[wordnumber--].type = OBJECT;
		return take(fromloc);
	    }
	    words[wordnumber].value = BROAD;
	    words[wordnumber--].type = OBJECT;
	    return take(fromloc);

	case MAID:
	    if (object_is_at (MAID, fromloc)) {
		words[wordnumber].value = MAID;
		words[wordnumber--].type = OBJECT;
		return take(fromloc);
	    } else if (object_is_at (DEADWOOD, fromloc)) {
		words[wordnumber].value = DEADWOOD;
		words[wordnumber--].type = OBJECT;
		return take(fromloc);
	    } else
		puts ("It doesn't seem to work.");
	    break;

	case AMULET:
	    if (object_is_at (AMULET, position)) {
		puts ("The amulet is warm to the touch, and its beauty catches your breath.\n"
			"A mist falls over your eyes, but then it is gone. Sounds seem clearer\n"
			"and sharper but far away as if in a dream. The sound of purling water\n"
			"reaches you from afar. The mist falls again, and your heart leaps in horror.\n"
			"The gold freezes your hands and fathomless darkness engulfs your soul.");
	    }
	    words[wordnumber--].type = OBJECT;
	    return take(fromloc);

	case MEDALION:
	    if (object_is_at (MEDALION, position)) {
		puts ("The medallion is warm, and it rekindles your spirit with the warmth of life.\n"
			"Your amulet begins to glow as the medallion is brought near to it,\n"
			"and together they radiate.");
	    }
	    words[wordnumber--].type = OBJECT;
	    return take(fromloc);

	case TALISMAN:
	    if (object_is_at (TALISMAN, position))
		puts ("The talisman is cold to the touch, and it sends a chill down your spine.");
	    words[wordnumber--].type = OBJECT;
	    return take(fromloc);

	case GODDESS:
	    if (!object_is_at (BATHGOD, position))
		puts ("She is not here.");
	    else if (object_is_at (AMULET, WEARING) || object_is_at (AMULET, INVENTORY)) {
		puts ("She offers a delicate hand, and you help her out of the sparkling springs.\n"
			"Water droplets like liquid silver bedew her golden skin, but when they part\n"
			"from her, they fall as teardrops. She wraps a single cloth around her and\n"
			"ties it at the waist. Around her neck hangs a golden amulet.\n"
			"She bids you to follow her, and walks away.");
		++game_score.pleasure;
		remove_object_from (BATHGOD, position);
		create_object_at (GODDESS, position);
	    } else
		puts ("She moves away from you.");
	    break;

	default:
	    puts ("It doesn't seem to work.");
	    break;
    }
    return firstnumber;
}

int throw_object (const char *name)
{
    int first = wordnumber;
    location_t deposit = 0;
    if (drop(name) != -1) {
	switch (words[wordnumber].value) {
	    default:
		deposit = relative_destination (words[wordnumber].value);
		break;
	    case UP:
		if (position == FINAL && !game_state (ROPE_IN_PIT))
		    deposit = 0;
		else
		    deposit = location[position].dir.up;
		break;
	    case DOWN:
		deposit = location[position].dir.down;
		break;
	}
	wordnumber = first + 1;
	while (wordnumber <= wordcount) {
	    int value = words[wordnumber].value;
	    if (deposit && object_is_at (value, position)) {
		remove_object_from (value, position);
		if (value == GRENADE) {
		    puts ("A thundering explosion nearby sends up a cloud of smoke and shrapnel.");
		    for (unsigned o = 0; o < NUMOFOBJECTS; ++o)
			remove_object_from (o, deposit);
		    create_object_at (CHAR, deposit);
		} else
		    create_object_at (value, deposit);
		if (value == ROPE && position == FINAL)
		    set_game_state (ROPE_IN_PIT);
		switch (deposit) {
		    case CAVE_DOOR:
		    case CAVE_ENTRANCE:
			puts ("The stone door is unhinged.");
			set_game_state (OPENED_CAVE_DOOR);
			break;
		    case KITCHEN_DOOR:
			puts ("The wooden door is blown open.");
			set_game_state (OPENED_KITCHEN_DOOR);
			break;
		    case DINING_ROOM_DOOR:
			puts ("The door is not damaged.");
		}
	    } else if (value == GRENADE && object_is_at (value, position)) {
		puts ("You are blown into shreds when your grenade explodes.");
		die();
	    }
	    if (wordnumber < wordcount - 1 && words[++wordnumber].value == AND)
		++wordnumber;
	    else
		return first;
	}
	return first;
    }
    return first;
}

int drop (const char *name)
{
    int firstnumber = wordnumber;
    ++wordnumber;
    while (wordnumber <= wordcount && (words[wordnumber].type == OBJECT || words[wordnumber].type == NOUNS)) {
	int value = words[wordnumber].value;
	if (value == MAID) {   // special case
	    words[wordnumber].type = OBJECT;
	    if (object_is_at (MAID, INVENTORY) || object_is_at (MAID, position))
		value = MAID;
	    else if (object_is_at (DEADWOOD, INVENTORY) || object_is_at (DEADWOOD, position))
		value = DEADWOOD;
	}
	if (words[wordnumber].type == NOUNS && value == DOOR) {
	    if (*name == 'K')
		puts ("You hurt your foot.");
	    else
		puts ("You're not holding a door.");
	} else if (c_objinfo[value].shdesc == NULL) {
	    if (*name == 'K')
		puts ("That's not for kicking!");
	    else
		puts ("You don't have that.");
	} else {
	    printf ("%s:\n", c_objinfo[value].shdesc);
	    if (object_is_at (value, INVENTORY)) {
		move_object_to (value, INVENTORY, position);
		if (value == BOMB) {
		    puts ("The bomb explodes. A blinding white light and immense concussion obliterate us.");
		    die();
		}
		++ourtime;
		if (*name == 'K')
		    puts ("Drop kicked.");
		else
		    printf ("%s.\n", name);
		if (game_state (IS_WIZARD) && (value == AMULET || value == MEDALION || value == TALISMAN)) {
		    clear_game_state (IS_WIZARD);
		    puts ("You no longer feel wizardly.");
		}
	    } else {
		if (*name != 'K') {
		    printf ("You aren't holding the %s.\n", c_objinfo[value].shdesc);
		    if (object_is_at (value, position)) {
			if (*name == 'T')
			    puts ("Kicked instead.");
			else if (*name == 'G')
			    puts ("Given anyway.");
		    }
		} else if (object_is_at (value, position))
		    puts ("Kicked.");
		else if (object_is_at (value, WEARING))
		    puts ("Not while it's being worn.");
		else
		    puts ("Not found.");
	    }
	}
	if (wordnumber < wordcount - 1 && words[++wordnumber].value == AND)
	    ++wordnumber;
	else
	    return firstnumber;
    }
    puts ("Do what?");
    return -1;
}

int takeoff (void)
{
    wordnumber = take (WEARING);
    return drop("Dropped");
}

int puton (void)
{
    wordnumber = take (position);
    return wearit();
}

static bool is_edible (enum ObjectId oid)
{
    static const uint16_t c_edible[] = { PAPAYAS, PINEAPPLE, KIWI, COCONUTS, MANGO };
    for (unsigned i = 0; i < ArraySize(c_edible); ++i)
	if (c_edible[i] == oid)
	    return true;
    return false;
}

int eat (void)
{
    int firstnumber = wordnumber;
    ++wordnumber;
    while (wordnumber <= wordcount) {
	int value = words[wordnumber].value;
	if (value < 0 || value >= NUMOFOBJECTS) {
	    puts ("Eat what?");
	    return firstnumber;
	}
	if (words[wordnumber].type != OBJECT || c_objinfo[value].shdesc == NULL) {
	    puts ("You can't eat that!");
	    return firstnumber;
	}
	if (!is_edible (value))
	    printf ("You can't eat %s%s!\n", A_OR_AN_OR_BLANK(value), c_objinfo[value].shdesc);
	else if (!object_is_at (value, INVENTORY))
	    printf ("You aren't holding the %s.\n", c_objinfo[value].shdesc);
	else if (ourtime < ate - CYCLE)
	    puts ("You're stuffed.");
	else if (!object_is_at (KNIFE, INVENTORY))
	    puts ("You need a knife.");
	else {
	    remove_object_from (value, INVENTORY);
	    ate = max_i (ourtime, ate) + CYCLE / 3;
	    snooze += CYCLE / 10;
	    ++ourtime;
	    printf ("You ate %s%s. You can explore a little longer now.", A_OR_AN_OR_BLANK(value), c_objinfo[value].shdesc);
	}
	if (wordnumber < wordcount - 1 && words[++wordnumber].value == AND)
	    ++wordnumber;
	else
	    return firstnumber;
    }
    return firstnumber;
}

void kiss (void)
{
    while (words[++wordnumber].type != NOUNS && wordnumber <= wordcount)
	continue;
    // The goddess must be "taken" first if bathing. This will remove her from the bath and move her into the throne room.
    if (words[wordnumber].type == NOUNS && words[wordnumber].value == GODDESS && object_is_at (BATHGOD, position)) {
	words[--wordnumber].value = TAKE;
	process_command();
	return;
    }
    if (!object_is_at (words[wordnumber].value, position))
	puts ("She is not here.");
    else if (words[wordnumber].value == NATIVE_GIRL) {
	++game_score.pleasure;
	puts ("You kiss her.\nHer lips are warm and her body robust. She pulls you down to the ground.");
    } else if (words[wordnumber].value == GODDESS) {
	++game_score.pleasure;
	switch (godready++) {
	    case 0: puts ("You try to kiss her. She avoids your advances."); break;
	    case 1: puts ("You try to kiss her. She leans away with a smile."); break;
	    case 2: puts ("She turns her head and the kiss lands on her cheek."); break;
	    default: puts ("Her lips feel like fire upon yours. You melt with pleasure."); break;
	}
    } else
	puts ("I'd prefer not to.");
}

void love (void)
{
    while (words[++wordnumber].type != NOUNS && wordnumber <= wordcount)
	continue;
    if (!object_is_at (words[wordnumber].value, position))
	puts ("Where's your lover?");
    else if (words[wordnumber].value == NATIVE_GIRL) {
	puts ("The girl peels off her sarong and indulges you.");
	game_score.pleasure += 5;
	printf ("Girl:\n");
	ourtime += 10;
	printf ("Loved.\n");
	zzz();
    } else if (words[wordnumber].value == GODDESS) {
	if (game_state (LOVED_GODDESS))
	    puts ("Loved.");
	else if (godready < 3)
	    puts ("You wish!");
	else {
	    puts ("She cuddles up to you, and speaks softly:\n\n"
		    "'That was my sister's amulet. The lovely goddess Purl, was she. The Empire\n"
		    "captured her just after the Darkness came. My other sister, Vert, was\n"
		    "killed by the Dark Lord himself. He took her amulet and warped its power.\n"
		    "Your quest was foretold by my father before he died, but to get the Dark\n"
		    "Lord's amulet you must use cunning and skill. I will leave you my amulet,\n"
		    "which you may use as you wish. As for me, I am the last goddess of the\n"
		    "waters. My father was the Island King, and the rule is rightfully mine.'\n\n"
		    "She pulls the throne out into a large bed.");
	    game_score.pleasure += 50;
	    ++game_score.power;
	    ++game_score.ego;
	    if (is_injured()) {
		puts ("Her kisses revive you; your wounds are healed.\n");
		cure_all_injuries();
	    }
	    printf ("Goddess:\n");
	    if (!game_state (LOVED_GODDESS)) {
		set_game_state (LOVED_GODDESS);
		create_object_at (MEDALION, position);
	    }
	    ourtime += 10;
	    printf ("Loved.\n");
	    zzz();
	}
    } else
	puts ("It doesn't seem to work.");
}

bool zzz (void)
{
    gametime_t oldtime = ourtime;
    if (snooze - ourtime >= 3*CYCLE/4)
	return false;
    ourtime += 3*CYCLE/4 - (snooze - ourtime);
    printf ("<zzz>");
    for (gametime_t n = 0; n < ourtime - oldtime; ++n)
	printf (".");
    printf ("\n");
    snooze += 3 * (ourtime - oldtime);
    if (game_state (LAUNCHED)) {
	fuel -= min_u (fuel, (ourtime - oldtime));
	if (location[position].dir.down) {
	    position = location[position].dir.down;
	    crash();
	} else
	    clear_game_state (LAUNCHED);
    }
    if (is_outside() && nrand(2)) {
	puts ("You are awakened abruptly by the sound of someone nearby.");
	unsigned c = nrand(4);
	if (c == 0)
	    create_object_at (DEADWOOD, position);
	else if (c == 1)
	    create_object_at (HALBERD, position);
	else if (count_objects_at (INVENTORY)) {
	    unsigned n;
	    do
		n = nrand (NUMOFOBJECTS);
	    while (!object_is_at (n, INVENTORY));
	    move_object_to (n, INVENTORY, position);
	    puts ("A fiendish little Elf is stealing your treasures!");
	    fight (ELF, 10);
	}
    }
    return true;
}

void chime (void)
{
    gametime_t cycletime = (ourtime%CYCLE)*7/CYCLE;
    if (!is_outside())
	puts ("I can't tell the time in here.");
    else if ((ourtime/CYCLE+1)%2) {
	static const char c_day_times[] =
	    "just after sunrise\0"
	    "early morning\0"
	    "late morning\0"
	    "near noon\0"
	    "early afternoon\0"
	    "late afternoon\0"
	    "near sunset";
	printf ("It is %s.\n", zstrn (c_day_times, cycletime, 7));
    } else {
	static const char c_night_times[] =
	    "It is just after sunset.\0"
	    "It is early evening.\0"
	    "The evening is getting old.\0"
	    "It is near midnight.\0"
	    "These are the wee hours of the morning.\0"
	    "The night is waning.\0"
	    "It is almost morning.";
	puts (zstrn (c_night_times, cycletime, 7));
    }
}

int give (void)
{
    int firstnumber = wordnumber;
    while (words[++wordnumber].type != OBJECT
	    && wordnumber <= wordcount
	    && words[wordnumber].value != AMULET
	    && words[wordnumber].value != MEDALION
	    && words[wordnumber].value != TALISMAN)
	continue;
    int obj = -1, person = 0, last1 = 0, last2 = 0;
    if (wordnumber <= wordcount) {
	obj = words[wordnumber].value;
	if (obj == EVERYTHING)
	    words[wordnumber].type = -1;
	last1 = wordnumber;
    }
    wordnumber = firstnumber;
    while ((words[++wordnumber].type != NOUNS || words[wordnumber].value == obj) && wordnumber <= wordcount) {}
    if (words[wordnumber].type == NOUNS) {
	person = words[wordnumber].value;
	last2 = wordnumber;
    }
    // Setting wordnumber to last1 - 1 looks wrong if last1 is 0, e.g.,
    // plain 'give'. However, detecting this case is liable to detect
    // 'give foo' as well, which would give a confusing error. We
    // need to make sure the -1 value can cause no problems if it arises.
    // If in the below we get to the drop("Given") then drop will look
    // at word 0 for an object to give, and fail, which is OK; then
    // result will be -1 and we get to the end, where wordnumber gets
    // set to something more sensible. If we get to "I don't think
    // that is possible" then again wordnumber is set to something
    // sensible. The wordnumber we leave with still isn't right if
    // you include words the game doesn't know in your command, but
    // that's no worse than what other commands than give do in
    // the same place.
    int result = -1;
    wordnumber = last1 - 1;
    if (person && object_is_at (person, position)) {
	if (person == GODDESS && godready < 2 && !(obj == RING || obj == BRACELET))
	    puts ("The goddess won't look at you.");
	else
	    result = drop("Given");
    } else {
	puts ("I don't think that is possible.");
	wordnumber = max_i (last1, last2) + 1;
	return 0;
    }
    if (result != -1 && (object_is_at (obj, position) || obj == AMULET || obj == MEDALION || obj == TALISMAN)) {
	remove_object_from (obj, position);
	++ourtime;
	++game_score.ego;
	switch (person) {
	    case NATIVE_GIRL:
		puts ("She accepts it shyly.");
		game_score.ego += 2;
		break;
	    case GODDESS:
		if (obj == RING || obj == BRACELET) {
		    puts ("She takes the charm and puts it on.\n"
			"A little kiss on the cheek is your reward.");
		    game_score.ego += 5;
		    godready += 3;
		}
		if (obj == AMULET || obj == MEDALION || obj == TALISMAN) {
		    game_score.ego += 5;
		    game_score.power -= min_u (game_score.power, 5);
		    if (++win >= 3) {
			puts ("The powers of the earth are now legitimate. You have destroyed the Darkness\n"
				"and restored the goddess to her throne. The entire island celebrates with\n"
				"dancing and spring feasts. As a measure of her admiration, the goddess weds\n"
				"you in the late summer and crowns you Prince Liverwort, Lord of Fungus.");
			remove_object_from (MEDALION, position);
			live();
		    }
		}
		break;
	    case TIMER:
		if (obj == COINS) {
		    puts ("He fingers the coins for a moment and then looks up agape. 'Kind you are and\n"
			    "I mean to repay you as best I can.' Grabbing a pencil and cocktail napkin...\n\n"
			    "+-----------------------------------------------------------------------------+\n"
			    "|				   xxxxxxxx\\				      |\n"
			    "|				       xxxxx\\	CLIFFS			      |\n"
			    "|		FOREST			  xxx\\				      |\n"
			    "|                          \\\\           x\\               OCEAN         |\n"
			    "|				||	       x\\			      |\n"
			    "|				||  ROAD	x\\			      |\n"
			    "|				||		x\\			      |\n"
			    "|		SECRET		||	  .........			      |\n"
			    "|		 - + -		||	   ........			      |\n"
			    "|		ENTRANCE	||		...     BEACH		      |\n"
			    "|				||		...		  E	      |\n"
			    "|				||		...		  |	      |\n"
			    "|				//		...	    N <-- + --- S     |\n"
			    "|		PALM GROVE     //		...		  |	      |\n"
			    "|			      //		...		  W	      |\n"
			    "+-----------------------------------------------------------------------------+\n"
			    "\n'This map shows a secret entrance to the catacombs.\n"
			    "You will know when you arrive because I left an old pair of shoes there.'");
		}
		break;
	}
    }
    wordnumber = max_i (last1, last2) + 1;
    return firstnumber;
}

bool launch (void)
{
    if (!object_is_at (STARFIGHTER, position) || game_state (CANTLAUNCH)) {
	puts ("Can't launch.");
	return false;
    }
    if (fuel <= 4) {
	puts ("Not enough fuel to launch.");
	return false;
    }
    remove_object_from (STARFIGHTER, position);
    position = location[position].dir.up;
    set_game_state (LAUNCHED);
    ++ourtime;
    fuel -= 4;
    puts ("You climb into the starfighter and prepare for launch.\n"
	"With a touch of your thumb the turbo engines ignite,\n"
	"thrusting you back into your seat.");
    return true;
}

bool land (void)
{
    if (!game_state (LAUNCHED) || !object_is_at (LAND, position) || !location[position].dir.down) {
	puts ("You can't land here.");
	return false;
    }
    clear_game_state (LAUNCHED);
    position = location[position].dir.down;
    create_object_at (STARFIGHTER, position);
    fuel -= min_u (fuel, 2);
    ++ourtime;
    puts ("You land.");
    return true;
}

int drive (void)
{
    if (!object_is_at (CAR, position)) {
	puts ("There is nothing to drive here.");
	return -1;
    }
    puts ("You hop in the car and turn the key.\n"
	    "There is a perceptible grating noise,\n"
	    "and an explosion knocks you unconscious...");
    remove_object_from (CAR, position);
    create_object_at (CRASH, position);
    suffer_injury (BROKEN_ARM);
    suffer_injury (BROKEN_RIBS);
    ourtime += 15;
    zzz();
    return 0;
}

int ride (void)
{
    if (!object_is_at (HORSE, position)) {
	puts ("There is no horse here.");
	return -1;
    }
    puts ("You climb onto the stallion and kick it in the guts.\n"
	    "The indignant steed launches forward through bush and fern.\n"
	    "You are thrown and the horse gallops off.");
    location_t oldpos = position;
    while (!(position = nrand(NUMOFROOMS+1)) || !is_outside()
	    || !visited_location(position) || location[position].dir.flyhere) {}
    move_object_to (HORSE, oldpos, position);
    if (location[position].dir.north)
	position = location[position].dir.north;
    else if (location[position].dir.south)
	position = location[position].dir.south;
    else if (location[position].dir.east)
	position = location[position].dir.east;
    else
	position = location[position].dir.west;
    return 0;
}

// synonyms = {strike, smoke} for matches, cigars
void light (void)
{
    if (!object_is_at (MATCHES, INVENTORY)) {
	puts ("You're out of matches.");
	return;
    }
    if (!matchcount) // reset if picked up another book of matches
	matchcount = 20;
    ++ourtime;
    if (!--matchcount)
	remove_object_from (MATCHES, INVENTORY);
    set_game_state (MATCH_LIGHT);
    puts ("Your match splutters to life.");
    if (position == BUNGALOW_PORCH) {
	puts ("The whole bungalow explodes with an intense blast.");
	die();
    }
}

// synonyms = {open, unlock}
void dooropen (void)
{
    ++wordnumber;
    if (wordnumber > wordcount || words[wordnumber].type != NOUNS || words[wordnumber].value != DOOR)
	puts ("That doesn't open.");
    else if (position == CAVE_DOOR || position == CAVE_ENTRANCE) {
	if (game_state (OPENED_CAVE_DOOR))
	    puts ("The door is already open.");
	else
	    puts ("The door does not budge.");
    } else if (position == KITCHEN_DOOR) {
	if (game_state (OPENED_KITCHEN_DOOR))
	    puts ("The door is gone.");
	else
	    puts ("The door is locked tight.");
    } else if (position == DINING_ROOM_DOOR)
	puts ("That's one immovable door.");
    else if (position == LASER_ROOM)
	puts ("The door is already ajar.");
    else
	puts ("What door?");
}
