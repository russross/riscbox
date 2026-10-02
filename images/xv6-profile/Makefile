ROOT := ../..
BIN := ../bin
WASM := $(ROOT)/target/wasm32-unknown-unknown/release/riscbox_wasm.wasm
WEB_SOURCES := $(shell find web -type f)
JS_SOURCES := $(shell find $(ROOT)/js/network -name '*.ts' -type f) $(ROOT)/js/storage.ts $(ROOT)/js/riscbox.d.ts

all:
	$(MAKE) -C $(ROOT) wasm js kernel opensbi
	$(MAKE) dist/riscbox.cfg

build/rootfs.ext4: setup.sh $(BIN)/create-alpine-ext4 $(BIN)/run-image-setup $(BIN)/mount-ephemeral-writes $(ROOT)/kernel/linux $(ROOT)/opensbi/fw_dynamic.bin
	mkdir -p build
	$(BIN)/create-alpine-ext4 512 $@
	$(BIN)/run-image-setup $@ setup.sh $(BIN)/mount-ephemeral-writes

build/rootfs.erofs: build/rootfs.ext4 $(BIN)/create-erofs-from-ext4
	$(BIN)/create-erofs-from-ext4 $< $@

dist/riscbox.cfg: Makefile build/rootfs.erofs riscbox.cfg $(WEB_SOURCES) $(JS_SOURCES) $(ROOT)/js/riscbox.js $(ROOT)/js/storage.ts $(ROOT)/build/js/.built $(WASM) $(ROOT)/kernel/.asset-name $(ROOT)/opensbi/.asset-name $(BIN)/build-distribution $(ROOT)/tools/splitimg.py $(ROOT)/tools/image_deployment.py ../DEPLOYMENT.md ../web/index.html
	$(BIN)/build-distribution . build/rootfs.ext4 --erofs

profile: all
	@mkdir -p profiles
	@set -o pipefail; node --cpu-prof --cpu-prof-dir profiles --cpu-prof-name riscbox.cpuprofile \
		profile-xv6.mjs riscbox dist/riscbox.cfg $(WASM) $${PROFILE_TIMEOUT_SECONDS:-1800} \
		2>&1 | tee profiles/riscbox.log
	@test -s profiles/riscbox.cpuprofile && test -s profiles/riscbox.log

profile-tinyemu: all
	$(MAKE) -C $(ROOT)/c -f Makefile.js
	@mkdir -p profiles
	@set -o pipefail; node --cpu-prof --cpu-prof-dir profiles --cpu-prof-name tinyemu.cpuprofile \
		profile-xv6.mjs tinyemu dist/riscbox.cfg $(ROOT)/c/js/riscbox-wasm.js $${PROFILE_TIMEOUT_SECONDS:-1800} \
		2>&1 | tee profiles/tinyemu.log
	@test -s profiles/tinyemu.cpuprofile && test -s profiles/tinyemu.log

clean:
	rm -rf build dist profiles

.PHONY: all profile profile-tinyemu clean
SHELL := /bin/bash
.DELETE_ON_ERROR:
