ROOT := ../..
BIN := ../bin
WASM := $(ROOT)/target/wasm32-unknown-unknown/release/riscbox_wasm.wasm
JS_SOURCES := $(shell find $(ROOT)/js/block $(ROOT)/js/network $(ROOT)/js/p9 -name '*.ts' -type f)

all:
	$(MAKE) -C $(ROOT) wasm js kernel opensbi
	$(MAKE) dist/riscbox.cfg

build/rootfs.ext4: setup.sh $(BIN)/create-alpine-ext4 $(BIN)/run-image-setup $(ROOT)/kernel/linux $(ROOT)/opensbi/fw_dynamic.bin
	mkdir -p build
	$(BIN)/create-alpine-ext4 64 $@
	$(BIN)/run-image-setup $@ setup.sh

dist/riscbox.cfg: Makefile build/rootfs.ext4 riscbox.cfg $(JS_SOURCES) $(ROOT)/js/riscbox.js $(ROOT)/js/p9/README.md $(ROOT)/build/js/.built $(WASM) $(ROOT)/kernel/.asset-name $(ROOT)/opensbi/.asset-name $(BIN)/build-distribution $(ROOT)/tools/splitimg.py $(ROOT)/tools/image_deployment.py ../DEPLOYMENT.md ../web/index.html
	$(BIN)/build-distribution . build/rootfs.ext4

clean:
	rm -rf build dist

.PHONY: all clean
.DELETE_ON_ERROR:
