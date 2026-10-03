CC = tcc
CFLAGS = -Wall -D_DEFAULT_SOURCE -Iinclude

all: number

number: number.c include/sys/cdefs.h
	$(CC) $(CFLAGS) -o $@ number.c

run: number
	./number 123456789

clean:
	rm -f number

.PHONY: all run clean
