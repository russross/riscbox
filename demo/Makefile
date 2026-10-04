RELEASE_ARCHIVE ?=
IMAGE_SOURCES := $(shell find guest examples -type f) build-image.sh
APP_SOURCES := $(shell find web examples docs -type f) assemble.mjs

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
