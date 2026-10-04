hangman standalone demo
=======================

From this directory, build and play:

```sh
make
./hangman
```

Requires TinyCC, GNU Make, libc development headers, ncurses development
headers/libraries, and terminal data for your `TERM`. On Alpine Linux:

```sh
apk add tcc tcc-libs-static make musl-dev ncurses-dev ncurses-terminfo-base
```

Scores and saves are local files in the current working directory. `make clean`
removes build products and keeps those files. `make run` builds and starts the
game. See `hangman.6` for controls and options.

This directory can be copied and built independently. Compiler and linker
options can be overridden on the make command line, such as `make CC=tcc`.

Hangman also needs a text dictionary. The default is `/usr/share/dict/words`;
use `./hangman -d /path/to/words` to select another dictionary.

Alpine's `cracklib-words` package supplies a compressed text dictionary:

```sh
apk add cracklib-words
gzip -dc /usr/share/cracklib/cracklib-words.gz > words
./hangman -d words
```

The extracted `words` file stays with this demo and survives `make clean`.
