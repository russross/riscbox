// Fixed defaults for standalone Linux/TinyCC demos.
#pragma once
#define BSDGAMES_NAME "bsdgames"
#define BSDGAMES_VERSION 0x33
#define BSDGAMES_VERSTRING "v3.3"
#define BSDGAMES_BUGREPORT "Mike Sharov <msharov@users.sourceforge.net>"

// State files are relative to the directory from which the demo is run.
#define _PATH_GAME_STATE ""
#ifndef _PATH_WORDLIST
#define _PATH_WORDLIST "/usr/share/dict/words"
#endif
#include "common/common.h"
