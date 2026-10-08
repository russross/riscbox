IMAGE_SOURCES := $(shell find guest src ../demo/bsd-games-3.3 -type f \( -name '*.c' -o -name '*.h' -o -name Makefile -o -name LICENSE -o -name '*.md' -o -path 'guest/*' \)) build-image.sh

image: build/rootfs.ext4

build/rootfs.ext4: $(IMAGE_SOURCES) ../kernel/.asset-name ../opensbi/.asset-name
	./build-image.sh

dist: image
	node assemble.mjs

run: dist
	node run.mjs $(BENCH_ARGS)

profile: dist
	node run.mjs --profile $(BENCH_ARGS)

clean:
	rm -rf build dist

.PHONY: image dist run profile clean
.DELETE_ON_ERROR:
