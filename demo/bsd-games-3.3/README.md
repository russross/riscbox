# BSD-GAMES

This is the bsd-games package for Linux, containing classic text mode
games from UNIX folklore. `wump`, developed in 1973, `trek` from
1976 (called `spirhunt` in this version), and `adventure` from 1977,
are the oldest.

## Standalone demos

Each game directory is a complete Linux/TinyCC project. Copy one directory,
enter it, compile, and run the local binary:

```sh
cd robots
make
./robots
```

Build requirements are TinyCC, GNU Make, libc development headers, and
ncurses development headers/libraries. Terminal data must cover your `TERM`.
On Alpine Linux, install `tcc tcc-libs-static make musl-dev ncurses-dev ncurses-terminfo-base`.
Hangman also needs a text dictionary; use `-d /path/to/words` when it is not
available at `/usr/share/dict/words`.

Scores and saved games are created in the current working directory, so run
each binary from its own demo directory. `make clean` keeps scores and saves.
There are no configure or installation steps. The root Makefile optionally
builds all games with `make -j`, or one with `make robots`.

Every source is compiled to an object before a separate link step. TinyCC
generates header dependencies. Compiler and linker settings support make
command-line overrides. Support sources and license notices are copied into
each project; no game needs files from its parent directory.

This BSD-GAMES 3.3 tree is vendored for the Riscbox demo. The self-contained
TinyCC ports retain each project's license and copied support-source license.
Demo builds use these sources directly, without downloading upstream games.
Alpine RV64 image preparation builds every game; Chrome acceptance compiles
them again on resident 9p and checks terminal startup in the WASM emulator.
These checks cover startup and interruption, rather than complete playthroughs.

If you find bugs, report them on the SourceForge
[project](https://sourceforge.net/projects/bsd-games)
[bugtracker](https://sourceforge.net/p/bsd-games/tickets).

## Included

This package contains the following games:

* adventure:	the original adventure by Crowther and Woods
* arithmetic:	asks you to do simple calculations
* atc:		air traffic control simulator
* battlestar:	a tropical adventure
* caesar:	performs rotated-alphabet cryptography (like rot13)
* cribbage:	cribbage card game
* dab:		dots and boxes
* drop4:	tetromino packing game
* gofish:	go fish card game
* gomoku:	connect-5 version of tic-tac-toe
* hangman:	guess the word before it is too late
* klondike:	curses-based solitaire
* robots:	avoid the evil robots
* sail:		sail your ship into battle
* snake:	grab the cash, avoid the snake, and exit
* spirhunt:	hunt space pirates
* worm:		eat the numbers without running into anything
* wump:		hunt the wumpus

## Excluded

Many programs that were in the original 2.17 distribution were removed
to focus on providing playable games, rather than a rusty junk pile.
boggle, mille, and monop were infringing on Hasbro copyrights. hunt,
dm, and phantasia were unplayable on single-user systems. rain and
worms were text-mode screensavers, of no use today. quiz, wtf, bcd, ppt,
morse, number, pig, pom, random, and wargames, were just plain junk.
The following programs were removed because they are already maintained
elsewhere as seprate projects.

## Found elsewhere

* backgammon:	[https://www.gnu.org/software/gnubg/](https://www.gnu.org/software/gnubg/)
* banner:	[https://packages.debian.org/stable/bsdmainutils](https://packages.debian.org/stable/bsdmainutils)
* factor:	[http://www.gnu.org/software/coreutils/coreutils.html](http://www.gnu.org/software/coreutils/coreutils.html)
* fortune:	[https://ibiblio.org/pub/linux/games/amusements/fortune/](https://ibiblio.org/pub/linux/games/amusements/fortune/)
* primes:	[http://primesieve.org/](http://primesieve.org/)
* rogue:	[http://coredumpcentral.org/](http://coredumpcentral.org/)
* hack:		[https://www.nethack.org](https://www.nethack.org)
