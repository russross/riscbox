// Copyright (c) 1987 by Ed James <edjames@berkeley.edu>
// This file is free software, distributed under the BSD license.

#include "atc.h"

//{{{ Helper macros ----------------------------------------------------

// Direction codes from old grammar
// w	DIR_NORTH
// e	DIR_NE
// d	DIR_EAST
// c	DIR_SE
// x	DIR_SOUTH
// z	DIR_SW
// a	DIR_WEST
// q	DIR_NW

#define ArrayCName(scenario,aname)	c_##scenario##Scenario_##aname##s
#define StaticScenarioArray(scenario,aname)\
    .num_##aname##s = ArraySize(ArrayCName(scenario,aname)),\
    .aname = ArrayCName(scenario,aname)

#define LineDef(x1,y1,x2,y2)	\
    {{x1,y1,DIR_NORTH}, {x2,y2,DIR_NORTH}}

//}}}-------------------------------------------------------------------
//{{{ Default scenario

static const EXIT c_DefaultScenario_exits[] = {
    { 12,  0, DIR_SOUTH	},
    { 29,  0, DIR_SW	},
    { 29,  7, DIR_WEST	},
    { 29, 17, DIR_WEST	},
    {  9, 20, DIR_NE	},
    {  0, 13, DIR_EAST	},
    {  0,  7, DIR_EAST	},
    {  0,  0, DIR_SE	}
};
static const AIRWAY c_DefaultScenario_lines[] = {
    LineDef (1,1,	6,6),
    LineDef (12,1,	12,6),
    LineDef (13,7,	28,7),
    LineDef (28,1,	13,16),
    LineDef (1,13,	11,13),
    LineDef (12,8,	12,16),
    LineDef (11,18,	10,19),
    LineDef (13,17,	28,17),
    LineDef (1,7,	11,7)
};
static const BEACON c_DefaultScenario_beacons[] = {
    { 12,  7, DIR_NORTH	},
    { 12, 17, DIR_NORTH	}
};
static const AIRPORT c_DefaultScenario_airports[] = {
    { 20, 15, DIR_NORTH	},
    { 20, 18, DIR_EAST	}
};
static const struct Scenario c_DefaultScenario = {
    .name		= "Default",
    .width		= 30,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 10,
    StaticScenarioArray (Default, exit),
    StaticScenarioArray (Default, line),
    StaticScenarioArray (Default, beacon),
    StaticScenarioArray (Default, airport)
};

//}}}-------------------------------------------------------------------
//{{{ Atlantis

static const EXIT c_AtlantisScenario_exits[] = {
    { 10,  0, DIR_SOUTH	},
    { 29,  6, DIR_WEST	},
    { 27, 20, DIR_NW	},
    {  0, 16, DIR_EAST	}
};
static const BEACON c_AtlantisScenario_beacons[] = {
    { 10,  6, DIR_NORTH	},
    { 23,  6, DIR_NORTH	},
    { 23, 16, DIR_NORTH	},
    { 15, 16, DIR_NORTH	},
    {  7, 16, DIR_NORTH	},
    {  7,  9, DIR_NORTH	},
    { 15,  6, DIR_NORTH	}
};
static const AIRWAY c_AtlantisScenario_lines[] = {
    LineDef (10,1, 10,5),
    LineDef (11,6, 14,6),
    LineDef (16,6, 22,6),
    LineDef (24,6, 28,6),
    LineDef (1,16, 6,16),
    LineDef (8,16, 14,16),
    LineDef (16,16, 22,16),
    LineDef (23,7, 23,7),
    LineDef (23,9, 23,15),
    LineDef (15,7, 15,11),
    LineDef (15,13, 15,15),
    LineDef (7,10, 7,15),
    LineDef (9,7, 8,8),
    LineDef (6,10, 1,15),
    LineDef (24,17, 26,19)
};
static const AIRPORT c_AtlantisScenario_airports[] = {
    { 23,  8, DIR_SOUTH	},
    { 15, 12, DIR_NORTH	}
};
static const struct Scenario c_AtlantisScenario = {
    .name		= "Atlantis",
    .width		= 30,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 5,
    StaticScenarioArray (Atlantis, exit),
    StaticScenarioArray (Atlantis, line),
    StaticScenarioArray (Atlantis, beacon),
    StaticScenarioArray (Atlantis, airport)
};

//}}}-------------------------------------------------------------------
//{{{ Killer

static const EXIT c_KillerScenario_exits[] = {
    { 29,  7, DIR_WEST	},
    { 29, 17, DIR_WEST	},
    {  0,  7, DIR_EAST	},
    {  0,  0, DIR_SE	},
};
static const BEACON c_KillerScenario_beacons[] = {
    { 12,  7, DIR_NORTH	},
    { 12, 17, DIR_NORTH	},
    { 14, 10, DIR_NORTH	},
    { 20, 15, DIR_NORTH	}
};
static const AIRPORT c_KillerScenario_airports[] = {
    { 20, 18, DIR_EAST	},
};
static const AIRWAY c_KillerScenario_lines[] = {
    LineDef (1,1, 6,6),
    LineDef (12,1, 12,6),
    LineDef (13,7, 28,7),
    LineDef (28,1, 13,16),
    LineDef (1,13, 11,13),
    LineDef (12,8, 12,16),
    LineDef (11,18, 10,19),
    LineDef (13,17, 28,17),
    LineDef (1,7, 11,7)
};
static const struct Scenario c_KillerScenario = {
    .name		= "Killer",
    .width		= 30,
    .height		= 21,
    .update_secs	= 1,
    .newplane_time	= 4,
    StaticScenarioArray (Killer, exit),
    StaticScenarioArray (Killer, line),
    StaticScenarioArray (Killer, beacon),
    StaticScenarioArray (Killer, airport)
};

//}}}-------------------------------------------------------------------
//{{{ OHare

static const EXIT c_OHareScenario_exits[] = {
    {  6,  0, DIR_SOUTH	},
    { 18,  0, DIR_SOUTH	},
    { 29,  5, DIR_WEST	},
    { 29, 13, DIR_WEST	},
    { 14, 20, DIR_NE	},
    {  6, 20, DIR_NORTH	},
};
static const BEACON c_OHareScenario_beacons[] = {
    {  6,  5, DIR_NORTH	},
    { 18,  5, DIR_NORTH	},
    {  6, 13, DIR_NORTH	}
};
static const AIRPORT c_OHareScenario_airports[] = {
    {  8,  8, DIR_EAST	},
};
static const AIRWAY c_OHareScenario_lines[] = {
    LineDef (6,1, 6,4),
    LineDef (18,1, 18,4),
    LineDef (6,6, 6,12),
    LineDef (6,14, 6,19),
    LineDef (28,6, 15,19),
    LineDef (7,13, 20,13),
    LineDef (22,13, 28,13),
    LineDef (7,5, 17,5),
    LineDef (19,5, 28,5)
};
static const struct Scenario c_OHareScenario = {
    .name		= "OHare",
    .width		= 30,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 5,
    StaticScenarioArray (OHare, exit),
    StaticScenarioArray (OHare, line),
    StaticScenarioArray (OHare, beacon),
    StaticScenarioArray (OHare, airport)
};

//}}}-------------------------------------------------------------------
//{{{ TicTacToe

static const EXIT c_TicTacToeScenario_exits[] = {
    { 10,  0, DIR_SOUTH	},
    { 19,  0, DIR_SOUTH	},
    { 29,  7, DIR_WEST	},
    { 29, 13, DIR_WEST	},
    { 19, 20, DIR_NORTH	},
    { 10, 20, DIR_NORTH	},
    {  0, 13, DIR_EAST	},
    {  0,  7, DIR_EAST	},
};
static const BEACON c_TicTacToeScenario_beacons[] = {
    { 10,  7, DIR_NORTH	},
    { 19,  7, DIR_NORTH	},
    { 19, 13, DIR_NORTH	},
    { 10, 13, DIR_NORTH	}
};
static const AIRWAY c_TicTacToeScenario_lines[] = {
    LineDef (10,1, 10,6),
    LineDef (19,1, 19,6),
    LineDef (1,7, 9,7),
    LineDef (11,7, 18,7),
    LineDef (20,7, 28,7),
    LineDef (10,8, 10,12),
    LineDef (19,8, 19,12),
    LineDef (1,13, 9,13),
    LineDef (11,13, 18,13),
    LineDef (20,13, 28,13),
    LineDef (10,14, 10,19),
    LineDef (19,14, 19,19)
};
static const struct Scenario c_TicTacToeScenario = {
    .name		= "TicTacToe",
    .width		= 30,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 5,
    StaticScenarioArray (TicTacToe, exit),
    StaticScenarioArray (TicTacToe, line),
    StaticScenarioArray (TicTacToe, beacon),
    NULL
};

//}}}-------------------------------------------------------------------
//{{{ Airports

static const EXIT c_AirportsScenario_exits[] = {
    { 13,  0, DIR_SOUTH	},
    {  0, 10, DIR_EAST	},
};
static const BEACON c_AirportsScenario_beacons[] = {
    {  6,  3, DIR_NORTH	},
    { 19,  4, DIR_NORTH	},
    { 27,  4, DIR_NORTH	},
    { 27, 10, DIR_NORTH	},
    { 27, 16, DIR_NORTH	},
    { 13, 16, DIR_NORTH	},
    {  6, 16, DIR_NORTH	},
    { 13, 10, DIR_NORTH	},
    { 19, 10, DIR_NORTH	}
};
static const AIRPORT c_AirportsScenario_airports[] = {
    {  6,  5, DIR_NORTH	},
    { 23,  4, DIR_WEST	},
    { 19,  7, DIR_NORTH	},
    { 25, 10, DIR_EAST	},
    { 24, 16, DIR_WEST	},
    { 13, 13, DIR_SOUTH	},
    {  6, 13, DIR_NORTH	},
};
static const AIRWAY c_AirportsScenario_lines[] = {
    LineDef (13,1, 13,9),
    LineDef (13,11, 13,12),
    LineDef (13,14, 13,15),
    LineDef (7,16, 12,16),
    LineDef (18,5, 14,9),
    LineDef (6,14, 6,15),
    LineDef (6,11, 6,12),
    LineDef (6,6, 6,9),
    LineDef (1,10, 12,10),
    LineDef (27,5, 27,9),
    LineDef (27,11, 27,15)
};
static const struct Scenario c_AirportsScenario = {
    .name		= "Airports",
    .width		= 30,
    .height		= 21,
    .update_secs	= 6,
    .newplane_time	= 6,
    StaticScenarioArray (Airports, exit),
    StaticScenarioArray (Airports, line),
    StaticScenarioArray (Airports, beacon),
    StaticScenarioArray (Airports, airport)
};

//}}}-------------------------------------------------------------------
//{{{ Box

static const EXIT c_BoxScenario_exits[] = {
    {  0,  0, DIR_SE	},
    { 14,  0, DIR_SOUTH	},
    { 28,  0, DIR_SW	},
    { 28, 10, DIR_WEST	},
    { 28, 20, DIR_NW	},
    { 14, 20, DIR_NORTH	},
    {  0, 20, DIR_NE	},
    {  0, 10, DIR_EAST	},
};
static const BEACON c_BoxScenario_beacons[] = {
    {  4,  4, DIR_NORTH	},
    { 14,  4, DIR_NORTH	},
    { 24,  4, DIR_NORTH	},
    { 24, 10, DIR_NORTH	},
    { 24, 16, DIR_NORTH	},
    { 14, 16, DIR_NORTH	},
    {  4, 16, DIR_NORTH	},
    {  4, 10, DIR_NORTH	},
    { 14, 13, DIR_NORTH	},
    { 14,  7, DIR_NORTH	}
};
static const AIRPORT c_BoxScenario_airports[] = {
    {  9,  7, DIR_WEST	},
    { 19,  7, DIR_EAST	},
    { 19, 13, DIR_EAST	},
    {  9, 13, DIR_WEST	},
};
static const AIRWAY c_BoxScenario_lines[] = {
    LineDef (1,1, 3,3),
    LineDef (14,1, 14,3),
    LineDef (27,1, 25,3),
    LineDef (5,4, 13,4),
    LineDef (15,4, 23,4),
    LineDef (4,5, 4,9),
    LineDef (14,5, 14,6),
    LineDef (24,5, 24,9),
    LineDef (10,7, 13,7),
    LineDef (15,7, 18,7),
    LineDef (14,8, 14,9),
    LineDef (1,10, 3,10),
    LineDef (5,10, 23,10),
    LineDef (25,10, 27,10),
    LineDef (4,11, 4,15),
    LineDef (14,11, 14,12),
    LineDef (24,11, 24,15),
    LineDef (10,13, 13,13),
    LineDef (15,13, 18,13),
    LineDef (14,14, 14,15),
    LineDef (5,16, 13,16),
    LineDef (15,16, 23,16),
    LineDef (3,17, 1,19),
    LineDef (14,17, 14,19),
    LineDef (25,17, 27,19)
};
static const struct Scenario c_BoxScenario = {
    .name		= "Box",
    .width		= 29,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 6,
    StaticScenarioArray (Box, exit),
    StaticScenarioArray (Box, line),
    StaticScenarioArray (Box, beacon),
    StaticScenarioArray (Box, airport)
};

//}}}-------------------------------------------------------------------
//{{{ Crosshatch

static const EXIT c_CrosshatchScenario_exits[] = {
    {  0, 10, DIR_EAST	},
    {  6,  0, DIR_SOUTH	},
    { 12,  0, DIR_SOUTH	},
    { 18,  0, DIR_SOUTH	},
    { 24,  0, DIR_SOUTH	},
    { 29, 10, DIR_WEST	},
    { 24, 20, DIR_NORTH	},
    { 18, 20, DIR_NORTH	},
    { 12, 20, DIR_NORTH	},
    {  6, 20, DIR_NORTH	}
};
static const BEACON c_CrosshatchScenario_beacons[] = {
    {  6, 10, DIR_NORTH	},
    { 12, 10, DIR_NORTH	},
    { 18, 10, DIR_NORTH	},
    { 24, 10, DIR_NORTH	},
    {  6,  5, DIR_NORTH	},
    { 12,  5, DIR_NORTH	},
    { 18,  5, DIR_NORTH	},
    { 24,  5, DIR_NORTH	},
    { 12, 15, DIR_NORTH	},
    { 18, 15, DIR_NORTH	}
};
static const AIRPORT c_CrosshatchScenario_airports[] = {
    {  9, 15, DIR_WEST	},
    { 21, 15, DIR_EAST	},
    { 15,  5, DIR_EAST	}
};
static const AIRWAY c_CrosshatchScenario_lines[] = {
    LineDef (6,1, 6,4),
    LineDef (12,1, 12,4),
    LineDef (18,1, 18,4),
    LineDef (24,1, 24,4),
    LineDef (6,6, 6,9),
    LineDef (12,6, 12,9),
    LineDef (18,6, 18,9),
    LineDef (24,6, 24,9),
    LineDef (12,11, 12,14),
    LineDef (18,11, 18,14),
    LineDef (6,11, 6,19),
    LineDef (24,11, 24,19),
    LineDef (12,16, 12,19),
    LineDef (18,16, 18,19),
    LineDef (7,5, 11,5),
    LineDef (19,5, 23,5),
    LineDef (13,15, 17,15),
    LineDef (1,10, 5,10),
    LineDef (7,10, 11,10),
    LineDef (13,10, 17,10),
    LineDef (19,10, 23,10),
    LineDef (25,10, 29,10)
};
static const struct Scenario c_CrosshatchScenario = {
    .name		= "Crosshatch",
    .width		= 30,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 5,
    StaticScenarioArray (Crosshatch, exit),
    StaticScenarioArray (Crosshatch, line),
    StaticScenarioArray (Crosshatch, beacon),
    StaticScenarioArray (Crosshatch, airport)
};

//}}}-------------------------------------------------------------------
//{{{ Crossover

static const EXIT c_CrossoverScenario_exits[] = {
    {  0,  0, DIR_SE	},
    {  8,  0, DIR_SE	},
    { 20,  0, DIR_SW	},
    { 28,  0, DIR_SW	},
    { 28, 20, DIR_NW	},
    { 20, 20, DIR_NW	},
    {  8, 20, DIR_NE	},
    {  0, 20, DIR_NE	}
};
static const BEACON c_CrossoverScenario_beacons[] = {
    { 14,  6, DIR_NORTH	},
    { 18, 10, DIR_NORTH	},
    { 14, 14, DIR_NORTH	},
    { 10, 10, DIR_NORTH	}
};

static const AIRWAY c_CrossoverScenario_lines[] = {
    LineDef (0,0, 20,20),
    LineDef (8,0, 28,20),
    LineDef (20,0, 0,20),
    LineDef (28,0, 8,20)
};
static const struct Scenario c_CrossoverScenario = {
    .name		= "Crossover",
    .width		= 29,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 5,
    StaticScenarioArray (Crossover, exit),
    StaticScenarioArray (Crossover, line),
    StaticScenarioArray (Crossover, beacon),
    NULL
};

//}}}-------------------------------------------------------------------
//{{{ Easy

static const EXIT c_EasyScenario_exits[] = {
    {  7,  0, DIR_SOUTH	},
    { 14,  0, DIR_SW	},
    { 12, 14, DIR_NW	},
    {  0, 14, DIR_NE	}
};
static const BEACON c_EasyScenario_beacons[] = {
    { 12,  7, DIR_NORTH	}
};
static const AIRPORT c_EasyScenario_airports[] = {
    {  7,  8, DIR_NORTH	}
};
static const AIRWAY c_EasyScenario_lines[] = {
    LineDef (1,1, 6,6),
    LineDef (7,9, 12,14),
    LineDef (7,0, 7,14),
    LineDef (1,7, 11,7)
};
static const struct Scenario c_EasyScenario = {
    .name		= "Easy",
    .width		= 15,
    .height		= 15,
    .update_secs	= 7,
    .newplane_time	= 12,
    StaticScenarioArray (Easy, exit),
    StaticScenarioArray (Easy, line),
    StaticScenarioArray (Easy, beacon),
    StaticScenarioArray (Easy, airport)
};

//}}}-------------------------------------------------------------------
//{{{ Scenario2

static const EXIT c_Scenario2Scenario_exits[] = {
    { 12,  0, DIR_SOUTH	},
    { 29,  0, DIR_SW	},
    { 29,  6, DIR_WEST	},
    { 29, 13, DIR_WEST	},
    {  9, 20, DIR_NE	},
    {  0, 13, DIR_EAST	},
    {  0,  6, DIR_EAST	},
    {  0,  0, DIR_SE	}
};
static const BEACON c_Scenario2Scenario_beacons[] = {
    { 12, 17, DIR_NORTH	},
    { 23,  6, DIR_NORTH	},
    { 23, 13, DIR_NORTH	},
    { 25, 17, DIR_NORTH	},
    { 12,  6, DIR_NORTH	},
    { 12, 13, DIR_NORTH	},
    {  6,  6, DIR_NORTH	}
};
static const AIRPORT c_Scenario2Scenario_airports[] = {
    { 18, 17, DIR_EAST	}
};
static const AIRWAY c_Scenario2Scenario_lines[] = {
    LineDef (1,1, 16,16),
    LineDef (1,6, 28,6),
    LineDef (12,1, 12,17),
    LineDef (10,19, 28,1),
    LineDef (13,17, 17,17),
    LineDef (1,13, 28,13),
    LineDef (19,17, 24,17),
    LineDef (19,17, 22,14),
    LineDef (26,16, 28,14)
};
static const struct Scenario c_Scenario2Scenario = {
    .name		= "Scenario2",
    .width		= 30,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 8,
    StaticScenarioArray (Scenario2, exit),
    StaticScenarioArray (Scenario2, line),
    StaticScenarioArray (Scenario2, beacon),
    StaticScenarioArray (Scenario2, airport)
};

//}}}-------------------------------------------------------------------
//{{{ Scenario3

static const EXIT c_Scenario3Scenario_exits[] = {
    { 12,  0, DIR_SOUTH	},
    {  0,  6, DIR_EAST	},
    { 29, 12, DIR_WEST	},
    { 26, 20, DIR_NW	}
};
static const BEACON c_Scenario3Scenario_beacons[] = {
    { 12,  6, DIR_NORTH	}
};
static const AIRPORT c_Scenario3Scenario_airports[] = {
    {  8, 11, DIR_SOUTH	}
};
static const AIRWAY c_Scenario3Scenario_lines[] = {
    LineDef (12,1, 12,5),
    LineDef (1,6, 11,6),
    LineDef (8,7, 8,10),
    LineDef (28,12, 19,12),
    LineDef (13,7, 25,19)
};
static const struct Scenario c_Scenario3Scenario = {
    .name		= "Scenario3",
    .width		= 30,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 5,
    StaticScenarioArray (Scenario3, exit),
    StaticScenarioArray (Scenario3, line),
    StaticScenarioArray (Scenario3, beacon),
    StaticScenarioArray (Scenario3, airport)
};

//}}}-------------------------------------------------------------------
//{{{ Scenario4

static const EXIT c_Scenario4Scenario_exits[] = {
    {  9,  0, DIR_SE	},
    { 29,  0, DIR_SW	},
    { 29, 20, DIR_NW	},
    {  9, 20, DIR_NE	},
    {  0, 20, DIR_NE	},
    {  0, 10, DIR_EAST	},
    {  0,  0, DIR_SE	}
};
static const BEACON c_Scenario4Scenario_beacons[] = {
    {  5,  5, DIR_NORTH	},
    { 14,  5, DIR_NORTH	},
    { 24,  5, DIR_NORTH	},
    { 24,  9, DIR_NORTH	},
    { 24, 11, DIR_NORTH	},
    { 24, 15, DIR_NORTH	},
    { 14, 15, DIR_NORTH	},
    {  5, 15, DIR_NORTH	},
    {  5, 10, DIR_NORTH	},
    { 14, 10, DIR_NORTH	}
};
static const AIRPORT c_Scenario4Scenario_airports[] = {
    { 19,  9, DIR_WEST	},
    { 19, 11, DIR_WEST	}
};
static const AIRWAY c_Scenario4Scenario_lines[] = {
    LineDef (1,1, 4,4),
    LineDef (10,1, 13,4),
    LineDef (28,1, 25,4),
    LineDef (6,5, 13,5),
    LineDef (15,5, 23,5),
    LineDef (1,10, 13,10),
    LineDef (15,10, 17,10),
    LineDef (18,9, 18,9),
    LineDef (18,11, 18,11),
    LineDef (20,9, 23,9),
    LineDef (20,11, 23,11),
    LineDef (6,15, 13,15),
    LineDef (15,15, 24,15),
    LineDef (1,19, 4,16),
    LineDef (10,19, 13,16),
    LineDef (28,19, 25,16),
    LineDef (5,6, 5,9),
    LineDef (5,11, 5,14),
    LineDef (14,6, 14,9),
    LineDef (14,11, 14,14),
    LineDef (24,6, 24,8),
    LineDef (24,10, 24,10),
    LineDef (24,12, 24,14)
};
static const struct Scenario c_Scenario4Scenario = {
    .name		= "Scenario4",
    .width		= 30,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 5,
    StaticScenarioArray (Scenario4, exit),
    StaticScenarioArray (Scenario4, line),
    StaticScenarioArray (Scenario4, beacon),
    StaticScenarioArray (Scenario4, airport)
};

//}}}-------------------------------------------------------------------
//{{{ Novice

static const EXIT c_NoviceScenario_exits[] = {
    {  0,  2, DIR_SE	},
    { 29,  2, DIR_SW	},
    { 29, 18, DIR_NW	},
    {  0, 18, DIR_NE	}
};
static const BEACON c_NoviceScenario_beacons[] = {
    {  8, 10, DIR_NORTH	},
    { 21, 10, DIR_NORTH	}
};
static const AIRWAY c_NoviceScenario_lines[] = {
    LineDef (1,3, 7,9),
    LineDef (7,11, 1,17),
    LineDef (28,3, 22,9),
    LineDef (22,11, 28,17),
    LineDef (9,10, 20,10)
};
static const struct Scenario c_NoviceScenario = {
    .name		= "Novice",
    .width		= 30,
    .height		= 21,
    .update_secs	= 6,
    .newplane_time	= 6,
    StaticScenarioArray (Novice, exit),
    StaticScenarioArray (Novice, line),
    StaticScenarioArray (Novice, beacon),
    NULL
};

//}}}-------------------------------------------------------------------
//{{{ TwoCorners

static const EXIT c_TwoCornersScenario_exits[] = {
    {  0,  0, DIR_SE	},
    { 10,  0, DIR_SOUTH	},
    { 29, 10, DIR_WEST	},
    { 29, 20, DIR_NW	},
    { 19, 20, DIR_NORTH	},
    {  0, 10, DIR_EAST	}
};
static const BEACON c_TwoCornersScenario_beacons[] = {
    { 10, 10, DIR_NORTH	},
    { 19, 10, DIR_NORTH	}
};
static const AIRPORT c_TwoCornersScenario_airports[] = {
    { 15, 15, DIR_SOUTH	}
};
static const AIRWAY c_TwoCornersScenario_lines[] = {
    LineDef (1,1, 9,9),
    LineDef (10,1, 10,9),
    LineDef (1,10, 9,10),
    LineDef (11,10, 18,10),
    LineDef (15,11, 15,14),
    LineDef (20,10, 28,10),
    LineDef (19,11, 19,19),
    LineDef (20,11, 28,19)
};
static const struct Scenario c_TwoCornersScenario = {
    .name		= "TwoCorners",
    .width		= 30,
    .height		= 21,
    .update_secs	= 5,
    .newplane_time	= 5,
    StaticScenarioArray (TwoCorners, exit),
    StaticScenarioArray (TwoCorners, line),
    StaticScenarioArray (TwoCorners, beacon),
    StaticScenarioArray (TwoCorners, airport)
};

//}}}-------------------------------------------------------------------

static const struct Scenario* c_Scenarios[] = {
    &c_DefaultScenario,
    &c_AtlantisScenario,
    &c_KillerScenario,
    &c_OHareScenario,
    &c_TicTacToeScenario,
    &c_AirportsScenario,
    &c_BoxScenario,
    &c_CrosshatchScenario,
    &c_CrossoverScenario,
    &c_EasyScenario,
    &c_Scenario2Scenario,
    &c_Scenario3Scenario,
    &c_Scenario4Scenario,
    &c_NoviceScenario,
    &c_TwoCornersScenario
};

static unsigned scenario_by_name (const char* name)
{
    for (unsigned i = 0; i < ArraySize(c_Scenarios); ++i)
	if (0 == strcmp (name, c_Scenarios[i]->name))
	    return i;
    return UINT_MAX;
}

int load_scenario (const char *s)
{
    unsigned scenarioi = scenario_by_name (s);
    if (scenarioi >= ArraySize(c_Scenarios))
	return -1;
    _sp = c_Scenarios[scenarioi];
    return 0;
}

const char* default_scenario (void)
{
    assert (ArraySize(c_Scenarios) && "no scenarios included with project?");
    return c_Scenarios[0]->name;
}

const char* okay_scenario (const char *s)
{
    return scenario_by_name(s) < ArraySize(c_Scenarios) ? s : NULL;
}

void list_scenarios (void)
{
    assert (ArraySize(c_Scenarios) && "no scenarios included with project?");
    puts ("Available scenarios:");
    for (unsigned i = 0; i < ArraySize(c_Scenarios); ++i)
	printf ("\t%s\n", c_Scenarios[i]->name);
}
