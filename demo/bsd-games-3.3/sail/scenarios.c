// Copyright (c) 1983 The Regents of the University of California.
// This file is free software, distributed under the BSD license.

#include "sail.h"

struct Scenario _scenarios [NSCENE] = {
    {"Ranger vs. Drake", dir_S,3,5, 2,
      (const struct ScenarioShip[]) {
	{"Ranger",	N_American,	 7,20,	dir_SE,	{ship_Brig,	qual_Crack,	 5, 4,7,3,  19,{ 2, 2},{ 0, 0}, { 2, 2, 2}, {4,4,4,4}}},
	{"Drake",	N_British,	 7,31,	dir_S,	{ship_Brig,	qual_Crack,	 5, 4,7,3,  17,{ 0, 0},{ 4, 4}, { 2, 2, 2}, {3,3,3,3}}},
      }
    },
    {"The Battle of Flamborough Head", dir_N,3,6, 2,
      (const struct ScenarioShip[]) {
	{"Bonhomme Rich", N_American,	13,40,	dir_NE,	{ship_Corvette,	qual_Crack,	 7, 3,5,2,  42,{ 2, 2},{ 0, 0}, { 2, 2, 2}, {5,5,5,-1}}},
	{"Serapis",	N_British,	 2,42,	dir_NE,	{ship_Frigate,	qual_Crack,	 7, 4,6,3,  44,{ 3, 3},{ 0, 0}, { 2, 2, 2}, {5,5,5,5}}},
      }
    },
    {"Arbuthnot and Des Touches", dir_S,5,5, 10,
      (const struct ScenarioShip[]) {
	{"America",	N_British,	 7,37,	dir_SE,	{ship_Line2,	qual_Crack,	17, 3,5,2,  64,{12,12},{ 2, 2}, { 8, 6, 6}, {7,7,7,-1}}},
	{"Befford",	N_British,	 5,35,	dir_SE,	{ship_Line2,	qual_Crack,	20, 3,5,2,  74,{16,16},{ 2, 2}, { 8, 8, 8}, {7,7,7,-1}}},
	{"Adamant",	N_British,	 3,33,	dir_SE,	{ship_Line2,	qual_Crack,	12, 3,5,2,  50,{ 8, 8},{ 2, 2}, { 6, 4, 4}, {6,6,6,-1}}},
	{"London",	N_British,	 1,31,	dir_SE,	{ship_Line,	qual_Crack,	23, 3,5,1,  98,{18,18},{ 2, 2}, {10,10,10}, {8,8,8,-1}}},
	{"Royal Oak",	N_British,	255,29,	dir_SE,	{ship_Line2,	qual_Crack,	20, 3,5,2,  74,{16,16},{ 2, 2}, { 8, 8, 8}, {7,7,7,-1}}},
	{"Neptune",	N_French,	 6,44,	dir_SE,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 0, 0}, {10,10, 8}, {7,7,7,-1}}},
	{"Duc Bougogne", N_French,	 8,46,	dir_SE,	{ship_Line,	qual_Mundane,	23, 3,5,1,  80,{22,22},{ 0, 0}, {12,12,10}, {7,7,7,-1}}},
	{"Conquerant",	N_French,	10,48,	dir_SE,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 2, 2}, {10, 8, 8}, {4,4,7,-1}}},
	{"Provence",	N_French,	12,50,	dir_SE,	{ship_Line2,	qual_Mundane,	18, 3,5,2,  64,{12,12},{ 0, 0}, { 8, 8, 6}, {7,7,7,-1}}},
	{"Romulus",	N_French,	20,58,	dir_SE,	{ship_Line2,	qual_Mundane,	11, 3,5,2,  44,{ 6, 6},{ 2, 2}, { 4, 4, 4}, {5,5,5,-1}}},
      }
    },
    {"Suffren and Hughes", dir_N,3,5, 10,
      (const struct ScenarioShip[]) {
	{"Monmouth",	N_British,	 9,45,	dir_NE,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 2, 2}, {10,10, 8}, {7,7,7,-1}}},
	{"Hero",	N_British,	13,49,	dir_NE,	{ship_Line2,	qual_Crack,	20, 3,5,2,  74,{16,16},{ 2, 2}, { 8, 8, 8}, {7,7,7,-1}}},
	{"Isis",	N_British,	12,48,	dir_NE,	{ship_Line2,	qual_Crack,	12, 3,5,2,  50,{ 8, 8},{ 2, 2}, { 6, 4, 4}, {6,6,6,-1}}},
	{"Superb",	N_British,	10,46,	dir_NE,	{ship_Line2,	qual_Crack,	21, 3,5,2,  74,{16,16},{ 4, 4}, { 8, 8, 6}, {7,7,7,-1}}},
	{"Burford",	N_British,	11,47,	dir_NE,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 2, 2}, {10, 8, 8}, {4,4,7,-1}}},
	{"Flamband",	N_French,	 7,59,	dir_SE,	{ship_Line2,	qual_Mundane,	14, 3,5,2,  50,{ 8, 8},{ 0, 0}, { 6, 6, 4}, {6,6,6,-1}}},
	{"Annibal",	N_French,	 4,56,	dir_SE,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 0, 0}, {10,10, 8}, {7,7,7,-1}}},
	{"Severe",	N_French,	 2,54,	dir_SE,	{ship_Line2,	qual_Mundane,	18, 3,5,2,  64,{12,12},{ 0, 0}, { 8, 8, 6}, {7,7,7,-1}}},
	{"Brilliant",	N_French,	255,51,	dir_SE,	{ship_Line2,	qual_Crack,	24, 3,5,2,  80,{20,20},{ 2, 2}, {10, 8, 8}, {8,8,8,-1}}},
	{"Sphinx",	N_French,	251,47,	dir_SE,	{ship_Line2,	qual_Mundane,	24, 3,5,2,  80,{22,22},{ 2, 2}, {12,12,10}, {7,7,7,-1}}},
      }
    },
    {"Nymphe vs. Cleopatre", dir_N,3,4, 2,
      (const struct ScenarioShip[]) {
	{"Nymphe",	N_British,	13,30,	dir_NE,	{ship_Frigate,	qual_Crack,	11, 4,6,3,  36,{ 4, 4},{ 2, 2}, { 4, 4, 2}, {5,5,5,5}}},
	{"Cleopatre",	N_French,	 3,41,	dir_NE,	{ship_Frigate,	qual_Mundane,	11, 4,6,3,  36,{ 4, 4},{ 2, 2}, { 4, 4, 4}, {5,5,5,5}}},
      }
    },
    {"Mars vs. Hercule", dir_N,3,5, 2,
      (const struct ScenarioShip[]) {
	{"Mars",	N_British,	13,30,	dir_NE,	{ship_Line2,	qual_Crack,	21, 3,5,2,  74,{18,18},{ 2, 2}, {10, 8, 8}, {7,7,7,-1}}},
	{"Hercule",	N_French,	 3,41,	dir_NE,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 2, 2}, {10,10, 8}, {7,7,7,-1}}},
      }
    },
    {"Ambuscade vs. Baionnaise", dir_S,3,5, 2,
      (const struct ScenarioShip[]) {
	{"Ambuscade",	N_British,	13,30,	dir_NE,	{ship_Frigate,	qual_Mundane,	 8, 4,6,3,  32,{ 4, 4},{ 2, 2}, { 4, 2, 2}, {5,5,5,5}}},
	{"Baionnaise",	N_French,	 3,41,	dir_NE,	{ship_Corvette,	qual_Mundane,	 6, 4,6,3,  24,{ 2, 2},{ 0, 0}, { 4, 4, 4}, {4,4,4,4}}},
      }
    },
    {"Constellation vs. Insurgent", dir_N,5,6, 2,
      (const struct ScenarioShip[]) {
	{"Constellation", N_American,	 9,50,	dir_NW,	{ship_Corvette,	qual_Elite,	14, 4,7,3,  38,{ 4, 4},{ 6, 6}, { 6, 4, 4}, {5,5,5,5}}},
	{"Insurgent",	N_French,	 4,24,	dir_NE,	{ship_Corvette,	qual_Mundane,	11, 4,7,3,  36,{ 4, 4},{ 2, 2}, { 6, 6, 4}, {5,5,5,5}}},
      }
    },
    {"Constellation vs. Vengeance", dir_N,3,5, 2,
      (const struct ScenarioShip[]) {
	{"Constellation", N_American,	12,40,	dir_NE,	{ship_Corvette,	qual_Elite,	14, 4,7,3,  38,{ 4, 4},{ 6, 6}, { 6, 4, 4}, {5,5,5,5}}},
	{"Vengeance",	N_French,	 1,43,	dir_NE,	{ship_Frigate,	qual_Mundane,	15, 4,6,3,  40,{ 6, 6},{ 4, 4}, { 8, 6, 6}, {5,5,5,5}}},
      }
    },
    {"The Battle of Lissa", dir_N,3,6, 10,
      (const struct ScenarioShip[]) {
	{"Amphion",	N_British,	 8,50,	dir_SE,	{ship_Frigate,	qual_Elite,	11, 4,6,3,  32,{ 4, 4},{ 2, 2}, { 4, 4, 2}, {5,5,5,5}}},
	{"Active",	N_British,	 6,48,	dir_SE,	{ship_Frigate,	qual_Elite,	14, 4,6,3,  38,{ 6, 6},{ 4, 4}, { 4, 4, 4}, {5,5,5,5}}},
	{"Volage",	N_British,	 4,46,	dir_SE,	{ship_Frigate,	qual_Elite,	 6, 4,6,3,  22,{ 0, 0},{ 8, 8}, { 2, 2, 2}, {4,4,4,4}}},
	{"Cerberus",	N_British,	 2,44,	dir_SE,	{ship_Frigate,	qual_Elite,	11, 4,6,3,  32,{ 4, 4},{ 2, 2}, { 4, 4, 2}, {5,5,5,5}}},
	{"Favorite",	N_French,	 9,34,	dir_NE,	{ship_Frigate,	qual_Mundane,	14, 4,6,3,  40,{ 6, 6},{ 4, 4}, { 6, 6, 4}, {5,5,5,5}}},
	{"Flore",	N_French,	13,39,	dir_NE,	{ship_Frigate,	qual_Mundane,	15, 4,6,3,  40,{ 6, 6},{ 4, 4}, { 8, 6, 6}, {5,5,5,5}}},
	{"Danae",	N_French,	15,37,	dir_NE,	{ship_Frigate,	qual_Crack,	15, 4,6,3,  40,{ 8, 8},{ 6, 6}, { 4, 4, 4}, {5,5,5,5}}},
	{"Bellona",	N_French,	17,35,	dir_NE,	{ship_Frigate,	qual_Green,	11, 4,6,3,  32,{ 4, 4},{ 0, 0}, { 4, 4, 4}, {5,5,5,5}}},
	{"Corona",	N_French,	12,31,	dir_NE,	{ship_Frigate,	qual_Green,	14, 4,6,3,  40,{ 6, 6},{ 4, 4}, { 6, 6, 4}, {5,5,5,5}}},
	{"Carolina",	N_French,	15,28,	dir_NE,	{ship_Frigate,	qual_Green,	 8, 4,6,3,  32,{ 2, 2},{ 0, 0}, { 4, 4, 1}, {4,4,4,4}}},
      }
    },
    {"Constitution vs. Guerriere", dir_NE,5,6, 2,
      (const struct ScenarioShip[]) {
	{"Constitution", N_American,	 7,35,	dir_N,	{ship_Corvette,	qual_Elite,	18, 4,7,3,  44,{ 8, 8},{ 6, 6}, { 6, 6, 6}, {6,6,6,6}}},
	{"Guerriere",	N_British,	 7,47,	dir_SE,	{ship_Frigate,	qual_Crack,	14, 4,6,3,  38,{ 6, 6},{ 4, 4}, { 4, 4, 2}, {5,5,5,5}}},
      }
    },
    {"United States vs. Macedonian", dir_N,3,5, 2,
      (const struct ScenarioShip[]) {
	{"United States", N_American,	 1,52,	dir_SW,	{ship_Frigate,	qual_Elite,	18, 4,5,3,  44,{ 8, 8},{ 8, 8}, { 8, 6, 6}, {6,6,6,6}}},
	{"Macedonian",	N_British,	14,40,	dir_N,	{ship_Frigate,	qual_Crack,	14, 4,6,3,  38,{ 6, 6},{ 4, 4}, { 4, 4, 4}, {5,5,5,5}}},
      }
    },
    {"Constitution vs. Java", dir_N,3,6, 2,
      (const struct ScenarioShip[]) {
	{"Constitution", N_American,	 1,40,	dir_NE,	{ship_Corvette,	qual_Elite,	18, 4,7,3,  44,{ 8, 8},{ 6, 6}, { 6, 6, 6}, {6,6,6,6}}},
	{"Java",	N_British,	11,40,	dir_NE,	{ship_Corvette,	qual_Crack,	14, 4,7,3,  38,{ 6, 6},{ 6, 6}, { 6, 6, 6}, {5,5,5,5}}},
      }
    },
    {"Chesapeake vs. Shannon", dir_N,3,5, 2,
      (const struct ScenarioShip[]) {
	{"Chesapeake",	N_American,	13,40,	dir_NE,	{ship_Frigate,	qual_Mundane,	14, 4,6,3,  38,{ 6, 6},{ 6, 6}, { 6, 6, 4}, {5,5,5,5}}},
	{"Shannon",	N_British,	 1,42,	dir_NE,	{ship_Frigate,	qual_Elite,	14, 4,6,3,  38,{ 6, 6},{ 6, 6}, { 6, 4, 4}, {5,5,5,5}}},
      }
    },
    {"The Battle of Lake Erie", dir_N,1,6, 5,
      (const struct ScenarioShip[]) {
	{"Lawrence",	N_American,	 4,55,	dir_NW,	{ship_Brig,	qual_Crack,	 6, 4,7,3,  20,{ 0, 0},{ 6, 6}, { 4, 2, 2}, {4,4,4,4}}},
	{"Niagara",	N_American,	 7,61,	dir_NW,	{ship_Brig,	qual_Elite,	 6, 4,7,3,  20,{ 0, 0},{ 6, 6}, { 4, 2, 2}, {4,4,4,4}}},
	{"Lady Prevost", N_British,	 4,25,	dir_NE,	{ship_Driftwood, qual_Crack,	 3, 4,7,3,  13,{ 0, 0},{ 2, 2}, { 0, 2, 2}, {2,2,2,2}}},
	{"Detroit",	N_British,	 7,22,	dir_NE,	{ship_Brig,	qual_Crack,	 5, 4,7,3,  19,{ 2, 2},{ 0, 0}, { 2, 2, 2}, {4,4,4,4}}},
	{"Q. Charlotte", N_British,	10,19,	dir_NE,	{ship_Brig,	qual_Crack,	 5, 4,7,3,  17,{ 2, 2},{ 0, 0}, { 2, 2, 2}, {3,3,3,3}}},
      }
    },
    {"Wasp vs. Reindeer", dir_N,1,5, 2,
      (const struct ScenarioShip[]) {
	{"Wasp",	N_American,	 3,41,	dir_NE,	{ship_Brig,	qual_Elite,	 6, 4,7,3,  20,{ 0, 0},{ 6, 6}, { 4, 2, 2}, {4,4,4,4}}},
	{"Reindeer",	N_British,	10,48,	dir_NE,	{ship_Brig,	qual_Elite,	 5, 4,7,3,  18,{ 0, 0},{ 6, 6}, { 2, 2, 2}, {4,4,4,4}}},
      }
    },
    {"Constitution vs. Cyane and Levant", dir_N,2,5, 3,
      (const struct ScenarioShip[]) {
	{"Constitution", N_American,	10,45,	dir_NE,	{ship_Corvette,	qual_Elite,	18, 4,7,3,  44,{ 8, 8},{ 6, 6}, { 6, 6, 6}, {6,6,6,6}}},
	{"Cyane",	N_British,	 3,37,	dir_NE,	{ship_Brig,	qual_Crack,	 6, 4,7,3,  24,{ 0, 0},{10,10}, { 4, 2, 2}, {4,4,4,4}}},
	{"Levant",	N_British,	 5,35,	dir_NE,	{ship_Brig,	qual_Crack,	 6, 4,7,3,  20,{ 0, 0},{ 8, 8}, { 2, 2, 2}, {4,4,4,4}}},
      }
    },
    {"Pellew vs. Droits de L'Homme", dir_S,5,5, 3,
      (const struct ScenarioShip[]) {
	{"Indefatigable", N_British,	12,45,	dir_SW,	{ship_Frigate,	qual_Elite,	11, 4,6,3,  44,{ 4, 4},{ 2, 2}, { 4, 4, 4}, {5,5,5,5}}},
	{"Amazon",	N_British,	 9,48,	dir_SW,	{ship_Frigate,	qual_Crack,	12, 4,6,3,  36,{ 6, 6},{ 2, 2}, { 4, 4, 4}, {5,5,5,5}}},
	{"Droits L'Hom", N_French,	 3,28,	dir_S,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 2, 2}, {10, 8, 8}, {4,4,7,-1}}},
      }
    },
    {"Algeciras", dir_NE,2,3, 10,
      (const struct ScenarioShip[]) {
	{"Caesar",	N_British,	 7,70,	dir_SW,	{ship_Line2,	qual_Crack,	24, 3,5,2,  80,{20,20},{ 2, 2}, {10, 8, 8}, {8,8,8,-1}}},
	{"Pompee",	N_British,	 5,72,	dir_SW,	{ship_Line2,	qual_Crack,	21, 3,5,2,  74,{16,16},{ 4, 4}, { 8, 8, 6}, {7,7,7,-1}}},
	{"Spencer",	N_British,	 3,74,	dir_SW,	{ship_Line2,	qual_Crack,	20, 3,5,2,  74,{16,16},{ 2, 2}, { 8, 8, 8}, {7,7,7,-1}}},
	{"Hannibal",	N_British,	 1,76,	dir_SW,	{ship_Line,	qual_Crack,	23, 3,5,1,  98,{18,18},{ 2, 2}, {10,10,10}, {8,8,8,-1}}},
	{"Real-Carlos",	N_Spanish,	 9,20,	dir_E,	{ship_Line,	qual_Green,	27, 3,5,1, 112,{24,24},{ 0, 0}, {12,12,12}, {9,9,9,-1}}},
	{"San Fernando", N_Spanish,	11,16,	dir_E,	{ship_Line,	qual_Green,	24, 3,5,1,  96,{20,20},{ 0, 0}, {12,12,10}, {8,8,8,-1}}},
	{"Argonauta",	N_Spanish,	10,14,	dir_SE,	{ship_Line2,	qual_Green,	23, 3,5,2,  80,{20,20},{ 0, 0}, {10,10, 8}, {7,7,7,-1}}},
	{"San Augustine", N_Spanish,	 6,22,	dir_SE,	{ship_Line2,	qual_Green,	21, 3,5,2,  74,{16,16},{ 4, 4}, {10, 8, 8}, {7,7,7,-1}}},
	{"Indomptable",	N_French,	 7,23,	dir_S,	{ship_Line2,	qual_Mundane,	24, 3,5,2,  80,{22,22},{ 2, 2}, {12,12,10}, {7,7,7,-1}}},
	{"Desaix",	N_French,	 7,27,	dir_W,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 2, 2}, {10,10, 8}, {7,7,7,-1}}},
      }
    },
    {"Lake Champlain", dir_S,3,6, 7,
      (const struct ScenarioShip[]) {
	{"Saratoga",	N_American,	 8,10,	dir_N,	{ship_Brig,	qual_Crack,	 6, 4,7,3,  26,{ 2, 2},{ 6, 6}, { 4, 2, 2}, {4,4,4,4}}},
	{"Eagle",	N_American,	 9,13,	dir_NE,	{ship_Brig,	qual_Crack,	 6, 4,7,3,  20,{ 0, 0},{ 6, 6}, { 4, 2, 2}, {4,4,4,4}}},
	{"Ticonderoga",	N_American,	12,17,	dir_E,	{ship_Brig,	qual_Crack,	 5, 4,7,3,  17,{ 0, 0},{ 6, 6}, { 2, 2, 2}, {4,4,4,4}}},
	{"Preble",	N_American,	14,20,	dir_NE,	{ship_Driftwood, qual_Crack,	 3, 4,7,3,   7,{ 0, 0},{ 2, 2}, { 0, 2, 2}, {2,2,2,2}}},
	{"Confiance",	N_British,	 4,70,	dir_SW,	{ship_Frigate,	qual_Crack,	12, 4,6,3,  37,{ 6, 6},{ 4, 4}, { 4, 4, 2}, {5,5,5,5}}},
	{"Linnet",	N_British,	 7,68,	dir_SW,	{ship_Brig,	qual_Elite,	 5, 4,7,3,  16,{ 0, 0},{ 4, 4}, { 2, 2, 2}, {4,4,4,4}}},
	{"Chubb",	N_British,	10,65,	dir_SW,	{ship_Driftwood, qual_Crack,	 3, 4,7,3,  11,{ 0, 0},{ 2, 2}, { 2, 2, 2}, {2,2,2,2}}},
      }
    },
    {"Last Voyage of the USS President", dir_S,3,6, 4,
      (const struct ScenarioShip[]) {
	{"President",	N_American,	12,42,	dir_S,	{ship_Frigate,	qual_Elite,	18, 4,5,3,  44,{ 8, 8},{ 8, 8}, { 8, 6, 6}, {6,6,6,6}}},
	{"Endymion",	N_British,	 5,42,	dir_S,	{ship_Frigate,	qual_Crack,	15, 4,6,3,  40,{ 8, 8},{ 6, 6}, { 4, 4, 4}, {5,5,5,5}}},
	{"Pomone",	N_British,	 7,82,	dir_SW,	{ship_Frigate,	qual_Crack,	15, 4,6,3,  44,{10,10},{ 2, 2}, { 8, 8, 6}, {6,6,6,6}}},
	{"Tenedos",	N_British,	 7,255,	dir_SE,	{ship_Frigate,	qual_Crack,	14, 4,6,3,  38,{ 6, 6},{ 6, 6}, { 4, 4, 4}, {5,5,5,5}}},
      }
    },
    {"Hornblower and the Natividad", dir_W,5,5, 2,
      (const struct ScenarioShip[]) {
	{"Lydia",	N_British,	12,40,	dir_NE,	{ship_Frigate,	qual_Elite,	 9, 4,6,3,  36,{ 4, 4},{ 2, 2}, { 4, 4, 2}, {5,5,5,5}}},
	{"Natividad",	N_Spanish,	 2,40,	dir_SE,	{ship_Line2,	qual_Green,	14, 3,5,2,  50,{ 8, 8},{ 0, 0}, { 6, 6, 6}, {6,6,6,-1}}},
      }
    },
    {"Curse of the Flying Dutchman", dir_N,3,6, 2,
      (const struct ScenarioShip[]) {
	{"Piece of Cake", N_Spanish,	 7,40,	dir_NE,	{ship_Corvette,	qual_Mundane,	 6, 4,6,3,  24,{ 2, 2},{ 0, 0}, { 4, 4, 4}, {4,4,4,4}}},
	{"Flying Dutchy", N_French,	 7,41,	dir_N,	{ship_Line,	qual_Elite,	27, 3,5,1, 120,{28,28},{ 2, 2}, {16,14,14}, {9,9,9,-1}}},
      }
    },
    {"The South Pacific", dir_N,4,1, 4,
      (const struct ScenarioShip[]) {
	{"USS Scurvy",	N_American,	 7,40,	dir_N,	{ship_Line,	qual_Mutinous,	30, 3,5,1, 136,{28,28},{ 0, 0}, { 8,14,14}, {9,9,9,-1}}},
	{"HMS Tahiti",	N_British,	12,60,	dir_N,	{ship_Line,	qual_Elite,	27, 3,5,1, 120,{28,28},{ 2, 2}, {16,14,14}, {9,9,9,-1}}},
	{"Australian",	N_Spanish,	 5,20,	dir_NW,	{ship_Frigate,	qual_Mundane,	 8, 4,6,3,  32,{ 4, 4},{ 2, 2}, { 4, 2, 2}, {5,5,5,5}}},
	{"Bikini Atoll", N_French,	 2,60,	dir_SE,	{ship_Driftwood, qual_Crack,	 3, 4,7,3,   7,{ 0, 0},{ 2, 2}, { 0, 2, 2}, {2,2,2,2}}},
      }
    },
    {"Hornblower and the battle of Rosas bay", dir_W,3,6, 5,
      (const struct ScenarioShip[]) {
	{"Sutherland",	N_British,	13,30,	dir_NE,	{ship_Line2,	qual_Crack,	20, 3,5,2,  74,{16,16},{ 2, 2}, { 8, 8, 8}, {7,7,7,-1}}},
	{"Turenne",	N_French,	 9,35,	dir_SW,	{ship_Line,	qual_Mundane,	23, 3,5,1,  80,{22,22},{ 0, 0}, {12,12,10}, {7,7,7,-1}}},
	{"Nightmare",	N_French,	 7,37,	dir_SW,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 0, 0}, {10,10, 8}, {7,7,7,-1}}},
	{"Paris",	N_French,	 3,45,	dir_SE,	{ship_Line,	qual_Green,	27, 3,5,1, 112,{24,24},{ 0, 0}, {12,12,12}, {9,9,9,-1}}},
	{"Napoleon",	N_French,	 1,40,	dir_SW,	{ship_Line2,	qual_Green,	21, 3,5,2,  74,{16,16},{ 4, 4}, {10, 8, 8}, {7,7,7,-1}}},
      }
    },
    {"Cape Horn", dir_SW,4,7, 5,
      (const struct ScenarioShip[]) {
	{"Concord",	N_American,	 3,20,	dir_SE,	{ship_Line2,	qual_Mundane,	24, 3,5,2,  80,{22,22},{ 2, 2}, {12,12,10}, {7,7,7,-1}}},
	{"Berkeley",	N_American,	 5,50,	dir_S,	{ship_Line,	qual_Crack,	23, 3,5,1,  98,{18,18},{ 2, 2}, {10,10,10}, {8,8,8,-1}}},
	{"Thames",	N_British,	10,40,	dir_N,	{ship_Line,	qual_Elite,	27, 3,5,1, 120,{28,28},{ 2, 2}, {16,14,14}, {9,9,9,-1}}},
	{"Madrid",	N_Spanish,	13,60,	dir_NW,	{ship_Line,	qual_Green,	27, 3,5,1, 112,{24,24},{ 0, 0}, {12,12,12}, {9,9,9,-1}}},
	{"Musket",	N_French,	10,60,	dir_W,	{ship_Line,	qual_Mundane,	23, 3,5,1,  80,{22,22},{ 0, 0}, {12,12,10}, {7,7,7,-1}}},
      }
    },
    {"New Orleans", dir_NW,3,7, 3,
      (const struct ScenarioShip[]) {
	{"Alligator",	N_American,	13, 5,	dir_N,	{ship_Line,	qual_Elite,	27, 3,5,1, 120,{28,28},{ 2, 2}, {16,14,14}, {9,9,9,-1}}},
	{"Firefly",	N_British,	10,20,	dir_NW,	{ship_Line2,	qual_Crack,	21, 3,5,2,  74,{16,16},{ 4, 4}, { 8, 8, 6}, {7,7,7,-1}}},
	{"Cypress",	N_British,	 5,10,	dir_SW,	{ship_Frigate,	qual_Elite,	11, 4,6,3,  44,{ 4, 4},{ 2, 2}, { 4, 4, 4}, {5,5,5,5}}},
      }
    },
    {"Botany Bay", dir_S,3,7, 3,
      (const struct ScenarioShip[]) {
	{"Shark",	N_British,	 6,15,	dir_SE,	{ship_Line2,	qual_Mundane,	18, 3,5,2,  64,{12,12},{ 0, 0}, { 8, 8, 6}, {7,7,7,-1}}},
	{"Coral Snake",	N_French,	 3,30,	dir_SW,	{ship_Corvette,	qual_Elite,	18, 4,7,3,  44,{ 8, 8},{ 6, 6}, { 6, 6, 6}, {6,6,6,6}}},
	{"Sea Lion",	N_French,	13,50,	dir_NW,	{ship_Frigate,	qual_Elite,	18, 4,5,3,  44,{ 8, 8},{ 8, 8}, { 8, 6, 6}, {6,6,6,6}}},
      }
    },
    {"Voyage to the Bottom of the Sea", dir_SE,3,6, 4,
      (const struct ScenarioShip[]) {
	{"Seaview",	N_American,	 6, 3,	dir_E,	{ship_Line,	qual_Elite,	27, 3,5,1, 120,{28,28},{ 2, 2}, {16,14,14}, {9,9,9,-1}}},
	{"Flying Sub",	N_American,	 8, 3,	dir_E,	{ship_Frigate,	qual_Crack,	15, 4,6,3,  40,{ 8, 8},{ 6, 6}, { 4, 4, 4}, {5,5,5,5}}},
	{"Mermaid",	N_British,	 2, 5,	dir_S,	{ship_Line,	qual_Mutinous,	30, 3,5,1, 136,{28,28},{ 0, 0}, { 8,14,14}, {9,9,9,-1}}},
	{"Giant Squid",	N_Spanish,	10,30,	dir_NW,	{ship_Line,	qual_Green,	27, 3,5,1, 112,{24,24},{ 0, 0}, {12,12,12}, {9,9,9,-1}}},
      }
    },
    {"Frigate Action", dir_W,3,6, 3,
      (const struct ScenarioShip[]) {
	{"Killdeer",	N_American,	 7,20,	dir_NW,	{ship_Frigate,	qual_Mundane,	15, 4,6,3,  40,{ 6, 6},{ 4, 4}, { 8, 6, 6}, {5,5,5,5}}},
	{"Sandpiper",	N_British,	 5,40,	dir_NW,	{ship_Frigate,	qual_Mundane,	14, 4,6,3,  40,{ 6, 6},{ 4, 4}, { 6, 6, 4}, {5,5,5,5}}},
	{"Curlew",	N_Spanish,	10,60,	dir_NW,	{ship_Frigate,	qual_Crack,	14, 4,6,3,  38,{ 6, 6},{ 4, 4}, { 4, 4, 4}, {5,5,5,5}}},
      }
    },
    {"The Battle of Midway", dir_W,2,5, 6,
      (const struct ScenarioShip[]) {
	{"Enterprise",	N_American,	10,70,	dir_NW,	{ship_Line2,	qual_Crack,	24, 3,5,2,  80,{20,20},{ 2, 2}, {10, 8, 8}, {8,8,8,-1}}},
	{"Yorktown",	N_American,	 3,70,	dir_W,	{ship_Line2,	qual_Mundane,	24, 3,5,2,  80,{22,22},{ 2, 2}, {12,12,10}, {7,7,7,-1}}},
	{"Hornet",	N_American,	 6,70,	dir_W,	{ship_Line2,	qual_Mundane,	21, 3,5,2,  74,{20,20},{ 2, 2}, {10,10, 8}, {7,7,7,-1}}},
	{"Akagi",	N_Japanese,	 6,10,	dir_SE,	{ship_Line,	qual_Green,	27, 3,5,1, 112,{24,24},{ 0, 0}, {12,12,12}, {9,9,9,-1}}},
	{"Kaga",	N_Japanese,	 4,12,	dir_SE,	{ship_Line,	qual_Green,	24, 3,5,1,  96,{20,20},{ 0, 0}, {12,12,10}, {8,8,8,-1}}},
	{"Soryu",	N_Japanese,	 2,14,	dir_SE,	{ship_Line2,	qual_Green,	23, 3,5,2,  80,{20,20},{ 0, 0}, {10,10, 8}, {7,7,7,-1}}},
      }
    },
};
