# Optional convenience targets; every game also builds independently.
GAMES = adventure arithmetic atc battlestar caesar cribbage dab drop4 gofish gomoku hangman klondike robots sail snake spirhunt worm wump

.PHONY: all clean $(GAMES)
all: $(GAMES)

$(GAMES):
	$(MAKE) -C $@

clean:
	@set -e; for game in $(GAMES); do $(MAKE) -C $$game clean; done
