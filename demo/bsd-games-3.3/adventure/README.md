adventure standalone demo
=========================

From this directory, build and play:

```sh
make
./adventure
```

Requires TinyCC, GNU Make, libc development headers, ncurses development
headers/libraries, and terminal data for your `TERM`. On Alpine Linux:

```sh
apk add tcc tcc-libs-static make musl-dev ncurses-dev ncurses-terminfo-base
```

Scores and saves are local files in the current working directory. `make clean`
removes build products and keeps those files. `make run` builds and starts the
game. See `adventure.6` for controls and options.

This directory can be copied and built independently. Compiler and linker
options can be overridden on the make command line, such as `make CC=tcc`.
