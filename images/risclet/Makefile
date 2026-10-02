ROOT := ../..
BIN := ../bin
WASM := $(ROOT)/target/wasm32-unknown-unknown/release/riscbox_wasm.wasm
RISCLET_URL := https://github.com/russross/risclet/releases/download/v0.4.9/risclet-riscv64gc-unknown-linux-musl
RISCLET_SHA256 := 2acca2490477236a9bb68678c9ba4fc390e3820a980ad6c49a221c85e6d830cb
JS_SOURCES := $(shell find $(ROOT)/js/network -name '*.ts' -type f) $(ROOT)/js/storage.ts $(ROOT)/js/riscbox.d.ts
UI_SOURCES := $(shell find ui $(ROOT)/client-core -type f ! -path '*/node_modules/*' ! -path '*/build/test-*/*' ! -path 'ui/dist/*')
EXAMPLES := $(shell find examples -type f)
WEB_SOURCES := $(shell find web -type f)

all:
	$(MAKE) -C $(ROOT) wasm js kernel opensbi
	$(MAKE) dist/riscbox.cfg

build/risclet:
	mkdir -p build
	curl --fail --location --show-error --output $@.part $(RISCLET_URL)
	printf '%s  %s\n' $(RISCLET_SHA256) $@.part | sha256sum --check --status
	mv $@.part $@

build/rootfs.ext4: build/risclet setup.sh $(BIN)/create-alpine-ext4 $(BIN)/run-image-setup $(BIN)/mount-ephemeral-writes $(ROOT)/kernel/linux $(ROOT)/opensbi/fw_dynamic.bin
	$(BIN)/create-alpine-ext4 64 $@
	$(BIN)/run-image-setup $@ setup.sh build/risclet $(BIN)/mount-ephemeral-writes

build/rootfs.erofs: build/rootfs.ext4 $(BIN)/create-erofs-from-ext4
	$(BIN)/create-erofs-from-ext4 $< $@

dist/riscbox.cfg: Makefile build/rootfs.erofs riscbox.cfg $(UI_SOURCES) $(EXAMPLES) $(WEB_SOURCES) $(JS_SOURCES) $(ROOT)/js/riscbox.js $(ROOT)/js/storage.ts $(ROOT)/build/js/.built $(WASM) $(ROOT)/kernel/.asset-name $(ROOT)/opensbi/.asset-name $(BIN)/build-distribution $(ROOT)/tools/splitimg.py $(ROOT)/tools/image_deployment.py ../DEPLOYMENT.md ../web/index.html
	$(BIN)/build-distribution . build/rootfs.ext4 --erofs

clean:
	rm -rf build dist

.PHONY: all clean
.DELETE_ON_ERROR:
