CC = tcc
CFLAGS = -Wall -D_DEFAULT_SOURCE -Iinclude

all: arithmetic

arithmetic: arithmetic.c include/sys/cdefs.h
	$(CC) $(CFLAGS) -o $@ arithmetic.c

run: arithmetic
	./arithmetic

clean:
	rm -f arithmetic

.PHONY: all run clean
