// This file is free software, distributed under the BSD license.

#include "battlestar.h"

void write_location_short_description (void)
{
    puts (location[position].name);
}

void write_location_long_description (void)
{
    printf ("\n\t%s\n\n", location[position].name);
    const char* desc = game_state (IS_NIGHT)
	? location[position].night_desc
	: location[position].day_desc;
    unsigned compass = NORTH;
    for (char c; (c = *desc++) != 0;) {
	if (c != '-' && c != '*' && c != '+') {
	    if (c == '=')
		putchar ('-');
	    else
		putchar (c);
	} else {
	    if (c != '*')
		printf ("%s", truedirec (compass, c));
	    ++compass;
	}
    }
    putchar ('\n');
}

location_t relative_destination (enum CommandId relway)
{
    struct room here = location [position];

    // Remove passages blocked by doors
    if (!game_state (OPENED_CAVE_DOOR)) {
	if (position == CAVE_DOOR)
	    here.dir.north = 0;
	else if (position == CAVE_ENTRANCE)
	    here.dir.south = 0;
    } else if (!game_state (OPENED_KITCHEN_DOOR)) {
	if (position == KITCHEN_DOOR)
	    here.dir.west = 0;
	else if (position == KITCHEN_STAIRWELL)
	    here.dir.east = 0;
    }

    // Set relative destinations based on facing direction
    switch (direction) {
	default:
	case NORTH:
	    switch (relway) {
		default:
		case AHEAD:	return here.dir.north;
		case BACK:	return here.dir.south;
		case LEFT:	return here.dir.west;
		case RIGHT:	return here.dir.east;
	    }
	    break;
	case SOUTH:
	    switch (relway) {
		default:
		case AHEAD:	return here.dir.south;
		case BACK:	return here.dir.north;
		case LEFT:	return here.dir.east;
		case RIGHT:	return here.dir.west;
	    }
	    break;
	case EAST:
	    switch (relway) {
		default:
		case AHEAD:	return here.dir.east;
		case BACK:	return here.dir.west;
		case LEFT:	return here.dir.north;
		case RIGHT:	return here.dir.south;
	    }
	    break;
	case WEST:
	    switch (relway) {
		default:
		case AHEAD:	return here.dir.west;
		case BACK:	return here.dir.east;
		case LEFT:	return here.dir.south;
		case RIGHT:	return here.dir.north;
	    }
	    break;
    }
}

const char* truedirec (enum CommandId way, char option)
{
    switch (way) {
	default:
	case NORTH:
	    switch (direction) {
		case NORTH: return "ahead";
		case SOUTH: return option == '+' ? "behind you" : "back";
		case EAST: return "left";
		case WEST: return "right";
	    }; break;
	case SOUTH:
	    switch (direction) {
		case NORTH: return option == '+' ? "behind you" : "back";
		case SOUTH: return "ahead";
		case EAST: return "right";
		case WEST: return "left";
	    }; break;
	case EAST:
	    switch (direction) {
		case NORTH: return "right";
		case SOUTH: return "left";
		case EAST: return "ahead";
		case WEST: return option == '+' ? "behind you" : "back";
	    }; break;
	case WEST:
	    switch (direction) {
		case NORTH: return "left";
		case SOUTH: return "right";
		case EAST: return option == '+' ? "behind you" : "back";
		case WEST: return "ahead";
	    }; break;
    }
    printf("Error: room %d.  More than four directions wanted.", position);
    return "!!";
}

enum CommandId compass_direction (enum CommandId thisway)
{
    switch (direction) {
	default:
	case NORTH:
	    switch (thisway) {
		default:
		case AHEAD: return NORTH;
		case LEFT: return WEST;
		case RIGHT: return EAST;
		case BACK: return SOUTH;
	    }
	    break;
	case SOUTH:
	    switch (thisway) {
		default:
		case AHEAD: return SOUTH;
		case LEFT: return EAST;
		case RIGHT: return WEST;
		case BACK: return NORTH;
	    }
	    break;
	case EAST:
	    switch (thisway) {
		default:
		case AHEAD: return EAST;
		case LEFT: return NORTH;
		case RIGHT: return SOUTH;
		case BACK: return WEST;
	    }
	    break;
	case WEST:
	    switch (thisway) {
		default:
		case AHEAD: return WEST;
		case LEFT: return SOUTH;
		case RIGHT: return NORTH;
		case BACK: return EAST;
	    }
	    break;
    }
}

//{{{ location ---------------------------------------------------------

const struct room location [NUMOFROOMS+1] = {
    {},{ // 1, MAIN_HANGAR
	"You are in the main hangar.",
	"This is a huge bay where many fighters and cargo craft lie. Alarms are \n"
	 "sounding and fighter pilots are running to their ships. Above is a gallery\n"
	 "overlooking the bay. The scream of turbo engines is coming from +. The rest\n"
	 "of the hangar is +. There is an exit +.*",
	"This is a huge bay where many fighters and cargo craft lie. Alarms are \n"
	 "sounding and fighter pilots are running to their ships. Above is a gallery\n"
	 "overlooking the bay. The scream of turbo engines is coming from +. The rest\n"
	 "of the hangar is +. There is an exit +.*",
	{ LAUNCH_ROOM, 9, 2, HANGAR_GALLERY, HANGAR_GALLERY, 0, 0, 1 }
    },{ // 2
	"This is the landing bay.",
	"Ships are landing here, some heavily damaged. Enemy fighters continually\n"
	 "strafe this vulnerable port. The main hangar is +. *\n"
	 "There is an exit +.*",
	"Ships are landing here, some heavily damaged. Enemy fighters continually\n"
	 "strafe this vulnerable port. The main hangar is +. *\n"
	 "There is an exit +.*",
	{ MAIN_HANGAR, 10, 0, 0, 0, 0, 0, 0 }
    },{ // 3, HANGAR_GALLERY
	"You are in the gallery.",
	"From here a view of the entire landing bay reveals that our battlestar\n"
	 "is near destruction. Fires are spreading out of control and laser blasts\n"
	 "lick at the shadows. The control room is +. ***",
	"From here a view of the entire landing bay reveals that our battlestar\n"
	 "is near destruction. Fires are spreading out of control and laser blasts\n"
	 "lick at the shadows. The control room is +. ***",
	{ 4, 0, 0, 0, 0, MAIN_HANGAR, 0, 0 }
    },{ // 4
	"You are in the control room.",
	"Several frantic technicians are flipping switches wildly but otherwise\n"
	 "this room seems fairly deserted. A weapons locker has been left open.\n"
	 "A staircase leads down. * There is a way -. **        ",
	"Several frantic technicians are flipping switches wildly but otherwise\n"
	 "this room seems fairly deserted. A weapons locker has been left open.\n"
	 "A staircase leads down. * There is a way -. **        ",
	{ 0, 0, HANGAR_GALLERY, 0, 0, LAUNCH_ROOM, 0, 0 }
    },{ // 5, LAUNCH_ROOM
	"This is the launch room.",
	"From the launch tubes here fighters blast off into space. Only one is left,\n"
	 "and it is guarded by two fierce men. A staircase leads up from here.\n"
	 "There is a cluttered workbench +. From the main hangar come sounds of great\n"
	 "explosions. The main hangar is +. The starfighter launch tubes are -.*",
	"From the launch tubes here fighters blast off into space. Only one is left,\n"
	 "and it is guarded by two fierce men. A staircase leads up from here.\n"
	 "There is a cluttered workbench +. From the main hangar come sounds of great\n"
	 "explosions. The main hangar is +. The starfighter launch tubes are -.*",
	{ 6, STARFIGHTER_ROOM, MAIN_HANGAR, 0, 4, 0, 0, 1 }
    },{ // 6
	"You are at the workbench.",
	"Strange and unwieldy tools are arranged here including a lunch box \n"
	 "and pneumatic wrenches and turbo sprocket rockets.*\n"
	 "The launch room is +. The remaining starfighter is +.*",
	"Strange and unwieldy tools are arranged here including a lunch box \n"
	 "and pneumatic wrenches and turbo sprocket rockets.*\n"
	 "The launch room is +. The remaining starfighter is +.*",
	{ 0, STARFIGHTER_ROOM, LAUNCH_ROOM, 0, 0, 0, 0, 0 }
    },{ // 7, STARFIGHTER_ROOM
	"You are in the starfighter launch tube.",
	"The two guards are eyeing you warily! ****",
	"The two guards are eyeing you warily! ****",
	{ 0, 0, LAUNCH_ROOM, LAUNCH_ROOM, OUTSIDE_BATTLESTAR, 0, 0, 0 }
    },{ // 8, STATEROOM_CLOSET
	"This is a walk in closet.",
	"A wardrobe of immense magnitude greets the eye. Furs and robes of kings\n"
	 "hang on rack after rack. Silken gowns, capes woven with spun gold, and \n"
	 "delicate synthetic fabrics are stowed here. The bedroom is +.***",
	"A wardrobe of immense magnitude greets the eye. Furs and robes of kings\n"
	 "hang on rack after rack. Silken gowns, capes woven with spun gold, and \n"
	 "delicate synthetic fabrics are stowed here. The bedroom is +.***",
	{ LUXURIOUS_STATEROOM, 0, 0, 0, 0, 0, 0, 0 }
    },{ // 9
	"You are in a wide hallway leading to the main hangar.",
	"The walls and ceiling here have been blasted through in several places.\n"
	 "It looks as if quite a battle has been fought for possession of the landing bay\n"
	 "Gaping corpses litter the floor.** The hallway continues +.\n"
	 "The main hangar is +.",
	"The walls and ceiling here have been blasted through in several places.\n"
	 "It looks as if quite a battle has been fought for possession of the landing bay\n"
	 "Gaping corpses litter the floor.**  The hallway continues +.\n"
	 "The main hangar is +.",
	{ 0, 11, 0, MAIN_HANGAR, 0, 0, 0, 0 }
    },{ // 10
	"You are in a wide hallway leading to the landing bay.",
	"Most of the men and supplies needed in the main hangar come through this\n"
	 "corridor, but the wounded are forced to use it too. It very dank and\n"
	 "crowded here, and the floor is slippery with blood.**\n"
	 "The hallway continues -. The landing bay is +.",
	"Most of the men and supplies needed in the main hangar come through this\n"
	 "corridor, but the wounded are forced to use it too. It very dank and\n"
	 "crowded here, and the floor is slippery with blood.**\n"
	 "The hallway continues -. The landing bay is +.",
	{ 0, 12, 0, 2, 0, 0, 0, 0 }
    },{ // 11
	"The hallway is very congested with rubble here.",
	"It is too choked with broken steel girders and other debris to continue\n"
	 "on much farther. Above, the ceiling has caved in and it is possible to \n"
	 "climb up. There is not much chance to go -, -, or -.\n"
	 "But the hallway seems clearer +.",
	"It is too choked with broken steel girders and other debris to continue\n"
	 "on much farther. Above, the ceiling has caved in and it is possible to \n"
	 "climb up. There is not much chance to go -, -, or -.\n"
	 "But the hallway seems clearer +.",
	{ 0, 0, 0, 9, AMULET_ROOM, 0, 0, 1 }
    },{ // 12
	"A wide hallway and a more narrow walkway meet here.",
	"The intersection is crowded with the many wounded who have come up\n"
	 "the wide hallway and continued +. The walkway is less crowded +.\n"
	 "The wide hallway goes *-.",
	"The intersection is crowded with the many wounded who have come up\n"
	 "the wide hallway and continued +. The walkway is less crowded +.\n"
	 "The wide hallway goes *-.",
	{ 14, 0, 15, 10, 0, 0, 0, 0 }
    },{ // 13, AMULET_ROOM
	"You are in what was once an elegant stateroom.",
	"Whoever lived in this stateroom, he and his female companion\n"
	 "were mercilessly slain in their sleep. Clothes, trinkets and personal\n"
	 "belongings are scattered all across the floor. Through a hole in the\n"
	 "collapsed floor I can see a hallway below. A door is +.***",
	"Whoever lived in this stateroom, he and his female companion\n"
	 "were mercilessly slain in their sleep. Clothes, trinkets and personal\n"
	 "belongings are scattered all across the floor. Through a hole in the\n"
	 "collapsed floor I can see a hallway below. A door is +.***",
	{ 16, 0, 0, 0, 0, 11, 0, 0 }
    },{ // 14
	"You're at the entrance to the sick bay.",
	"The wounded are entering the sick bay in loudly moaning files.\n"
	 "The walkway continues - and +. A doctor is motioning for you to \n"
	 "come to the -. *",
	"The wounded are entering the sick bay in loudly moaning files.\n"
	 "The walkway continues - and +. A doctor is motioning for you to \n"
	 "come to the -. *",
	{ 17, 18, 12, 0, 0, 0, 0, 0 }
    },{ // 15
	"You're in the walkway.",
	"Most of the men and supplies were coming from the armory. The walkway\n"
	 "continues -. The armory is +.**",
	"Most of the men and supplies were coming from the armory. The walkway\n"
	 "continues -. The armory is +.**",
	{ 12, 0, 19, 0, 0, 0, 0, 0 }
    },{ // 16
	"These are the executive suites of the battlestar.",
	"Luxurious staterooms carpeted with crushed velvet and adorned with beaten\n"
	 "gold open onto this parlor. A wide staircase with ivory banisters leads\n"
	 "up or down. This parlor leads into a hallway +. The bridal suite is\n"
	 "+. Other rooms lie - and +.",
	"Luxurious staterooms carpeted with crushed velvet and adorned with beaten\n"
	 "gold open onto this parlor. A wide staircase with ivory banisters leads\n"
	 "up or down. This parlor leads into a hallway +. The bridal suite is\n"
	 "+. Other rooms lie - and +.",
	{ LASER_ROOM, 21, AMULET_ROOM, LUXURIOUS_STATEROOM, 23, 24, 0, 1 }
    },{ // 17
	"You're in a long dimly lit hallway.",
	"This part of the walkway is deserted. There is a dead end +. The\n"
	 "entrance to the sickbay is +. The walkway turns sharply -.*",
	"This part of the walkway is deserted. There is a dead end +. The\n"
	 "entrance to the sickbay is +. The walkway turns sharply -.*",
	{ 0, KITCHEN_STAIRWELL, 14, 0, 0, 0, 0, 0 }
    },{ // 18
	"This is the sick bay.",
	"Sinister nurses with long needles and pitiful aim probe the depths of suffering\n"
	 "here. Only the mortally wounded receive medical attention on a battlestar,\n"
	 "but afterwards they are thrown into the incinerators along with the rest.**\n"
	 "Nothing but death and suffering +. The walkway is +.",
	"Sinister nurses with long needles and pitiful aim probe the depths of suffering\n"
	 "here. Only the mortally wounded receive medical attention on a battlestar,\n"
	 "but afterwards they are thrown into the incinerators along with the rest.**\n"
	 "Nothing but death and suffering +. The walkway is +.",
	{ 0, 0, 0, 14, 0, 0, 0, 0 }
    },{ // 19
	"You're in the armory.",
	"An armed guard is stationed here to protect the magazine.\n"
	 "The walkway is +. The magazine is +.**",
	"An armed guard is stationed here to protect the magazine.\n"
	 "The walkway is +. The magazine is +.**",
	{ 15, 0, 26, 0, 0, 0, 0, 0 }
    },{ // 20, LASER_ROOM
	"The hallway ends here at the presidential suite.",
	"The door to this suite is made from solid magnesium, and the entryway is\n"
	 "inlaid with diamonds and fire opals. The door is ajar +. The hallway\n"
	 "goes -.**",
	"The door to this suite is made from solid magnesium, and the entryway is\n"
	 "inlaid with diamonds and fire opals. The door is ajar +. The hallway\n"
	 "goes -.**",
	{ 27, 0, 16, 0, 0, 0, 0, 0 }
    },{ // 21
	"This is the maid's utility room.",
	"What a gruesome sight! The maid has been brutally drowned in a bucket of\n"
	 "Pine Sol and repeatedly stabbed in the back with a knife.***\n"
	 "The hallway is +.",
	"What a gruesome sight! The maid has been brutally drowned in a bucket of\n"
	 "Pine Sol and repeatedly stabbed in the back with a knife.***\n"
	 "The hallway is +.",
	{ 0, 0, 0, 16, 0, 0, 0, 0 }
    },{ // 22, LUXURIOUS_STATEROOM
	"This is a luxurious stateroom.",
	"The floor is carpeted with a soft animal fur and the great wooden furniture\n"
	 "is inlaid with strips of platinum and gold. Electronic equipment built\n"
	 "into the walls and ceiling is flashing wildly. The floor shudders and\n"
	 "the sounds of dull explosions rumble though the room. From a window in\n"
	 "the wall + comes a view of darkest space. There is a small adjoining\n"
	 "room +, and a doorway +.*",
	"The floor is carpeted with a soft animal fur and the great wooden furniture\n"
	 "is inlaid with strips of platinum and gold. Electronic equipment built\n"
	 "into the walls and ceiling is flashing wildly. The floor shudders and\n"
	 "the sounds of dull explosions rumble though the room. From a window in\n"
	 "the wall + comes a view of darkest space. There is a small adjoining\n"
	 "room +, and a doorway +.*",
	{ 0, 16, STATEROOM_CLOSET, 0, 0, 0, 0, 0 }
    },{ // 23
	"You are at the entrance to the dining hall.",
	"A wide staircase with ebony banisters leads down here.**\n"
	 "The dining hall is -.*",
	"A wide staircase with ebony banisters leads down here.**\n"
	 "The dining hall is -.*",
	{ 0, 28, 0, 0, 0, 16, 0, 0 }
    },{ // 24
	"This was once the first class lounge.",
	"There is much rubble and destruction here that was not apparent elsewhere.\n"
	 "The walls and ceilings have broken in in some places. A staircase with\n"
	 "red coral banisters leads up. It is impossible to go - or -.\n"
	 "It seems a little clearer +.*",
	"There is much rubble and destruction here that was not apparent elsewhere.\n"
	 "The walls and ceilings have broken in in some places. A staircase with\n"
	 "red coral banisters leads up. It is impossible to go - or -.\n"
	 "It seems a little clearer +.*",
	{ 0, 29, 0, 0, 16, 0, 0, 1 }
    },{ // 25, KITCHEN_STAIRWELL
	"You are in a narrow stairwell.",
	"These dusty and decrepit stairs lead up. There is no way -. The\n"
	 "hallway turns sharply -.**",
	"These dusty and decrepit stairs lead up. There is no way -. The\n"
	 "hallway turns sharply -.**",
	{ 0, KITCHEN_DOOR, 17, 0, KITCHEN_DOOR, 0, 0, 1 }
    },{ // 26
	"You are in the magazine.",
	"Rows and rows of neatly stacked ammunition for laser pistols and grenade\n"
	 "launchers are here. The armory is +.***",
	"Rows and rows of neatly stacked ammunition for laser pistols and grenade\n"
	 "launchers are here. The armory is +.***",
	{ 19, 0, 0, 0, 0, 0, 0, 0 }
    },{ // 27
	"You're in the presidential suite.",
	"Apparently the president has been assassinated. A scorched figure lies\n"
	 "face downward on the carpet clutching his chest.*\n"
	 "The hallway leads -.**",
	"Apparently the president has been assassinated. A scorched figure lies\n"
	 "face downward on the carpet clutching his chest.*\n"
	 "The hallway leads -.**",
	{ 0, 0, LASER_ROOM, 0, 0, 0, 0, 0 }
    },{ // 28
	"You are in the dining hall.",
	"This was the scene of a mass suicide. Hundreds of ambassadors and assorted\n"
	 "dignitaries sit slumped over their breakfast cereal. I suppose the news\n"
	 "of the drenian attack killed them. There is a strange chill in this room. I\n"
	 "would not linger here. * The kitchen is +. Entrances + and +.",
	"This was the scene of a mass suicide. Hundreds of ambassadors and assorted\n"
	 "dignitaries sit slumped over their breakfast cereal. I suppose the news\n"
	 "of the drenian attack killed them. There is a strange chill in this room. I\n"
	 "would not linger here. * The kitchen is +. Entrances + and +.",
	{ 0, DINING_ROOM_DOOR, KITCHEN_DOOR, 23, 0, 0, 0, 0 }
    },{ // 29
	"The debris is very thick here.",
	"Broken furniture, fallen girders, and other rubble block the way.\n"
	 "There is not much chance to continue -, -, or -.\n"
	 "It would be best to go -.",
	"Broken furniture, fallen girders, and other rubble block the way.\n"
	 "There is not much chance to continue -, -, or -.\n"
	 "It would be best to go -.",
	{ 0, 0, 11, 24, 0, 0, 0, 0 }
    },{ // 30, KITCHEN_DOOR
	"You are in the kitchen.",
	"This room is full of shining stainless steel and burnished bronze cookware. An \n"
	 "assortment of tropical fruits and vegetables as well as fine meats and cheeses \n"
	 "lies on a sterling platter. The chef, unfortunately, has been skewered like a \n"
	 "side of beef. The dining room is +. ** There is a locked door +.",
	"This room is full of shining stainless steel and burnished bronze cookware. An \n"
	 "assortment of tropical fruits and vegetables as well as fine meats and cheeses \n"
	 "lies on a sterling platter. The chef, unfortunately, has been skewered like a \n"
	 "side of beef. The dining room is +. ** There is a locked door +.",
	{ 28, 0, 0, KITCHEN_STAIRWELL, 0, 0, 0, 0 }
    },{ // 31, DINING_ROOM_DOOR
	"You are in an arched entry leading to the dining room.",
	"The door leading out is bolted shut from the outside and is very strong.***\n"
	 "The dining room is +.",
	"The door leading out is bolted shut from the outside and is very strong.***\n"
	 "The dining room is +.",
	{ 0, 0, 0, 28, 0, 0, 0, 0 }
    },{ // 32, OUTSIDE_BATTLESTAR
	"You are in space.",
	"****",
	"****",
	{ 33, 35, 34, 36, 37, 33, 1, 1 }
    },{ // 33
	"You are in space.",
	"****",
	"****",
	{ 38, 39, OUTSIDE_BATTLESTAR, 40, 41, 42, 1, 1 }
    },{ // 34
	"You are in space.",
	"****",
	"****",
	{ OUTSIDE_BATTLESTAR, 45, 44, 46, 47, 48, 1, 1 }
    },{ // 35
	"You are in space.",
	"****",
	"****",
	{ 40, 49, 45, OUTSIDE_BATTLESTAR, 50, 51, 1, 1 }
    },{ // 36
	"You are in space.",
	"****",
	"****",
	{ 41, OUTSIDE_BATTLESTAR, 46, 52, 53, 54, 1, 1 }
    },{ // 37
	"You are in space.",
	"****",
	"****",
	{ 42, 50, 47, 53, 55, OUTSIDE_BATTLESTAR, 1, 1 }
    },{ // 38
	"You are in space.",
	"****",
	"****",
	{ 43, 51, 48, 54, OUTSIDE_BATTLESTAR, 56, 1, 1 }
    },{ // 39
	"You are in space.",
	"****",
	"****",
	{ 57, 40, 33, 41, 42, 43, 1, 1 }
    },{ // 40
	"You are in space.",
	"****",
	"****",
	{ 39, 57, 35, 33, 58, 59, 1, 1 }
    },{ // 41
	"You are in space.",
	"****",
	"****",
	{ 39, 33, 36, 59, 60, 61, 1, 1 }
    },{ // 42
	"You are in space.",
	"****",
	"****",
	{ 39, 58, 37, 60, 62, 33, 1, 1 }
    },{ // 43
	"You are in space.",
	"****",
	"****",
	{ 39, 59, 38, 61, 33, 63, 1, 1 }
    },{ // 44
	"You are in space.",
	"****",
	"****",
	{ 34, 45, 64, 46, 47, 48, 1, 1 }
    },{ // 45
	"You are in space.",
	"****",
	"****",
	{ 35, 49, 44, 34, 50, 51, 1, 1 }
    },{ // 46
	"You are in space.",
	"****",
	"****",
	{ 36, 34, 44, 52, 53, 54, 1, 1 }
    },{ // 47
	"You are in space.",
	"****",
	"****",
	{ 37, 50, 44, 53, 55, 34, 1, 1 }
    },{ // 48
	"You are in space.",
	"****",
	"****",
	{ 38, 51, 44, 54, 34, 56, 1, 1 }
    },{ // 49
	"You are in space.",
	"****",
	"****",
	{ 49, 52, 49, 35, 49, 49, 1, 1 }
    },{ // 50
	"You are in space.",
	"****",
	"****",
	{ 58, 49, 47, 37, 55, 35, 1, 1 }
    },{ // 51
	"You are in space.",
	"****",
	"****",
	{ 59, 49, 48, 38, 35, 56, 1, 1 }
    },{ // 52
	"You are in space.",
	"****",
	"****",
	{ 52, 36, 52, 49, 52, 52, 1, 1 }
    },{ // 53
	"You are in space.",
	"****",
	"****",
	{ 60, 37, 46, 52, 55, 36, 1, 1 }
    },{ // 54
	"You are in space.",
	"****",
	"****",
	{ 61, 38, 48, 52, 36, 56, 1, 1 }
    },{ // 55
	"You are in space.",
	"****",
	"****",
	{ 62, 55, 55, 55, 56, 37, 1, 1 }
    },{ // 56
	"You are in space.",
	"****",
	"****",
	{ 56, 56, 56, 56, 38, 55, 1, 1 }
    },{ // 57
	"You are in space.",
	"****",
	"****",
	{ 65, 57, 39, 57, 57, 57, 1, 1 }
    },{ // 58
	"You are in space.",
	"****",
	"****",
	{ 39, 49, 50, 42, 62, 40, 1, 1 }
    },{ // 59
	"You are in space.",
	"****",
	"****",
	{ 39, 49, 51, 43, 40, 63, 1, 1 }
    },{ // 60
	"You are in space.",
	"****",
	"****",
	{ 39, 43, 53, 59, 62, 41, 1, 1 }
    },{ // 61
	"You are in space.",
	"****",
	"****",
	{ 39, 43, 54, 59, 41, 56, 1, 1 }
    },{ // 62
	"You are in space.",
	"****",
	"****",
	{ 39, 62, 55, 62, 56, 42, 1, 1 }
    },{ // 63
	"You are in space.",
	"****",
	"****",
	{ 39, 35, 56, 36, 43, 55, 1, 1 }
    },{ // 64
	"You are in space.",
	"****",
	"****",
	{ 44, 66, 66, 66, 66, 66, 1, 1 }
    },{ // 65
	"You are in space.",
	"****",
	"****",
	{ 67, 67, 57, 67, 67, 67, 1, 1 }
    },{ // 66
	"You are in space.",
	"****",
	"****",
	{ 64, ORBITING_PLANET, ORBITING_PLANET, ORBITING_PLANET, ORBITING_PLANET, ORBITING_PLANET, 1, 1 }
    },{ // 67
	"You are orbiting a small blue planet.",
	"****",
	"****",
	{ 67, 67, 67, 67, 65, 69, 1, 1 }
    },{ // 68, ORBITING_PLANET
	"You are orbiting a tropical planet.",
	"****",
	"****",
	{ ORBITING_PLANET, ORBITING_PLANET, ORBITING_PLANET, ORBITING_PLANET, 66, 70, 1, 1 }
    },{ // 69
	"You are flying through a dense fog.",
	"A cold grey sea of mist is swirling around the windshield and water droplets\n"
	 "are spewing from the wingtips. Ominous shadows loom in the darkness and it\n"
	 "feels as if a trap is closing around us. I have lost all sense of direction.\n"
	 "****",
	"A cold grey sea of mist is swirling around the windshield and water droplets\n"
	 "are spewing from the wingtips. Ominous shadows loom in the darkness and it\n"
	 "feels as if a trap is closing around us. I have lost all sense of direction.\n"
	 "****",
	{ 69, 69, 69, 69, 69, 69, 1, 1 }
    },{ // 70
	"You are approaching an island.",
	"Coconut palms, sword ferns, orchids, and other lush vegetation drape this\n"
	 "jagged island carved seemingly from pure emerald and set in a turquoise\n"
	 "sea. The land rises sharply +. There is a nice beach* +.*",
	"Feather palms outlined by mellow moonlight and a silvery black ocean line\n"
	 "the perimeter of the island. Mighty mountains of emerald and amethyst rise\n"
	 "like jagged teeth from black gums. The land rises sharply +. The shore\n"
	 "line stretches on *+.*",
	{ 71, 73, 72, 74, ORBITING_PLANET, 0, 1, 1 }
    },{ // 71
	"You are flying over a mountainous region.",
	"Below is a magnificent canyon with deep gorges, high pinnacles and\n"
	 "waterfalls plummeting hundreds of feet into mist. Everything in sight\n"
	 "is carpeted with a tropical green.* The ocean is +.**",
	"Below is a shadow=filled canyon with looming spires and pinnacles and\n"
	 "luminous waterfalls plummeting down beyond sight. **The ocean is +.*",
	{ 75, 76, 73, 77, ORBITING_PLANET, 0, 1, 1 }
    },{ // 72
	"You are flying over the ocean.",
	"You bank over the water and your wingtips dip low to the green waves. The\n"
	 "sea is very shallow here and the white coral beds beneath us teem with \n"
	 "colorful fish.****",
	"You bank over the water and your wingtips dip low to the green waves. The\n"
	 "sea is very shallow here and the white coral beds beneath us teem with \n"
	 "shadowy fish.****",
	{ 74, 78, 78, 78, ORBITING_PLANET, 0, 1, 1 }
    },{ // 73
	"You are flying over the beach.",
	"A warm gentle surf caresses the white coral beach here. The land rises\n"
	 "sharply +.* The beach is lost in low cliffs and rocks +.*",
	"A warm gentle surf caresses the beach here. The land rises\n"
	 "sharply +.* The beach is lost in low cliffs +.*",
	{ 71, 79, 72, 74, ORBITING_PLANET, 80, 1, 1 }
    },{ // 74
	"You are flying over a large lagoon.",
	"Encircled by a coral reef, the palms and ferns in this sheltered spot\n"
	 "have grown down to the water's very brink which winds tortuously inland.\n"
	 "There looks like a small village +.***",
	"The water's brink winds tortuously inland. There are some lights +.***",
	{ 81, 73, 72, 82, ORBITING_PLANET, 0, 1, 1 }
    },{ // 75
	"You are flying over a gently sloping plane.",
	"This is where several alluvial fans and ancient lava flows have run\n"
	 "together forming a fertile plane choked with vegetation. It would be\n"
	 "impossible to land safely here.* The terrain is more rugged +.**",
	"The ground appears to be choked with vegetation.*  The terrain is more\n"
	 "rugged +.**",
	{ 83, 84, 71, 85, ORBITING_PLANET, 0, 1, 1 }
    },{ // 76
	"You are flying through a gorge.",
	"This narrow, steep sided canyon is lined with waving ferns. The floor is of\n"
	 "light gravel with many freshets pouring from the walls and running along it.\n"
	 "The gorge leads to the sea** +, and to a tumultuous origin +.",
	"This is a narrow, steep sided canyon lined with plants. The stars above\n"
	 "glisten through the over hanging trees. The gorge leads to the\n"
	 "sea** +, and to a tumultuous origin +.",
	{ 0, 86, 0, 71, ORBITING_PLANET, 102, 1, 1 }
    },{ // 77
	"You are flying over a plantation.",
	"Rows of palms, papayas, mangoes, kiwi, as well as smaller fields of sugar\n"
	 "cane and pineapple are growing here. It might be possible to land here, but\n"
	 "I wouldn't advise it.* There looks like two small settlements +     \n"
	 "and *+.",
	"It might be possible to land here, but not in the dark.*  There are some lights\n"
	 "+ and *+.",
	{ 85, 71, 81, 88, ORBITING_PLANET, FIRST_ISLAND_LAND, 1, 1 }
    },{ // 78
	"You are over the ocean.",
	"The deep green swells foam and roll into the shore **+*.",
	"The deep green swells foam and roll into the shore **+*.",
	{ 72, 79, 78, 74, ORBITING_PLANET, 0, 1, 1 }
    },{ // 79
	"You are flying along the coast.",
	"The coastline here is very rocky with little or no sand. The surf in some\n"
	 "places is violent and explodes in a shower of sparkling spray.\n"
	 "There is a winding road below which closely follows the shore. ****",
	"The coastline here is very rocky. The surf in some places is violent\n"
	 "and explodes in a shower of sparkling, moonlit spray. ****",
	{ 86, 90, 72, 73, ORBITING_PLANET, 91, 1, 1 }
    },{ // 80
	"This is a beautiful coral beach.",
	"Fine silver sand kissed lightly by warm tropical waters stretches at least\n"
	 "30 meters here from the ocean to under gently swaying palms +.***",
	"Fine silver sand kissed lightly by warm tropical waters and illuminated\n"
	 "by a huge tropical moon stretches at least 30 meters inland.\n"
	 "Gently swaying palm trees are +.***",
	{ 106, 107, 0, 108, 73, 0, 0, 0 }
    },{ // 81
	"You are flying over a small fishing village.",
	"A few thatched huts a short distance from the water and row of more modern\n"
	 "bungalows on either side of a dirt road are all that is here. The road\n"
	 "continues on ***+.",
	"A few thatched huts lit with torches and bonfires line a road below.\n"
	 "The road continues on ***+.",
	{ 77, 71, 74, 82, ORBITING_PLANET, 92, 1, 1 }
    },{ // 82
	"You are flying over a clearing.",
	"There is a dock here (big enough for a seaplane) leading to a grassy\n"
	 "meadow and a road. Some people are having a party down there. Below is\n"
	 "a good landing site. ****",
	"There is a dock here (big enough for a seaplane) leading to a clearing.\n"
	 "The still waters of the lagoon reflects our orange turbo thrusters.****",
	{ 88, 74, 72, 87, ORBITING_PLANET, DOCK, 1, 1 }
    },{ // 83
	"You are flying over the shore.",
	"Rocky lava flows or coarse sandy beaches are all that is here except for\n"
	 "sparse herbs and weeds.****",
	"Rocky lava flows have overtaken the beach here.****",
	{ 94, 95, 75, 96, ORBITING_PLANET, 0, 1, 1 }
    },{ // 84
	"You are flying in a wide valley.",
	"This is a shallow valley yet the floor is obscured by a thick mist.\n"
	 "The valley opens to the sea +. The mist grows thicker +.**",
	"This is a shallow valley yet the floor is obscured by a thick mist.\n"
	 "The valley opens into blackness +. The mist grows thicker +.**",
	{ 95, 86, 97, 75, ORBITING_PLANET, 98, 1, 1 }
    },{ // 85
	"You are flying near the shore.",
	"Very tall palm trees growing in neatly planted rows march off from the \n"
	 "water here towards the hills and down to the flat lands *+.*\n"
	 "There is a nice beach +.",
	"Very tall trees growing in neatly planted rows march off from the \n"
	 "water here towards the hills and down to the flat lands *+.**",
	{ 96, 75, 77, 99, ORBITING_PLANET, 0, 1, 1 }
    },{ // 86
	"You are flying around the very tip of the island.",
	"There is no beach here for sheer cliffs rise several hundred feet\n"
	 "to a tree covered summit. Far below, the blue sea gnaws voraciously at\n"
	 "the roots of these cliffs. The island bends around +** and +.",
	"Sheer cliffs rise several hundred feet to a tree covered summit. Far below,\n"
	 "the grey sea gnaws voraciously at the roots of these cliffs. The island bends\n"
	 "around +** and +.",
	{ 95, 90, 79, 84, ORBITING_PLANET, 0, 1, 1 }
    },{ // 87
	"You are flying along the coastline.",
	"There is a narrow strip of sand here lined with ferns and shrubs, but very\n"
	 "few trees. The beach is barley wide enough to land on. The beach continues\n"
	 "on -.* There are some buildings +.*",
	"This is a narrow strip of sand lined with very few trees. The stars above\n"
	 "flicker through wisps of clouds. The beach continues on -.* There\n"
	 "are some lights +.*",
	{ 99, 88, 82, 100, ORBITING_PLANET, 101, 1, 1 }
    },{ // 88
	"You are flying over several cottages and buildings",
	"The grounds here are landscaped with palm trees, ferns, orchids, and beds of\n"
	 "flowering plumeria and antheriums. Directly below is a small ornate white\n"
	 "house with a belltower, a lush green lawn, and a spurting fountain.\n"
	 "Small dirt roads go + and +.**",
	"Directly below is small ornate house lit up with spot lights and decorative\n"
	 "bulbs. A bell tower and a spurting fountain are nearby. Small dirt roads go\n"
	 "+ and +.**",
	{ 99, 77, 82, 87, ORBITING_PLANET, 103, 1, 1 }
    },{ // 89, FIRST_ISLAND_LAND
	"You are in a field of sugar cane.",
	"These strong, thick canes give little shelter but many cuts and scrapes.\n"
	 "There are some large trees ***+.",
	"These strong, thick canes give little shelter but many cuts and scrapes.\n"
	 "There are some large trees ***+.",
	{ 109, 111, 110, 112, 77, 0, 0, 0 }
    },{ // 90
	"You are flying over the ocean.",
	"The water is a placid turquoise and so clear that fish and sharks\n"
	 "many fathoms below are clearly visible.****",
	"The water is a placid ebony.****",
	{ 95, 90, 78, 86, ORBITING_PLANET, 0, 1, 1 }
    },{ // 91
	"You are on the coast road.",
	"The road winds close to the shore here and the sound of crashing surf is\n"
	 "deafening.* The water is +. The road continues - and -.",
	"The road winds close to the shore here. The trees on either side press close\n"
	 "in the darkness and seem to be watching us.**  The road continues -\n"
	 "and -.",
	{ 113, 115, ABOVE_SEA_CAVE, 116, 79, 0, 0, 0 }
    },{ // 92
	"You are on the main street of the village.",
	"Thatched roofs and outrigger canoes, palm trees and vacation bungalows, and\n"
	 "comely natives in a tropical paradise all make this a fantasy come true.\n"
	 "There is an open bungalow +.*  The road continues - and -.",
	"The natives are having a festive luau here. Beautiful dancers gyrate in the\n"
	 "torchlight to the rhythm of wooden drums. A suckling pig is sizzling in a\n"
	 "bed of coals and ti leaves are spread with poi and tropical fruits. Several\n"
	 "natives have come over to you and want you to join in the festivities.\n"
	 "There is a light burning in a bungalow +.***",
	{ 117, 119, 118, 120, 81, 0, 0, 0 }
    },{ // 93, DOCK
	"You are at the sea plane dock.",
	"Native girls with skin of gold, clad only in fragrant leis and lavalavas,\n"
	 "line the dockside to greet you. A couple of ukulele=plucking islanders and a\n"
	 "keyboard player are adding appropriate music. A road crosses the clearing \n"
	 "+*. There are some tables set up +.*",
	"The clearing is deserted. The grass is wet with the evening dew +.*\n"
	 "There is something set up +.*",
	{ 121, 123, 122, 124, 82, 0, 0, 0 }
    },{ // 94
	"You are flying over the ocean.",
	"Sea weeds and kelp surge in the waves off shore here. The ocean becomes \n"
	 "much deeper +.***",
	"The black waves surge off shore here. The ocean becomes much calmer +.***",
	{ 94, 95, 83, 96, ORBITING_PLANET, 0, 1, 1 }
    },{ // 95
	"You are flying along the coast.",
	"The land is very low here with a river running into the sea +. There\n"
	 "is a wide valley opening up +. The very tip of the island is +.*",
	"The land is very low here with a river running into the sea +. There\n"
	 "is a wide valley opening up +, but a strange mist is flowing out of it.\n"
	 "The very tip of the island is +.*",
	{ 94, 86, 84, 83, ORBITING_PLANET, 0, 1, 1 }
    },{ // 96
	"You are flying along the coast.",
	"There are some secluded sandy stretches of beach here, but too many rocky\n"
	 "outcroppings of lava to land. There is a nicer beach ***+.",
	"The coast here is cluttered with rocky outcroppings.****",
	{ 94, 83, 85, 99, ORBITING_PLANET, 0, 1, 1 }
    },{ // 97
	"You are lost in a sea of fog.",
	"What have you gotten us into?\n"
	 "I can't see a thing! ****",
	"What have you gotten us into?\n"
	 "I can't see a thing! ****",
	{ 97, 97, 104, 97, 97, 0, 1, 1 }
    },{ // 98
	"You are on a gravel wash.",
	"The sound of cascading water is the background for a diluted chorus of \n"
	 "gurgling, splashing, and enchantingly delicate singing. Great billows\n"
	 "of steam are rising *+.**",
	"It is very dark here. A cool breeze is blowing from +. No moonlight can\n"
	 "reach below a thick canopy of fog above. The sound of cascading water is\n"
	 "coming from +.**",
	{ 125, 127, POOLS, 128, 84, 0, 0, 0 }
    },{ // 99
	"You are flying over a wide beach.",
	"Unlike the leeward beaches, few coconut palms grow here but a well groomed\n"
	 "lawn and garden with traipsing stone walks leads down to the sand.*\n"
	 "There are some buildings +. Some trees are growing +.*",
	"There are some lighted buildings *+. Some trees are growing +.*",
	{ 96, 85, 88, 87, ORBITING_PLANET, 105, 1, 1 }
    },{ // 100
	"You are flying over the ocean.",
	"The sea is a perfectly clear blue with a white sandy bottom. No coral\n"
	 "grows underwater here, but the force of the waves is broken by the steep\n"
	 "incline.****",
	"The black waves surge and splash against the rocky shore.****",
	{ 100, 87, 100, 100, ORBITING_PLANET, 0, 1, 1 }
    },{ // 101
	"You are on a narrow strip of sand.",
	"Rather coarse sand makes this beach very steep and only a few meters wide.\n"
	 "A fresh ocean breeze is rustling the ferns **+.*",
	"Rather coarse sand makes this beach very steep and only a few meters wide.\n"
	 "A fresh ocean breeze is rustling the ferns **+.*",
	{ 129, 131, 130, 0, 87, 0, 0, 0 }
    },{ // 102
	"This is Fern Canyon.",
	"Delicate waving ferns flourish here, suckled by warm water dripping from \n"
	 "every fissure and crevice in the solid rock walls.\n"
	 "The canyon winds **-, and -.",
	"Delicate waving ferns flourish here, suckled by warm water dripping from \n"
	 "every fissure and crevice in the solid rock walls. The stars above sparkle\n"
	 "through a thin mist. The canyon winds **-, and -.",
	{ 0, 132, 0, WIDER_CANYON, 76, 0, 0, 0 }
    },{ // 103
	"This is the front lawn.",
	"There is a small fountain here where the driveway meets the lawn.\n"
	 "Across the driveway, +, is an ornate white house with and elegant \n"
	 "woodworking. The bargeboards are carved with fylfots, the ancient \n"
	 "symbols of luck. Even a bell tower has been built here.*  There is a \n"
	 "road + which turns into the driveway.*",
	"There is a small fountain lighted with green and yellow bulbs here where\n"
	 "the driveway meets the lawn. Across the driveway, +, is an ornate white\n"
	 "house lit with gas lamps. A bell tower here is awash in pale blue.*  There\n"
	 "is a road + which turns into the driveway.*",
	{ 134, 136, 135, 137, 88, 0, 0, 0 }
    },{ // 104
	"You have just crossed the crest of a mountain.",
	"The fog vanished mysteriously as we flew over the crest.*\n"
	 "Far + I can see the ocean.**",
	"The fog vanished mysteriously as we flew over the crest.*\n"
	 "Far + I can see the ocean sparkling in the moonlight.**",
	{ 97, 86, 79, 71, ORBITING_PLANET, 0, 1, 1 }
    },{ // 105
	"You are on a sandy beach.",
	"This is the only good beach on the weather side of the island. Fine coral\n"
	 "sand, a fresh sea breeze, and dramatic surf add to its appeal.**\n"
	 "Stone steps lead to the gardens +.*",
	"Fine coral sand, a fresh sea breeze, and dramatic surf add to this beach's\n"
	 "appeal.**  Stone steps lead to a lighted path in the gardens +.*",
	{ 138, 140, 139, 0, 99, 0, 0, 0 }
    },{ // 106
	"You are among palm trees near the shore.",
	"Arching coconut palms laden with fruit provide a canopy for the glistening\n"
	 "white sand and sparse grasses growing here. The forest grows denser +.\n"
	 "The ocean is +.**",
	"Arching coconut palms laden with fruit provide a canopy for the glistening\n"
	 "white sand and sparse, dew covered grasses growing here. The forest grows\n"
	 "denser +. Crickets are chirping loudly here. The ocean is +.**",
	{ 141, 142, 80, 143, 73, 0, 0, 0 }
    },{ // 107
	"You are walking along the beach.",
	"The warm tropical waters nuzzle your ankles as you walk. Above is a fiercely\n"
	 "blue sky. The slope of the sand is so gentle that two hundred meters\n"
	 "offshore the water is only knee deep.** There are some rocks +.*",
	"The warm tropical waters nuzzle your ankles as you walk. Above is a gorgeous\n"
	 "starscape. The battlestar must be up there somewhere. The slope of the sand\n"
	 "is so gentle that the surf only slides up the sand.** There are some rocks\n"
	 "+.*",
	{ SECRET_THICKET, TIDE_POOLS, 0, 80, 73, 0, 0, 0 }
    },{ // 108
	"You are walking along the beach.",
	"Many beautiful shells have been washed up here including bright yellow \n"
	 "cowries, chocolate colored murex, orange conches, striped tritons and the\n"
	 "deadly cone shells.****",
	"The tide is out very far tonight, and it is possible to explore hidden rocks\n"
	 "and caves not ordinarily accessible. Rich beds of seaweed have been exposed\n"
	 "to the cool night air.****",
	{ 146, 80, 0, 147, 73, 0, 0, 0 }
    },{ // 109
	"You are in a papaya grove.",
	"Green slender trees no taller than three meters bulge with their\n"
	 "orange succulent fruit. There are some tall trees +.***",
	"Slender trees with their large seven lobed leaves bulge with succulent fruit.\n"
	 "There are some tall trees +.***",
	{ 148, 149, FIRST_ISLAND_LAND, 150, 77, 0, 0, 0 }
    },{ // 110
	"You are in a field of pineapple.",
	"The sharp dagger like pineapple leaves can pierce the flesh and hold fast\n"
	 "a skewered victim with tiny barbs.* The field ends +.**",
	"The sharp dagger like pineapple leaves can pierce the flesh and hold fast\n"
	 "a skewered victim with tiny barbs.* The field ends +.**",
	{ FIRST_ISLAND_LAND, 152, 151, 153, 77, 0, 0, 0 }
    },{ // 111
	"You are in a field of kiwi plants.",
	"Round hairy fruit hang from staked vines here. There are some trees +\n"
	 "and +. The field ends in a dirt road +.*",
	"Round hairy fruit hang from staked vines here. There are some trees +\n"
	 "and +. The field ends in a road +.*",
	{ 149, 155, 154, FIRST_ISLAND_LAND, 77, 0, 0, 0 }
    },{ // 112
	"You are in a large grove of coconuts.",
	"These trees are much taller than any growing near the shore plus the fat,\n"
	 "juicy coconuts have been selectively cultivated. The grove continues\n"
	 "+, +, *and +.",
	"These trees are much taller than any growing near the shore and the shadows\n"
	 "are also deeper. It's hard to keep my sense of direction.****",
	{ 150, FIRST_ISLAND_LAND, 153, 156, 77, 0, 0, 0 }
    },{ // 113
	"You are in the woods.",
	"Tropical undergrowth makes the going rough here. Sword ferns give no strong\n"
	 "foot hold and the dangling vines would gladly throttle one. Strange cackling\n"
	 "noises are coming from somewhere +.***",
	"Tropical undergrowth makes the going rough here. Sword ferns give no strong\n"
	 "foot hold and the dangling vines would gladly throttle one. The darkness is\n"
	 "so intense here that we stand in utter blackness.****",
	{ 157, 158, 91, 116, 79, 0, 0, 0 }
    },{ // 114, ABOVE_SEA_CAVE
	"You are at the shore.",
	"Explosions of surf jetting out of underwater tunnels here make it\n"
	 "impossible to climb down to a small cave entrance below. Only at rare\n"
	 "minus tides would it be possible to enter.*** The beach is better +.",
	"The low minus tide tonight might make it possible to climb down to a\n"
	 "small cave entrance below. Large rocks would usually churn the waves\n"
	 "asunder.***  The beach goes -.",
	{ 91, ALONG_THE_SHORE, 0, TIDE_POOLS, 79, SEA_CAVE_ENTRANCE, 0, 0 }
    },{ // 115
	"You are on the coast road.",
	"The road is beginning to turn inland.* I can hear the surf +. The road\n"
	 "continues +.*",
	"The road is beginning to turn slightly -. I can hear the surf +. The road\n"
	 "continues into the dark forest +.*",
	{ 158, COAST_ROAD_TURN, 161, 91, 79, 0, 0, 0 }
    },{ // 116
	"The road winds deeper into the trees.",
	"Only narrow sunbeams filter through the foliage above. The moist rich earth\n"
	 "has nurtured a myriad of trees, shrubs, and flowers to grow here. The\n"
	 "road continues - and *- from here.*",
	"Only narrow moonbeams filter through the dense foliage above. The moist rich\n"
	 "earth has nurtured a myriad of slugs, snakes, and spiders to grow here. The\n"
	 "road continues - and *- into the shadows.*",
	{ 163, 91, 142, 164, 79, 0, 0, 0 }
    },{ // 117
	"This is the front porch of the bungalow.",
	"These wooden steps and porch are very bucolic. A little woven mat on the \n"
	 "doorstep reads \"Don't Tread on Me\". The open front door is +.\n"
	 "A stone walk leads to the main street +.**",
	"The veranda is lit by a small yellow bug light. The door leads -.\n"
	 "The stone walk down to the luau is lined with burning torches +. That\n"
	 "roast pig smells good.**",
	{ 165, 0, 92, 0, 81, 0, 0, 0 }
    },{ // 118
	"You are on a path leading to the lagoon.",
	"This path trampled fern, grass, sapling, and anything else that got in its\n"
	 "way.* The water is +.**",
	"This path winds through the underbrush and towards the lagoon *+. The\n"
	 "broad faced moon peeps though the branches above. The sound of drums echos\n"
	 "in the woods.**",
	{ 92, 167, 166, 168, 81, 0, 0, 0 }
    },{ // 119
	"This is a dirt road.",
	"**The road continues on - here for some distance. A village is +.",
	"**The road continues on - here for some distance. A bonfire and party light\n"
	 "up the night sky +.",
	{ 169, 170, 118, 92, 81, 0, 0, 0 }
    },{ // 120
	"You are on a dirt road.",
	"**There is a small village +. The road continues +.",
	"**There is a village +. A huge bonfire licks at the trees, and a celebration\n"
	 "of some sort is going on there. The smell of luscious cooking is tantalizing\n"
	 "my flared nostrils. The road continues +.",
	{ 171, 92, 118, BRIDGE_OVER_LAGOON, 81, 0, 0, 0 }
    },{ // 121
	"You are on a dirt road.",
	"The light tan soil of the road contrasts artistically with the lush green\n"
	 "vegetation and searing blue sky.*  There is a clearing and many people +.\n"
	 "The road continues - and -.",
	"This is a wide grassy clearing bedewed with droplets of evening mist. The\n"
	 "trees alongside the road moan and whisper as we pass. They seem annoyed at\n"
	 "our presence. **The road continues - and -.",
	{ 173, 174, DOCK, 175, 82, 0, 0, 0 }
    },{ // 122
	"You are at the seaplane dock.",
	"Several muscular, bronze skinned men greet you warmly as you pass under\n"
	 "a thatched shelter above the dock here. Polynesian hospitality.\n"
	 "There is a clearing +.* A trail runs around the lagoon + and +.",
	"Not a living thing stirs the calm surface of the lagoon. The wooden planks\n"
	 "creak unnaturally as we tread on them. The dock reaches a clearing +.\n"
	 "A dark trail leads around the lagoon **+.",
	{ DOCK, 176, 0, 177, 82, 0, 0, 0 }
    },{ // 123
	"There are some tables on the lawn here.",
	"Hors d'oeuvres, canapes, mixed drinks, and various narcotic drugs along with\n"
	 "cartons of Di Gel fill the tables to overflowing. Several other guests are\n"
	 "conversing and talking excitedly****.",
	"Some tables are strewn on the wet lawn.****",
	{ 121, 123, 122, DOCK, 82, 0, 0, 0 }
    },{ // 124
	"You are nosing around in the bushes.",
	"There is little here but some old beer cans. You are making fools out of\n"
	 "us in front of the other guests.** It would be best to go -.*",
	"There is little here but some old beer cans. It is damp and dirty in here.\n"
	 "I think I stepped in something unpleasant. It would be best to go **-.*",
	{ 124, DOCK, 124, 124, 82, 0, 0, 0 }
    },{ // 125
	"You are walking in a dry stream bed.",
	"The large cobblestones are difficult to walk on. No sunlight reaches\n"
	 "below a white canopy of fog seemingly generated from *+. A dirt path \n"
	 "along the wash is +. A high bank is impossible to climb +.",
	"The large cobblestones are difficult to walk on. No starlight reaches\n"
	 "below a black canopy of fog seemingly engulfing the whole island. A dirt\n"
	 "path along the wash is **+. The high bank is impossible to climb +.",
	{ 178, 179, 98, 0, 84, 0, 0, 0 }
    },{ // 126, POOLS
	"You are at the thermal pools.",
	"Several steaming fumaroles and spluttering geysers drenched by icy mountain\n"
	 "waters from a nearby waterfall heat half a dozen natural pools to a\n"
	 "delicious 42 degrees. Enchantingly beautiful singing seems to flow from the\n"
	 "water itself as it tumbles down the falls.*** There is a mossy entrance\n"
	 "to a cave +.",
	"Odd spluttering and belching water splashes up around the rocks here.\n"
	 "A spectacular waterfall nearby tumbles down as a river of effervescent\n"
	 "bubbles. The air is quite warm and a cave entrance ***+ spews steam.",
	{ 98, 180, 0, 181, 84, 0, 0, 0 }
    },{ // 127
	"You are in the woods.",
	"Coniferous trees girded by wild huckleberries, elderberries, salmonberries\n"
	 "and thimbleberries enjoy a less tropical climate here in the high mountains.\n"
	 "*The sound of rushing water is coming from +.**",
	"It is pitch black in the forest here and my pant leg is caught on something.\n"
	 "There may be poison oak here. What was that?  A lantern just flickered by in\n"
	 "the dark!  The sound of rushing water is coming from *+.**",
	{ 127, 182, 180, 98, 84, 0, 0, 0 }
    },{ // 128
	"You are on a dirt trail.",
	"The trail seems to start here and head -.** High cliffs border the \n"
	 "trail +.",
	"The trail seems to start here and head towards the forest +.**  High,\n"
	 "dark cliffs border the trail +. Some crickets are chirping\n"
	 "noisily.",
	{ 179, 98, 181, 0, 84, 0, 0, 0 }
    },{ // 129
	"You are  walking along the beach.",
	"A rather unnerving surf explodes onto the beach here and dashes itself into\n"
	 "spray on the steep incline. The beach continues + and +.**",
	"The surf is rather tame tonight. The beach continues + and +.**",
	{ 183, 184, 101, 0, 87, 0, 0, 0 }
    },{ // 130
	"You are walking along the beach.",
	"This is not a very nice beach. The coarse sand hurts my feet.****",
	"This is not a very nice beach. The coarse sand hurts my feet.****",
	{ 101, 186, 185, 0, 87, 0, 0, 0 }
    },{ // 131
	"You are walking through some ferns.",
	"This is a wide field growing only ferns and small shrubs.** The \n"
	 "ocean is *+.",
	"This is a wide field growing only ferns and small shrubs.**  In the dark\n"
	 "it would be all to easy to stumble into a venomous snake. The ocean is\n"
	 "*+.",
	{ 184, 187, 186, 101, 87, 0, 0, 0 }
    },{ // 132
	"You are in a narrow canyon.",
	"The steep sides here squeeze a little freshet through a gauntlet like\n"
	 "series of riffles and pools.****",
	"The steep sides here squeeze a little freshet through a gauntlet like\n"
	 "series of riffles and pools. The cool mountain air is refreshing.****",
	{ 0, 188, 0, 102, 76, 0, 0, 0 }
    },{ // 133, WIDER_CANYON
	"The canyon is much wider here.",
	"The sheer rock walls rise 10 meters to the forest above. A slender \n"
	 "waterfall careens away from the face of the rock high above and showers\n"
	 "the gravel floor with sparkling raindrops.** The canyon continues -\n"
	 "and -.",
	"The sheer rock walls rise 10 meters into darkness. A slender waterfall\n"
	 "careens away from the face of the rock high above and showers the gravel\n"
	 "floor with sparkling raindrops.**  The canyon continues -\n"
	 "and -.",
	{ 0, 102, 0, CAVE_DOOR, 76, 0, 0, 0 }
    },{ // 134
	"You are on the front porch of the cottage.",
	"Several giggling native girls came running down the steps as you approached\n"
	 "and headed on down the road. On the fern rimmed porch is a small table with\n"
	 "matching white wrought iron chairs cushioned with red velvet. The front\n"
	 "door leads -. The lawn and fountain are +.**",
	"The veranda is deserted. A table and chair are the only things on the porch.\n"
	 "Inside the house is a parlor lighted with an elegant chandelier. The door\n"
	 "leads -. The lawn and fountain are +.**",
	{ 190, 0, 103, 0, 0, 0, 0, 0 }
    },{ // 135
	"You are in a palm grove.",
	"****",
	"Crickets are chirping in the cool night air.****",
	{ 103, 192, 191, 105, 88, 0, 0, 0 }
    },{ // 136
	"You are on a dirt road.",
	"There is a large village +. The road cleaves a coconut plantation +.\n"
	 "A small dirt road goes -, and a driveway peels off +.",
	"There are many bright lights +. The road cleaves the darkness +.\n"
	 "A small dirt road goes -, and a driveway peels off +.",
	{ 193, 245, 192, 103, 88, 0, 0, 0 }
    },{ // 137
	"You are in a field of small shrubs.",
	"**Pine and other coniferous saplings have been planted here. The rich brown\n"
	 "soil is well tilled and watered. Across a large lawn, there is a small\n"
	 "cottage +. I can feel a delicious sea breeze blowing from +.",
	"**Pine and other coniferous saplings are growing here. The rich brown\n"
	 "soil is well watered. Across a large lawn +, there is a small cottage lighted\n"
	 "with spot lights and gas lamps. A cool land breeze is blowing.*",
	{ 184, 103, 186, 187, 88, 0, 0, 0 }
    },{ // 138
	"The beach is pretty rocky here.",
	"Dangerous surf and lava outcroppings make this a treacherous strand.\n"
	 "The beach is nicer* +.**",
	"The tide is very low tonight. The beach is nicer *+.**",
	{ 194, 195, 105, 0, 96, 0, 0, 0 }
    },{ // 139
	"The beach is almost 10 meters wide here.",
	"The sand has become more coarse and the beach steeper.* It gets \n"
	 "worse +.**",
	"The sand has become more coarse and the beach steeper.****",
	{ 105, 196, 183, 0, 99, 0, 0, 0 }
    },{ // 140
	"You are in the gardens.",
	"Lush green lawns studded with palms and benches stretch as far as the eye\n"
	 "can see.** A path leads -. Stone steps lead down to the beach +.",
	"Shadowy expanses of lawn and leaf have been groomed and manicured here.\n"
	 "The night sky is glowing with a full moon.**  A lighted path leads -.\n"
	 "Stone steps lead down to the beach +.",
	{ 195, GARDEN, 196, 105, 99, 0, 0, 0 }
    },{ // 141
	"You are on the coast road.",
	"The forest is dense on either side and conceals the road from anyone\n"
	 "approaching it.**  The road continues - and -.",
	"The forest is dense on either side. The trees seem to be actually squeezing\n"
	 "together to keep us from passing. A feeling of enmity is in the air.**\n"
	 "The road continues - and -.",
	{ 198, 163, 106, 199, 73, 0, 0, 0 }
    },{ // 142
	"You are in the forest.",
	"There are trees and ferns all around.****",
	"I suppose there are trees and ferns all around, but it is too dark to see.****",
	{ 116, 91, 107, 106, 73, 0, 0, 0 }
    },{ // 143
	"You are in the forest.",
	"There are trees and ferns all around.****",
	"There are shadowy trees and ferns all around.****",
	{ 199, 106, 108, 146, 73, 0, 0, 0 }
    },{ // 144, SECRET_THICKET
	"You are in a copse.",
	"This is a secret hidden thicket only noticeable from the beach. Someone\n"
	 "has been digging here recently.****",
	"This is a secret hidden thicket only noticeable from the beach. In the\n"
	 "moonlight, I can tell that someone has been digging here recently.****",
	{ 142, TIDE_POOLS, 107, 80, 0, 0, 0, 0 }
    },{ // 145, TIDE_POOLS
	"You are at the tide pools.",
	"These rocks and pools are the home for many sea anemones and crustaceans.\n"
	 "**The surf is very rough +. There is a nice beach +.",
	"These rocks and pools are the home for many sea anemones and crustaceans.\n"
	 "They are exposed because of the low tide. There is a beach ***+.",
	{ 91, ABOVE_SEA_CAVE, 0, 107, 79, 0, 0, 0 }
    },{ // 146
	"You are in the forest.",
	"This is a shallow depression sheltered from the wind by a thick growth of \n"
	 "thorny shrubs. It looks like someone has camped here. There is a fire pit\n"
	 "with some dry sticks and grass nearby.* The beach is +.* The thorny\n"
	 "shrubs block the way -.",
	"This is a shallow depression sheltered from the wind by a thick growth of \n"
	 "thorny shrubs. It looks like someone is camping here. There is a fire pit\n"
	 "with warm, crackling flames and coals here.*  The beach is +.*  The thorny\n"
	 "shrubs block the way -.",
	{ 199, 143, 108, 0, 73, 0, 0, 0 }
    },{ // 147
	"You are at the mouth of the lagoon.",
	"The beach ends here where the coral reef rises to form a wide lagoon\n"
	 "bending inland. A path winds around the lagoon to the -.*\n"
	 "The beach continues on -. Only water lies +.",
	"The beach ends here where the coral reef rises to form a wide lagoon.\n"
	 "A path winds around the lagoon to the -.* The beach continues\n"
	 "on -. Only water lies +.",
	{ 200, 108, 0, IN_THE_LAGOON, 74, 0, 0, 0 }
    },{ // 148
	"You are in a breadfruit grove.",
	"The tall trees bend leisurely in the breeze, holding many round breadfruits\n"
	 "close to their large serrated leaves. There are coconut palms +,\n"
	 "*+, and +.",
	"The tall trees bend leisurely in the breeze, holding many round breadfruits\n"
	 "close to their large serrated leaves. There are coconut palms +,\n"
	 "*+, and +.",
	{ 202, 203, 109, 204, 77, 0, 0, 0 }
    },{ // 149
	"You are in a grove of mango trees.",
	"The juicy yellow red fruits are nearly ripe on the trees here. There are\n"
	 "some coconut palms +. There are some vines +. There is a road +.*",
	"The trees are not tall enough to obscure the view and the bright moonlight\n"
	 "makes it fairly easy to see.****",
	{ 203, 205, 111, 109, 77, 0, 0, 0 }
    },{ // 150
	"You are in a grove of coconut palms.",
	"All I can see around us are palm trees.****",
	"All I can see around us are trees and ominous shapes darting in and out of the\n"
	 "shadows.****",
	{ 204, 109, 112, 206, 77, 0, 0, 0 }
    },{ // 151
	"You are in a coconut grove.",
	"There are countless trees here.****",
	"There are countless trees here.****",
	{ 110, 208, 207, 209, 77, 0, 0, 0 }
    },{ // 152
	"You are in a field of pineapple.",
	"The sharp leaves are cutting me to ribbons. There is a road **+.\n"
	 "More pineapple +.",
	"The sharp leaves are cutting me to ribbons. There is a road **+.*",
	{ 154, 210, 208, 110, 77, 0, 0, 0 }
    },{ // 153
	"You are in a coconut grove.",
	"There is a field of pineapple **+.*",
	"There is a field of something **+.*",
	{ 112, 110, 209, 211, 77, 0, 0, 0 }
    },{ // 154
	"You are on the edge of a kiwi and pineapple field.",
	"An irrigation ditch separates the two fields here. There is a road **+.*",
	"An irrigation ditch separates the two fields here. There is a road **+.*",
	{ 111, 155, 152, 110, 77, 0, 0, 0 }
    },{ // 155
	"This is a dirt road.",
	"The road runs - and - here.**",
	"The road runs - and - here. It is very dark in the forest.**",
	{ 205, 212, 210, 111, 77, 0, 0, 0 }
    },{ // 156
	"You are in a palm grove.",
	"There are palm trees all around us.****",
	"There are trees all around us.****",
	{ 206, 112, 211, 213, 77, 0, 0, 0 }
    },{ // 157
	"You are on the edge of a small clearing.",
	"The ground is rather marshy here and darting in and out of the many tussocks\n"
	 "is a flock of wild chicken like fowl.****",
	"The ground is rather marshy here and the darkness is intense. A swarm of\n"
	 "ravenous mosquitoes has descended upon you and has sent you quaking to your\n"
	 "knees.****",
	{ 157, 157, 113, 157, 79, 0, 0, 0 }
    },{ // 158
	"You are in the woods.",
	"You have walked a long way and found only trees. ****",
	"You have walked a long way and found only spider webs. ****",
	{ 158, ROAD_TURNAROUND, 115, 113, 79, 0, 0, 0 }
    },{ // 159, ALONG_THE_SHORE
	"You are walking along the shore.",
	"You are now about 10 meters above the surf on a gently rising cliffside.**\n"
	 "The land rises +. There is a beach far +.",
	"You are now about 10 meters above the surf on a gently rising cliffside.**\n"
	 "The land rises +. There is a beach far +.",
	{ 115, ALONG_HIGH_CLIFFS, 0, ABOVE_SEA_CAVE, 86, 0, 0, 0 }
    },{ // 160, SEA_CAVE_ENTRANCE
	"You are just inside the entrance to the sea cave.",
	"The sound of water dripping in darkness and the roar of the ocean just outside\n"
	 "create a very unwelcoming atmosphere inside this cave. Only on rare occasions\n"
	 "such as this is it possible to enter the forbidden catacombs... The cave\n"
	 "continues -.***",
	"The sound of water dripping in darkness and the roar of the ocean just outside\n"
	 "create a very unwelcoming atmosphere inside this cave. Only on rare occasions\n"
	 "such as this is it possible to enter the forbidden catacombs... The cave\n"
	 "continues -.***",
	{ 246, 0, ABOVE_SEA_CAVE, 0, ABOVE_SEA_CAVE, 0, 0, 1 }
    },{ // 161
	"You are in a secret nook beside the road.",
	"Hidden from all but the most stalwart snoopers are some old clothes, empty\n"
	 "beer cans and a trash baggie full of used Huggies and ordure. Lets get\n"
	 "back to the road +.***",
	"This little thicket is hidden from the road in the shadows of the forest.\n"
	 "From here we have a clear view of any traffic along the road. A great hollow\n"
	 "tree stuffed with something is nearby. The road is +.***",
	{ 115, COAST_ROAD_TURN, ALONG_THE_SHORE, 91, 79, 0, 0, 0 }
    },{ // 162, COAST_ROAD_TURN
	"You are on the coast road.",
	"The road turns abruptly - here, avoiding the cliffs near the shore\n"
	 "+ and +.*",
	"The road turns abruptly - here, wandering deeper into the black forest.***",
	{ ROAD_TURNAROUND, 0, ALONG_HIGH_CLIFFS, 115, 86, 0, 0, 0 }
    },{ // 163
	"You are on a dirt road.",
	"The roadside is choked with broad leaved plants fighting for every breath of\n"
	 "sunshine. The palm trees are taller than at the shore yet bend over the road \n"
	 "forming a canopy. The road continues *- and *-.",
	"We are walking through a tunnel of unfriendly trees and shrubs. The tall\n"
	 "ones bend over the roadway and reach down with their branches to grab us.\n"
	 "Broad leafed plants at the roadside whisper in the darkness. Something\n"
	 "just darted across the road and into the bushes *+. Let's go *-.",
	{ 216, 113, 116, 141, 79, 0, 0, 0 }
    },{ // 164
	"You have discovered a hidden thicket near the road.",
	"Stuffed into a little bundle here is a bloody silken robe and many beer cans.\n"
	 "*Some droplets of blood and a major spill sparkle farther +.\n"
	 "The road is +.*",
	"I would think it best to stay on the road. The forest seems very unfriendly\n"
	 "at night. The road is **+.*",
	{ 163, 116, 142, 106, 73, 0, 0, 0 }
    },{ // 165
	"You are in the living room.",
	"A decorative entry with fresh flowers and wall to wall carpeting leads into\n"
	 "the living room here where a couch and two chairs converge with an end table.\n"
	 "*The exit is +.* The bedroom is +.",
	"A decorative entry with fresh flowers and wall to wall carpeting leads into\n"
	 "the living room here where a couch and two chairs converge with an end table.\n"
	 "*The exit is +.* The bedroom is +.",
	{ 0, BUNGALOW_PORCH, 117, BUNGALOW_BEDROOM, 0, 0, 0, 0 }
    },{ // 166
	"You are at the lagoon.",
	"There are several outrigger canoes pulled up on a small beach here and a\n"
	 "catch of colorful fish is drying in the sun. There are paths leading \n"
	 "off -*, -, and -.",
	"A small beach here is deserted except for some fishing nets. It is very\n"
	 "peaceful at the lagoon at night. The sound of native drums is carried on\n"
	 "the night breeze. There are paths leading off into darkness +,\n"
	 "*+, and +.",
	{ 118, 167, 0, 168, 81, 0, 0, 0 }
    },{ // 167
	"You are at the lagoon.",
	"This is a grassy little spot near the water. A sightly native girl is frolicking\n"
	 "in the water close to shore here.** The path continues - and -. ",
	"The grass near the water is moist with the refreshing evening dew. Far away,\n"
	 "drums reverberate in the forest.**  The path continues + and +.",
	{ 118, 170, 0, 166, 81, 0, 0, 0 }
    },{ // 168
	"You are at the lagoon.",
	"The path meanders through tussocks of grass, ferns, and thorny bushes here\n"
	 "and continues on **- and -.",
	"The path meanders through shadows of tussocks of grass, ferns, and thorny\n"
	 "bushes here and continues on **- and -.",
	{ 118, 166, 0, BRIDGE_OVER_LAGOON, 81, 0, 0, 0 }
    },{ // 169
	"You are in the woods.",
	"There are plenty of ferns and thorny bushes here! ****",
	"There are plenty of ferns and thorny bushes here! Spider webs and probing\n"
	 "branches snare us as we stumble along in the pitch black night.****",
	{ 219, 220, 119, 92, 81, 0, 0, 0 }
    },{ // 170
	"You are on a dirt road.",
	"The road winds rather close to a large lagoon here and many sedges and tall\n"
	 "grasses line the shoulder *+. The road continues - and -.",
	"The road winds rather close to a large lagoon here and many sedges and tall\n"
	 "loom in the darkness *+. The road continues - and -.",
	{ 220, 199, 167, 119, 74, 0, 0, 0 }
    },{ // 171
	"You are in the woods beside the road.",
	"The forest grows darker +. The road is +.**",
	"The forest grows darker +. The road is +.**",
	{ 221, 92, 120, 222, 81, 0, 0, 0 }
    },{ // 172, BRIDGE_OVER_LAGOON
	"The road crosses the lagoon here.",
	"Coursing through the trees, the road at this point bridges a watery finger\n"
	 "of the lagoon.* The water is +. The road continues - and -.",
	"Strange mists rising from the water engulf a rickety old enclosed bridge here.\n"
	 "Spider webs catch our hair as we pass through its rotting timbers. I felt\n"
	 "something drop on my neck. The road delves into the accursed forest\n"
	 "**+ and +.",
	{ 222, 120, 0, 174, 81, 0, 0, 0 }
    },{ // 173
	"You are in a coconut palm grove.",
	"The tall palms are planted about 30 feet apart with a hardy deep green grass\n"
	 "filling the spaces in between. There are tire tracks through the grass. The\n"
	 "grove continues -. There is a road +.**",
	"The tall palms are planted about 30 feet apart and the starry sky is clearly\n"
	 "visible above. A low growing grass carpets the ground all around. The grove\n"
	 "continues +.***",
	{ 223, END_OF_THE_ROAD, 121, 225, 82, 0, 0, 0 }
    },{ // 174
	"You are walking along a dirt road.",
	"You are nearing the lagoon.** The road continues - and -.",
	"You are near misty patch of the roadway **+. The road continues -.",
	{ END_OF_THE_ROAD, BRIDGE_OVER_LAGOON, 176, 121, 82, 0, 0, 0 }
    },{ // 175
	"You are on a dirt road.",
	"The road turns abruptly - here, entering a grove of palm trees.* The road\n"
	 "also continues - toward the lagoon.*",
	"The road turns abruptly - here, splitting a grove of palm trees.* In the\n"
	 "starlight I can also discern that the road continues - toward the lagoon.*",
	{ 225, 121, 177, 226, 82, 0, 0, 0 }
    },{ // 176
	"You are on a trail running around the lagoon.",
	"The dark waters brush the trail here and the path crosses a bridge +.\n"
	 "There is deep water + and +. The trail continues -.",
	"The dark waters brush the trail here and the path crosses an old  bridge\n"
	 "+. There is deep water + and +. The trail continues -.",
	{ BRIDGE_OVER_LAGOON, 0, 0, 122, 82, 0, 0, 0 }
    },{ // 177
	"This is the mouth of the lagoon.",
	"The coral reef wraps around a natural bay here to create a wide lagoon which\n"
	 "winds tortuously inland.** A trail goes around the lagoon +. The beach\n"
	 "is +.",
	"The coral reef wraps around a natural bay here to create a wide lagoon which\n"
	 "winds tortuously inland.**  A trail goes around the lagoon +.\n"
	 "The beach is -.",
	{ 175, 122, 0, 227, 82, 0, 0, 0 }
    },{ // 178
	"You are in a dry stream bed.",
	"The dry wash drains over a tall precipice here into a turbid morass below. The\n"
	 "most noisome stench imaginable is wafting up to defile our nostrils. Above,\n"
	 "the lurid sun glows brown through a strange mist.* The only direction \n"
	 "I'm going is -.**",
	"The dry wash drains over a tall precipice here into a turbid morass below. The\n"
	 "most noisome stench imaginable is wafting up to defile our nostrils. Above,\n"
	 "the blackness is intense and a strange mist engulfs the island.*  Let's go\n"
	 "-.**",
	{ 0, 0, 125, 0, 84, 0, 0, 0 }
    },{ // 179
	"You are on a dirt path along the wash.",
	"This path looks more like a deer trail. It scampers away ***+.",
	"The trail winds along the gravel wash and delves into the forest ***+.",
	{ 0, 125, 128, 228, 84, 0, 0, 0 }
    },{ // 180
	"The thermal pools flow into a stream here.",
	"The gurgling hot waters pour over boulders into a swiftly flowing\n"
	 "stream **+. The pools are +.",
	"The gurgling hot waters pour over boulders into a swiftly flowing\n"
	 "stream **+. The pools are +.",
	{ 127, CAVE_STREAM_BANK, 0, POOLS, 84, 0, 0, 0 }
    },{ // 181
	"You are at the entrance to a cave.",
	"A tall narrow fissure in the rock cliffs here has become a well traveled\n"
	 "passage way. A hoof beaten dirt path leads directly into it. A curl of\n"
	 "steam is trailing from a corner of the fissure's gaping mouth. The path\n"
	 "leads - and -. The pools are +.*",
	"A torch lights the entrance to the cave. Deep inside I can see shadows moving.\n"
	 "A path goes + from here. The entrance is +.**",
	{ 128, POOLS, 230, 0, 84, 0, 0, 0 }
    },{ // 182
	"You are in the woods.",
	"Wild berry bushes plump with fruit and thorns tangle your every effort to\n"
	 "proceed.* The sound of rushing water is +.**",
	"Thorns tangle your every effort to proceed.*  The sound of rushing water is\n"
	 "+.**",
	{ 182, 182, CAVE_STREAM_BANK, 127, 84, 0, 0, 0 }
    },{ // 183
	"You are walking along the beach.",
	"Some dunes here progress inland and make it impossible to get very far in that\n"
	 "direction. The beach continues - and -.* The ocean is +.",
	"Some dunes here progress inland and make it impossible to get very far in that\n"
	 "direction. The beach continues - and -.*  The ocean is +.",
	{ 139, 184, 129, 0, 99, 0, 0, 0 }
    },{ // 184
	"You are in the dunes.",
	"The endless rolling and pitching sand dunes are enough to make one very queasy!\n"
	 "The only way I'm going is ***+.",
	"The endless rolling and pitching sand dunes are enough to make one very queasy!\n"
	 "The sand is cool and the stars are bright at the ocean. The only way I'm going\n"
	 "is ***+.",
	{ 183, 184, 101, 129, 87, 0, 0, 0 }
    },{ // 185
	"This is a lousy beach.",
	"Volcanic and viciously sharp bitted grains of sand here bite like cold steel\n"
	 "into my tender feet. I refuse to continue on. Let's get out of here. The\n"
	 "beach is better +.***",
	"Volcanic and viciously sharp bitted grains of sand here bite like cold steel\n"
	 "into my tender feet. I refuse to continue on. Let's get out of here. The\n"
	 "beach is better +.***",
	{ 130, 0, 0, 0, 87, 0, 0, 0 }
    },{ // 186
	"You are in a field of sparse ferns.",
	"The lava rock outcroppings here will support few plants. There is more \n"
	 "vegetation +. There is a nice beach +.* The ocean is +.",
	"The lava rock outcroppings here will support few plants. There is more \n"
	 "vegetation +.**  The ocean is +.",
	{ 131, 187, 185, 130, 87, 0, 0, 0 }
    },{ // 187
	"You are in the woods.",
	"Young trees and tall shrubs grow densely together at this distance from the \n"
	 "shore.** The trees grow thicker +.*",
	"Young trees and tall shrubs grow densely together here.\n"
	 "They grow thicker **+.*",
	{ 131, 137, 131, 131, 87, 0, 0, 0 }
    },{ // 188
	"The canyon is no wider than a foot here.",
	"The freshet is gushing through the narrow trough, but the canyon has grown\n"
	 "too narrow to follow it any farther.*** I guess we'll have to go -.",
	"The freshet is gushing through the narrow trough, but the canyon has grown\n"
	 "too narrow to follow it any farther.***  I guess we'll have to go -.",
	{ 0, 0, 0, 132, 0, 0, 0, 0 }
    },{ // 189, CAVE_DOOR
	"You are in a narrow part of the canyon.",
	"The two sheer sides are no more than a few meters apart here. There is a stone\n"
	 "door in the wall +. The gravelly floor runs with tiny rivulets seeping \n"
	 "from the ground itself.* The canyon continues - and -.",
	"The two sheer sides are no more than a few meters apart here. There is a stone\n"
	 "door in the wall +. The gravelly floor runs with tiny rivulets seeping \n"
	 "from the ground itself.* The canyon continues - and -.",
	{ CAVE_ENTRANCE, WIDER_CANYON, 0, HUGE_CHASM, 76, 0, 0, 0 }
    },{ // 190
	"You are in the drawing room.",
	"Exquisitely decorated with plants and antique furniture of superb\n"
	 "craftsmanship, the parlor reflects its owner's impeccable taste. The tropical\n"
	 "sun is streaming in through open shutters *+. There doesn't seem \n"
	 "to be anybody around. A large immaculate oaken desk is visible in the\n"
	 "study and it even has a old fashioned telephone to complete the decor.**",
	"Exquisitely decorated with plants and antique furniture of superb\n"
	 "craftsmanship, the parlor reflects its owner's impeccable taste. The tropical\n"
	 "night air pours in through open shutters *+. There doesn't seem \n"
	 "to be anybody around. A large immaculate oaken desk is visible in the\n"
	 "study and it even has a old fashioned telephone to complete the decor.**",
	{ 0, 0, 134, 0, 0, 0, 0, 0 }
    },{ // 191
	"You are in a palm grove.",
	"Grassy rows of palms stretch as far as I can see.** There is a road +.*",
	"Grassy rows of dew covered palms stretch as far as I can see.**\n"
	 "There is a road +.*",
	{ 135, 233, 191, 191, 88, 0, 0, 0 }
    },{ // 192
	"You are on a dirt road.",
	"The road winds through a coconut palm grove here. It continues on - \n"
	 "and -.**",
	"The road winds through a coconut palm grove here. It continues on - \n"
	 "and -.**",
	{ 136, 234, 233, 135, 88, 0, 0, 0 }
    },{ // 193
	"The road leads to several large buildings here.",
	"There is a clubhouse +,* a large barn and stable +, and a garage of \n"
	 "similar construct to the barn +.",
	"There is a lighted clubhouse +,* a large barn and stable +, and a\n"
	 "garage of similar construct to the barn +.",
	{ 235, 236, 136, 237, 88, 0, 0, 0 }
    },{ // 194
	"This part of the beach is impassable.",
	"The huge rocks and thunderous surf here would pound our frail bodies to pulp\n"
	 "in an instant.* The only direction I'm going is -.**",
	"The see is calm tonight. The beach goes *-.**",
	{ 0, 0, 138, 0, 96, 0, 0, 0 }
    },{ // 195
	"You are in the gardens.",
	"So much green grass is a pleasure to the eyes.****",
	"Dew beaded grass sparkles in the moonlight. Tiny lamps beside the path light\n"
	 "the way to the ocean ***+.",
	{ 195, GARDEN, 140, 138, 96, 0, 0, 0 }
    },{ // 196
	"You are in the gardens.",
	"Beautiful flowers and shrubs surround a little goldfish pond.****",
	"Beautiful flowers and shrubs surround a lighted goldfish pond.****",
	{ 140, GARDEN, 183, 139, 99, 0, 0, 0 }
    },{ // 197, GARDEN
	"You are on a stone walk in the garden.",
	"The walk leads to a road **+.*",
	"The walk leads to a road **+.*",
	{ 195, 238, 196, 140, 99, 0, 0, 0 }
    },{ // 198
	"You are in the forest near the road.",
	"There are many thorny bushes here!****",
	"There are many thorny bushes here!****",
	{ 198, 216, 141, 198, 73, 0, 0, 0 }
    },{ // 199
	"You are at a fork in the road.",
	"Two roads come together in the forest here. One runs -,* the other \n"
	 "runs - and -.",
	"Two roads come together in the darkness here. One runs -,* the other \n"
	 "runs - and -.",
	{ 239, 141, 146, 170, 73, 0, 0, 0 }
    },{ // 200
	"You are on a dirt path around the lagoon.",
	"The still waters reflect bending palms and a cloudless sky. It looks like\n"
	 "the path runs into a clearing +. The path continues -.**",
	"The still waters reflect bending palms and a starry sky. It looks like\n"
	 "the path runs into a clearing +. The path continues -.**",
	{ 170, 146, 147, 0, 74, 0, 0, 0 }
    },{ // 201, IN_THE_LAGOON
	"You are drowning in the lagoon.",
	"I suggest you get out before you become waterlogged.****",
	"I suggest you get out before you become waterlogged.****",
	{ IN_THE_LAGOON, 147, IN_THE_LAGOON, IN_THE_LAGOON, 74, 0, 0, 0 }
    },{ // 202
	"You are in a coconut palm grove.",
	"****",
	"****",
	{ 202, 203, 148, 204, 77, 0, 0, 0 }
    },{ // 203
	"You are in a palm grove.",
	"****",
	"****",
	{ 202, 205, 149, 148, 77, 0, 0, 0 }
    },{ // 204
	"You are in a palm grove.",
	"****",
	"****",
	{ 202, 148, 150, 206, 77, 0, 0, 0 }
    },{ // 205
	"You are on a dirt road.",
	"*This road ends here at a palm grove but continues on - for quite\n"
	 "some way.**",
	"*This road ends here at a palm grove but continues on - for quite\n"
	 "some way.**",
	{ 203, 212, 155, 149, 77, 0, 0, 0 }
    },{ // 206
	"You are in a coconut palm grove.",
	"****",
	"****",
	{ 204, 150, 156, 213, 77, 0, 0, 0 }
    },{ // 207
	"You are in a coconut grove.",
	"*The grove ends +.**",
	"*The grove ends +.**",
	{ 151, 208, 219, 209, 77, 0, 0, 0 }
    },{ // 208
	"You are in a coconut grove.",
	"**There is a dirt road +.*",
	"**There is a dirt road +.*",
	{ 152, 239, 207, 151, 77, 0, 0, 0 }
    },{ // 209
	"You are in a coconut grove.",
	"****",
	"****",
	{ 153, 151, 207, 211, 77, 0, 0, 0 }
    },{ // 210
	"This is a dirt road.",
	"The road continues - and -.**",
	"The road continues - and -.**",
	{ 205, 212, 239, 154, 77, 0, 0, 0 }
    },{ // 211
	"You are in a coconut grove.",
	"****",
	"****",
	{ 153, 153, 209, 213, 77, 0, 0, 0 }
    },{ // 212
	"You are in the woods near the road.",
	"There are many thorny bushes here!****",
	"There are many thorny bushes here!****",
	{ 205, 212, 210, 155, 77, 0, 0, 0 }
    },{ // 213
	"You are in a coconut grove.",
	"***The grove ends in a clearing +.",
	"***The grove ends in a clearing +.",
	{ 213, 156, 213, 234, 88, 0, 0, 0 }
    },{ // 214, ALONG_HIGH_CLIFFS
	"You are walking along some high cliffs.",
	"The island bends sharply + here with high cliffs -\n"
	 "and -. The cliffs are lower +.",
	"The island bends sharply + here with high cliffs -\n"
	 "and -. The cliffs are lower +.",
	{ COAST_ROAD_TURN, 0, 0, ALONG_THE_SHORE, 86, 0, 0, 0 }
    },{ // 215, ROAD_TURNAROUND
	"You are at the coast road turn around.",
	"The coast road ends here in a lookout with a view of 100 kilometers of blue\n"
	 "sea and 100 meters of rugged cliff. Far below the waves crash against rocks.\n"
	 "****",
	"The coast road ends here in a lookout with a view of the ocean.\n"
	 "Far below, the waves crash against the rocks.\n"
	 "****",
	{ 0, 0, COAST_ROAD_TURN, 158, 90, 0, 0, 0 }
    },{ // 216
	"You are in the woods near the road.",
	"These thorny bushes are killing me.****",
	"These thorny bushes are killing me.****",
	{ 216, 216, 163, 198, 79, 257, 0, 0 }
    },{ // 217, BUNGALOW_PORCH
	"You are in the kitchen.",
	"A small gas stove and a refrigerator are all the only appliances here. The\n"
	 "gas oven has been left on and the whole room is reeking with natural gas.\n"
	 "One spark from a match and.... The door out is ***+.",
	"A small gas stove and a refrigerator are all the only appliances here. The\n"
	 "gas oven has been left on and the whole room is reeking with natural gas.\n"
	 "One spark from a match and.... The door out is ***+.",
	{ 0, 0, 0, 165, 0, 0, 0, 0 }
    },{ // 218, BUNGALOW_BEDROOM
	"You are in the bedroom.",
	"A soft feather comforter on top of layers of Answer blankets make this a very\n"
	 "luxurious place to sleep indeed. There are also some end tables and a dresser\n"
	 "here.** The living room is +.*",
	"A soft feather comforter on top of layers of Answer blankets make this a very\n"
	 "luxurious place to sleep indeed. There are also some end tables and a dresser\n"
	 "here.** The living room is +.*",
	{ 0, 165, 0, 0, 0, 0, 0, 0 }
    },{ // 219
	"You are in the woods.",
	"There seems to be a clearing +.***",
	"The darkness is intense, but there seems to be a clearing +.***",
	{ 207, 220, 169, 221, 81, 0, 0, 0 }
    },{ // 220
	"You are in the woods near the road.",
	"*As far as I can tell, there are two roads + and +.*",
	"*As far as I can tell, there are two roads + and +.*",
	{ 219, 239, 170, 169, 81, 0, 0, 0 }
    },{ // 221
	"You are in the woods.",
	"The forest is clearer +.***",
	"The spider webs thin out and the forest is clearer +.***",
	{ 207, 219, 171, 222, 81, 0, 0, 0 }
    },{ // 222
	"You are on the lagoon's inland finger.",
	"It is impossible to follow the lagoon any farther inland because of sharp\n"
	 "and very painful sedges.* The road is +.**",
	"It is impossible to follow the lagoon any farther inland because of sharp\n"
	 "and very painful sedges.* The road is +.**",
	{ 0, 171, BRIDGE_OVER_LAGOON, BRIDGE_OVER_LAGOON, 81, 0, 0, 0 }
    },{ // 223
	"You are in a grassy coconut grove.",
	"The tall palms provide a perfect canopy for the lush green grass.***\n"
	 "There is a road +.",
	"The tall palms provide a ghostly canopy for the sandy ground covering.****",
	{ 240, END_OF_THE_ROAD, 173, 241, 82, 0, 0, 0 }
    },{ // 224, END_OF_THE_ROAD
	"You are near the lagoon's inland finger.",
	"Very sharp sedges make it impossible to follow the lagoon any farther inland.\n"
	 "*There is a road +.**",
	"Very sharp sedges make it impossible to follow the lagoon any farther inland.\n"
	 "*There is a road +.**",
	{ 0, 0, 174, 173, 82, 0, 0, 0 }
    },{ // 225
	"You are on a dirt road.",
	"The road winds through a coconut grove here and continues - and -.**",
	"The road winds through a coconut grove here and continues - and -.**",
	{ 241, 173, 175, 226, 82, 0, 0, 0 }
    },{ // 226
	"You are in the woods near the road.",
	"**The road is +.*",
	"**The road is +.*",
	{ 226, 175, 226, 226, 82, 0, 0, 0 }
    },{ // 227
	"This is a beach?",
	"Hard jagged rocks that pierce with every footstep hardly comprise a beach.**\n"
	 "Let's go -.*",
	"Hard jagged rocks that pierce with every footstep hardly comprise a beach.**\n"
	 "Let's go -.*",
	{ 227, 177, 227, 0, 82, 0, 0, 0 }
    },{ // 228
	"The trail is lost in the woods here.",
	"I suppose the animals that use this trail all depart in different directions\n"
	 "when they get this far into the woods.** The trail goes -.*",
	"The trail goes **-.*",
	{ 241, 179, 241, 241, 84, 0, 0, 0 }
    },{ // 229, CAVE_STREAM_BANK
	"You are on the bank of a stream.",
	"The stream falls over several small boulders here and continues on **-.*",
	"The stream falls over several small boulders here and continues on **-.*",
	{ 182, CLIFF_WATERFALL, 0, 180, 84, 0, 0, 0 }
    },{ // 230
	"You are just inside the cave.",
	"A steamy hot breath is belching from the depths of the earth within.* The\n"
	 "cave  continues -.**",
	"A steamy hot breath is belching from the depths of the earth within.* The\n"
	 "cave  continues -.**",
	{ 181, 0, 267, 0, 0, 0, 0, 0 }
    },{ // 231, CAVE_ENTRANCE
	"You are just inside the cave entrance.",
	"The air is hot and sticky inside. The cave continues -. There is a \n"
	 "stone door in the wall +. A wooden sign in the dust reads in old elven\n"
	 "runes, \"GSRF KDIRE NLVEMP!\".**",
	"The air is hot and sticky inside. The cave continues -. There is a \n"
	 "stone door in the wall +. A wooden sign in the dust warns in old elven\n"
	 "runes, \"GSRF KDIRE NLVEMP!\".**",
	{ 274, 0, CAVE_DOOR, 0, 0, 0, 0, 0 }
    },{ // 232, HUGE_CHASM
	"You are at the edge of a huge chasm.",
	"Several hundred feet down I can see the glimmer of placid water. The\n"
	 "rivulets drain over the edge and trickle down into the depths. It is \n"
	 "impossible to climb down without a rope.** The canyon continues -.*",
	"Several hundred feet down I can see the glimmer of placid water. The\n"
	 "rivulets drain over the edge and trickle down into the depths. It is \n"
	 "impossible to climb down.**  The canyon continues -.*",
	{ 0, CAVE_DOOR, 0, 0, 76, 0, 0, 0 }
    },{ // 233
	"You are on a dirt road.",
	"The road winds through a coconut grove here. The road continues on -\n"
	 "and -.**",
	"The road winds through a coconut grove here. The road continues on into the\n"
	 "shadows - and -.**",
	{ 192, 240, 241, 191, 88, 0, 0, 0 }
    },{ // 234
	"You are in a coconut palm grove near the road.",
	"***The road is +.",
	"***The road is +.",
	{ 193, 213, 233, 192, 88, 0, 0, 0 }
    },{ // 235
	"You are at the clubhouse.",
	"The clubhouse is built over the most inland part of the lagoon. Tropical\n"
	 "bananas and fragrant frangipani grow along the grassy shore. Walking across\n"
	 "the short wooden bridge, we enter. Along one wall is a bar with only a few\n"
	 "people seated at it. The restaurant and dance floor are closed off with\n"
	 "a 2 inch nylon rope. ****",
	"The clubhouse is built over the most inland part of the lagoon. Tropical\n"
	 "bananas and fragrant frangipani grow along the grassy shore. Walking across\n"
	 "the short wooden bridge, we enter. Along one wall is a bar crowded with people.\n"
	 "The restaurant and disco dance floor are filled to capacity. A rock group\n"
	 "electrocutes itself to the satisfaction of the audience.****",
	{ 0, 0, 193, 0, 0, 0, 0, 0 }
    },{ // 236
	"You are in the stables.",
	"Neighing horses snacking on hay and oats fill the stalls on both sides of\n"
	 "the barn. It is rather warm in here but that is not the most offensive\n"
	 "part. The old boards of the barn part just enough to let in dust laden\n"
	 "shafts of light. Flies swarm overhead and strafe the ground for dung.\n"
	 "My nose is beginning to itch. ****",
	"Neighing horses snacking on hay and oats fill the stalls on both sides of\n"
	 "the barn. It is rather warm in here but that is not the most offensive\n"
	 "part.****",
	{ 0, 0, 0, 193, 0, 0, 0, 0 }
    },{ // 237
	"You are in the old garage.",
	"This is an old wooden building of the same vintage as the stables. Beneath\n"
	 "a sagging roof stand gardening tools and greasy rags. Parked in the center\n"
	 "is an underpowered Plymouth Volare' with a red and white striped golf cart\n"
	 "roof. ****",
	"This is an old wooden building of the same vintage as the stables. Beneath\n"
	 "a sagging roof stand gardening tools and greasy rags. Parked in the center\n"
	 "is an underpowered Plymouth Volare' with a red and white striped golf cart\n"
	 "roof. ****",
	{ 0, 193, 0, 0, 0, 0, 0, 0 }
    },{ // 238
	"You are on a dirt road.",
	"The road leads to a beautiful formal garden laced with stone walks and tropical\n"
	 "flowers and trees.** The road continues -. A walk leads -.",
	"The road leads to a formal garden laced with lighted stone walks and tropical\n"
	 "flowers and trees.** The road continues -. A walk leads -.",
	{ GARDEN, 243, GARDEN, GARDEN, 85, 0, 0, 0 }
    },{ // 239
	"You are on a dirt road.",
	"The road runs - and -.**",
	"The road runs - and -.**",
	{ 210, 198, 199, 220, 73, 0, 0, 0 }
    },{ // 240
	"You are in a coconut grove near the road.",
	"***The road is +.",
	"***The road is +.",
	{ 234, 234, 223, 233, 88, 0, 0, 0 }
    },{ // 241
	"You are on a dirt road.",
	"The road continues - and -.**",
	"The road continues - and -.**",
	{ 233, 223, 225, 226, 82, 0, 0, 0 }
    },{ // 242, CLIFF_WATERFALL
	"The stream plummets over a cliff here.",
	"Falling 10 agonizing meters into spray, only droplets of the stream are\n"
	 "left to dance off the floor below. I thought I saw a sparkle of gold\n"
	 "at the bottom of the falls, but now it is gone. There is no way down,\n"
	 "even with a strong rope. ****",
	"Falling 10 agonizing meters into darkness, only droplets of the stream must\n"
	 "be left to dance off the floor below. There is no way down, even with a\n"
	 "strong rope. ****",
	{ 182, 0, 0, CAVE_STREAM_BANK, 84, 0, 0, 0 }
    },{ // 243
	"You are on a dirt road.",
	"**The road continues - and -.",
	"**The road continues - and -.",
	{ 0, 244, 0, 238, 85, 0, 0, 0 }
    },{ // 244
	"You are on a dirt road.",
	"*The road continues -* and -.",
	"*The road continues -* and -.",
	{ 0, 0, 245, 243, 88, 0, 0, 0 }
    },{ // 245
	"You are on a dirt road.",
	"The road goes -* and *-.",
	"The road goes -* and *-.",
	{ 244, 213, 234, 136, 88, 0, 0, 0 }
    },{ // 246
	"You are in a low passage.",
	"The passage is partially flooded here and it may be hazardous to proceed.\n"
	 "Water is surging from the tunnel and heading out to sea. Strange moaning\n"
	 "noises rise above the rushing of the water. They are as thin as a whispering\n"
	 "wind yet penetrate to my very soul. I think we have come too far...\n"
	 "The passage continues -.***",
	"The ceiling here sparkles with iridescent gems and minerals. Colorful starfish\n"
	 "and sea anemones cling to the slippery walls and floor. The passage continues\n"
	 "+.***",
	{ 247, 0, SEA_CAVE_ENTRANCE, 0, 0, 0, 0, 0 }
    },{ // 247
	"The walls are very close together here.",
	"I can barely squeeze through the jagged opening. Slimy sea weeds provide\n"
	 "no footing at all. This tunnel seems to be an ancient lava tube. There is\n"
	 "a large room -.***",
	"I can barely squeeze through the jagged opening. Slimy sea weeds provide\n"
	 "no footing at all. This tunnel seems to be an ancient lava tube. There is\n"
	 "a large room +.***",
	{ 248, 0, 246, 0, 0, 0, 0, 0 }
    },{ // 248
	"You are in the cathedral room.",
	"Your light casts ghostly shadows on the walls but cannot pierce the \n"
	 "engulfing darkness overhead. The sound of water dripping echoes in the void.\n"
	 "*I can see no passages leading out of this room. We have definitely\n"
	 "come too far.*** ",
	"Your light casts ghostly shadows on the walls but cannot pierce the \n"
	 "engulfing darkness overhead. The sound of water dripping echoes in the void.\n"
	 "*I can see no passages leading out of this room.*** ",
	{ 249, 249, 251, 251, 0, 0, 0, 0 }
    },{ // 249
	"You are walking through a very round tunnel.",
	"The round walls of this tunnel are amazingly smooth to the touch. A little\n"
	 "trickle of water flows down the center. The tunnel climbs steadily +.\n"
	 "The cave is beginning to flood again!  Let's get out of here! ***",
	"The round walls of this tunnel are amazingly smooth to the touch. A little\n"
	 "trickle of water flows down the center. The tunnel climbs steadily +.\n"
	 "There is a large room +.**",
	{ 252, 0, 248, 0, 252, 0, 0, 1 }
    },{ // 250
	"You are in the cathedral anteroom.",
	"This small chamber with a flat stone floor is to one side of the cathedral \n"
	 "room. We appear to be at the bottom of a tall narrow shaft. There are many \n"
	 "puddles of water here. A staircase hewn from solid rock and black lava \n"
	 "leads up.*** The cathedral room is +.",
	"This small chamber with a flat stone floor is to one side of the cathedral \n"
	 "room. We appear to be at the bottom of a tall narrow shaft. There are many \n"
	 "puddles of water here. A staircase hewn from solid rock and black lava \n"
	 "leads up.***  The cathedral room is -.",
	{ 0, 0, 0, 248, 253, 0, 0, 1 }
    },{ // 251
	"You are in a wide chamber.",
	"Water is sprinkling from the ceiling here. A shallow pool populated by a \n"
	 "myriad of blind white creatures sparkles in your light. Tiny shrimp and\n"
	 "crabs scurry away, frightened by the blinding rays.** The cave \n"
	 "continues - and -.",
	"Water is sprinkling from the ceiling here. A shallow pool populated by a \n"
	 "myriad of blind white creatures sparkles in your light. Tiny shrimp and\n"
	 "crabs scurry away, frightened by the blinding rays.**  The cave \n"
	 "continues + and +.",
	{ 0, 248, 0, 254, 0, 0, 0, 0 }
    },{ // 252
	"You are at the top of a sloping passage.",
	"There is much algae growing here, both green and brown specimens. \n"
	 "Water from an underground sea surges and splashes against the slope of\n"
	 "the rock. The walls glisten with shiny minerals. High above, light\n"
	 "filters in through a narrow shaft.**  A hallway here runs -\n"
	 "and -.",
	"There is much algae growing here, both green and brown specimens. I suspect\n"
	 "that we are near the high tide zone, but no light can get in here. The walls\n"
	 "glisten with shiny minerals.**  A hallway here runs + and -.",
	{ 0, 255, 249, 256, 257, 249, 0, 1 }
    },{ // 253
	"You are in an elaborately tiled room.",
	"Large colorful tiles plate the floor and walls. The ceiling is a mosaic\n"
	 "of gems set in gold. Hopefully it is only our footsteps that are echoing in\n"
	 "this hollow chamber.** The room continues -. A stone staircase\n"
	 "leads down.*",
	"Large colorful tiles plate the floor and walls. The ceiling is a mosaic\n"
	 "of gems set in gold. Hopefully it is only our footsteps that are echoing in\n"
	 "this hollow chamber.**  The room continues -. A stone staircase leads\n"
	 "down.*",
	{ 0, 258, 0, 0, 0, 250, 0, 0 }
    },{ // 254
	"You are at a dead end.",
	"The walls here are alive with dark mussels. They click their shells menacingly\n"
	 "if we disturb them. ** The only exit is +.*",
	"The walls here are alive with dark mussels. They click their shells menacingly\n"
	 "if we disturb them.**  The only exit is +.*",
	{ 0, 251, 0, 0, 0, 0, 0, 0 }
    },{ // 255
	"The tunnel is very low here.",
	"I practically have to crawl on my knees to pass through this opening. The\n"
	 "air is stiflingly damp, but I can't hear any sounds of water dripping.**\n"
	 "The crawlspace continues -. The tunnel seems wider +.",
	"You practically have to crawl on your knees to pass through this opening. The\n"
	 "air is stiflingly damp, but you can't hear any sounds of water dripping.**\n"
	 "The crawlspace continues -. The tunnel seems wider +.",
	{ 0, 259, 0, 252, 0, 0, 0, 0 }
    },{ // 256
	"This is the supply room.",
	"Picks and shovels line the walls here, as well as hard hats, boxes of\n"
	 "dynamite, and a cartload of very high grade gold and silver ore.** \n"
	 "A tunnel leads off +.*",
	"Picks and shovels line the walls here, as well as hard hats, boxes of\n"
	 "dynamite, and a cartload of very high grade gold and silver ore.** \n"
	 "A tunnel leads off +.*",
	{ 0, 252, 0, 0, 0, 0, 0, 0 }
    },{ // 257
	"You have found a secret entrance to the catacombs.",
	"I have a sickening feeling that we should not have entered the catacombs.\n"
	 "Below is a wet, seaweed covered floor. Above is a way out. ****",
	"Below is a wet, seaweed covered floor. Above is a way out.****",
	{ 0, 0, 0, 0, 216, 252, 0, 1 }
    },{ // 258
	"You are in the catacombs.",
	"Ornate tombs and piles of treasure line the walls. Long spears with many\n"
	 "blades, fine swords and coats of mail, heaps of coins, jewelry, pottery, \n"
	 "and golden statues are tribute of past kings and queens.** The catacombs\n"
	 "continue - and -.",
	"Ornate tombs and piles of treasure line the walls. Long spears with many\n"
	 "blades, fine swords and coats of mail, heaps of coins, jewelry, pottery, \n"
	 "and golden statues are tribute past kings and queens.** The catacombs\n"
	 "continue - and -.",
	{ 0, 260, 0, 253, 0, 0, 0, 0 }
    },{ // 259
	"You are crawling on your stomach.",
	"The passage is quite narrow and jagged, but the rock is no longer lava.\n"
	 "It appears to be a form of granite.** The crawlspace continues -, \n"
	 "but I would just as soon go -.",
	"The passage is quite narrow and jagged, but the rock is no longer lava.\n"
	 "It appears to be a form of granite.**  The crawlspace continues -, \n"
	 "but I would just as soon go -.",
	{ 0, 261, 0, 255, 0, 0, 0, 0 }
    },{ // 260
	"You are in the Sepulcher.",
	"A single tomb is here. Encrusted with diamonds and opals, and secured with \n"
	 "straps of a very hard, untarnished silver, this tomb must be of a great king.\n"
	 "Vases overflowing with gold coins stand nearby. A line of verse on the wall\n"
	 "reads, \"Three he made and gave them to his daughters.\"****",
	"A single tomb is here. Encrusted with diamonds and opals, and secured with \n"
	 "straps of a very hard, untarnished silver, this tomb must be of a great king.\n"
	 "Vases overflowing with gold coins stand nearby. A line of verse on the wall\n"
	 "reads, \"Three he made and gave them to his daughters.\"****",
	{ 0, 0, 0, 258, 0, 0, 0, 0 }
    },{ // 261
	"The passage is wider here.",
	"You are at the top of a flooded shaft. About a meter below the edge,\n"
	 "dark water rises and falls to the rhythm of the sea. A ladder goes\n"
	 "down into water here.***  A small crawlspace goes -.",
	"A ladder goes down into darkness here.***  A small crawlspace goes -.",
	{ 0, 0, 0, 259, 0, 262, 0, 0 }
    },{ // 262
	"You are at the bottom of a ladder.",
	"This is a narrow platform to rest on before we continue either up or down this\n"
	 "rickety wooden ladder.****",
	"This is a narrow platform to rest on before we continue either up or down this\n"
	 "rickety wooden ladder.****",
	{ 0, 0, 0, 0, 261, 263, 0, 1 }
    },{ // 263
	"You are standing in several inches of water.",
	"This seems to be a working mine. Many different tunnels wander off following\n"
	 "glowing veins of precious metal. The floor is flooded here since we must\n"
	 "be nearly at sea level. A ladder leads up. ****",
	"This seems to be a working mine. Many different tunnels wander off following\n"
	 "glowing veins of precious metal. The floor is flooded here since we must\n"
	 "be nearly at sea level. A ladder leads up.****",
	{ 264, 265, 0, MINE_CHAMBER, 262, 0, 0, 1 }
    },{ // 264
	"The tunnel here is blocked by broken rocks.",
	"The way is blocked, but if you had some dynamite, we might be able to blast our\n"
	 "way through.*  The passage goes -.**",
	"The way is blocked, but if you had some dynamite, we might be able to blast our\n"
	 "way through.*  The passage goes -.**",
	{ 0, 0, 263, 0, 0, 0, 0, 0 }
    },{ // 265
	"The tunnel is too flooded to proceed.",
	"Hidden shafts could swallow us if we tried to continue on down this tunnel.\n"
	 "The flooding is already up to my waist. Large crystals overhead shimmer\n"
	 "rainbows of reflected light.***  Let's go -.",
	"Hidden shafts could swallow us if we tried to continue on down this tunnel.\n"
	 "The flooding is already up to my waist. Large crystals overhead shimmer\n"
	 "rainbows of reflected light.***  Let's go -.",
	{ 0, 0, 0, 263, 0, 0, 0, 0 }
    },{ // 266, MINE_CHAMBER
	"The mine is less flooded here.",
	"A meandering gold laden vein of quartz and blooming crystals of diamonds\n"
	 "and topaz burst from the walls of the cave. A passage goes -.***",
	"A meandering gold laden vein of quartz and blooming crystals of diamonds\n"
	 "and topaz burst from the walls of the cave. A passage goes -.***",
	{ 0, 263, 0, 0, 0, 0, 0, 0 }
    },{ // 267
	"You are inside the cave.",
	"A hot steam swirls around our heads, and the walls are warm to the touch.\n"
	 "The trail winds + and +.**",
	"A hot steam swirls around our heads, and the walls are warm to the touch.\n"
	 "The trail winds - and -.**",
	{ 230, 0, GODDESS_THRONE_ROOM, 0, 0, 0, 0, 0 }
    },{ // 268, GODDESS_THRONE_ROOM
	"You are in a rather large chamber.",
	"Beds of ferns and palm leaves make several cozy nests along the walls. In the\n"
	 "center of the room is a throne of gold and silver which pulls out into a bed\n"
	 "of enormous size.***  A passageway - leads down.",
	"Beds of ferns and palm leaves make several cozy nests along the walls. In the\n"
	 "center of the room is a throne of gold and silver.***  A passageway leads\n"
	 "down and +.",
	{ 267, 0, 0, 269, 0, 269, 0, 0 }
    },{ // 269
	"You are walking along the edge of a huge abyss.",
	"Steam is rising in great clouds from the immeasurable depths. A very narrow\n"
	 "trail winds down.**  There is a tunnel +.*",
	"Steam is rising in great clouds from the immeasurable depths. A very narrow\n"
	 "trail winds down.**  There is a tunnel -.*",
	{ 0, GODDESS_THRONE_ROOM, 0, 0, GODDESS_THRONE_ROOM, 270, 0, 1 }
    },{ // 270
	"You are on the edge of a huge abyss.",
	"The trail winds farther down.****",
	"The trail winds farther down.****",
	{ 0, 0, 0, 0, 269, 271, 0, 1 }
    },{ // 271
	"You are winding your way along the abyss.",
	"The trail continues up and down.****",
	"The trail continues up and down.****",
	{ 0, 0, 0, 0, 270, 272, 0, 1 }
    },{ // 272
	"You are on a wide shelf near the steamy abyss.",
	"The stifling hot cave seems even hotter to me, staring down into this misty \n"
	 "abyss. A trail winds up.*  A passageway leads -.**",
	"The stifling hot cave seems even hotter to me, staring down into this misty \n"
	 "abyss. A trail winds up.*  A passageway leads -.**",
	{ 0, 0, 273, 0, 271, 0, 0, 1 }
    },{ // 273
	"You are in a wide tunnel leading to a fuming abyss.",
	"The passageway winds through many beautiful formations of crystals and\n"
	 "sparkling minerals. The tunnel continues - and -.**",
	"The passageway winds through many beautiful formations of crystals and\n"
	 "sparkling minerals. The tunnel continues - and -.**",
	{ 272, 0, 274, 0, 0, 0, 0, 0 }
    },{ // 274
	"You are in a tunnel.",
	"It is very warm in here. The smell of steam and hot rocks permeates the place.\n"
	 "The cave continues - and -.**",
	"It is very warm in here. The smell of steam and hot rocks permeates the place.\n"
	 "The cave continues - and -.**",
	{ 273, 0, CAVE_ENTRANCE, 0, 0, 0, 0, 0 }
    },{ // 275, FINAL
	"You are at the bottom of a pit.",
	"I can see daylight far up at the mouth of the pit. A cool draft wafts down.\n"
	 "There doesn't seem to be any way out, and I don't remember how we came in.\n"
	 "If you had a rope it might be possible to climb out. ****",
	"At the top of the pit, a single star can be seen in the night sky. There\n"
	 "doesn't appear to be any way to get out without a rope. I don't remember\n"
	 "how we got here.****",
	{ 0, 0, 0, 0, HUGE_CHASM, 0, 0, 0 }
    }
};
//----------------------------------------------------------------------
