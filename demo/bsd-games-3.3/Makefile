# Optional convenience targets; every game also builds independently.
GAMES = adventure atc robots snake spirhunt

.PHONY: all clean $(GAMES)
all: $(GAMES)

$(GAMES):
	$(MAKE) -C $@

clean:
	@set -e; for game in $(GAMES); do $(MAKE) -C $$game clean; done
