CC = tcc
CFLAGS = -Wall -D_DEFAULT_SOURCE -Iinclude

all: wump

wump: wump.c pathnames.h include/sys/cdefs.h
	$(CC) $(CFLAGS) -o $@ wump.c

run: wump
	./wump

clean:
	rm -f wump

.PHONY: all run clean
