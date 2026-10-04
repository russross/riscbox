RELEASE_ARCHIVE ?=
# Source-only prerequisites keep local game builds out of image and app assembly.
GAME_SOURCES := $(shell find bsd-games-3.3 -type f \( -name '*.c' -o -name '*.h' -o -name '*.6' -o -name '*.md' -o -name Makefile -o -name LICENSE \))
IMAGE_SOURCES := $(shell find guest -type f) $(GAME_SOURCES) build-image.sh Makefile
APP_SOURCES := $(shell find web docs -type f) $(GAME_SOURCES) assemble.mjs Makefile

all: release
	$(MAKE) dist/.built
	@printf '\nServe the demo:\n  python3 -m http.server --directory "%s" 8000\n  http://localhost:8000/\n' '$(abspath dist)'

# A downloaded archive bypasses the parent build and is the only runtime input.
release:
	@set -eu; if [ -n "$(RELEASE_ARCHIVE)" ]; then \
		./stage-release.sh "$(RELEASE_ARCHIVE)"; \
	else \
		$(MAKE) -C ..; \
		archive=$$($(MAKE) --no-print-directory -s -C .. release-path); \
		./stage-release.sh "$$archive"; \
	fi

build/rootfs.erofs: $(IMAGE_SOURCES) build/release.sha256
	./build-image.sh "build/release/$$(cat build/release.name)"

dist/.built: build/rootfs.erofs build/release.sha256 $(APP_SOURCES)
	node assemble.mjs
	touch $@

test: all
	node --test ../tests/demo_browser.test.mjs

clean:
	rm -rf build dist

.PHONY: all release test clean
.DELETE_ON_ERROR:
