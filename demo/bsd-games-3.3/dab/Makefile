# Build this demo in place with TinyCC; command-line overrides are supported.
CC = tcc
CPPFLAGS ?= -D_GNU_SOURCE -D_XOPEN_SOURCE=700
CFLAGS ?= -std=c11 -Wall -Wextra
LDFLAGS ?=
LDLIBS ?= -lncursesw

PROGRAM = dab
SOURCES = dab.c common/scores.c common/ui.c common/util.c common/vector.c
OBJECTS = $(SOURCES:.c=.o)
DEPS = $(OBJECTS:.o=.d)

# Each translation unit is compiled separately before the executable is linked.
.PHONY: all clean run
.DELETE_ON_ERROR:
all: $(PROGRAM)

$(PROGRAM): $(OBJECTS) Makefile
	$(CC) $(LDFLAGS) -o $@ $(OBJECTS) $(LDLIBS)

$(OBJECTS): Makefile
%.o: %.c
	$(CC) $(CPPFLAGS) $(CFLAGS) -MD -MF $(@:.o=.d) -c -o $@ $<

# Cleaning preserves local scores and saves so rebuilding does not erase play.
run: $(PROGRAM)
	./$(PROGRAM)

clean:
	rm -f $(PROGRAM) $(OBJECTS) $(DEPS)

-include $(DEPS)
