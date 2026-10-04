RUSTBOX_WASM=target/wasm32-unknown-unknown/release/riscbox_wasm.wasm
VERSION := $(shell python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
ARCHIVE := build/releases/riscbox-$(VERSION).tar.gz
JS_SOURCES := $(shell find js/network -name '*.ts' -type f) js/storage.ts js/riscbox.d.ts
RUST_SOURCES := $(shell find src riscbox-wasm tinyemu-core -type f)

all: dist

release:
	cargo build --release --workspace

test-unit: js
	cargo test --workspace
	uv run -q --script tests/test_splitimg.py
	node --test js/network.test.mjs js/riscbox.test.cjs
	node --test tests/release_plan.test.mjs
	node --test tests/ninep_wasm.test.mjs
	node --test tests/ninep_abi.test.mjs

test: test-unit wasm
	node --test tests/network_browser.test.mjs
	RISCBOX_TEST_BROWSER=1 node --test tests/ninep_wasm.test.mjs
	RISCBOX_TEST_BROWSER=1 node --test tests/ninep_abi.test.mjs

check: test
	$(MAKE) js-check
	cargo clippy --all-targets --workspace -- -D warnings
	uvx --quiet ty check tools/splitimg.py

check-release:
	$(MAKE) check
	$(MAKE) dist
	node tools/check_release.mjs $(ARCHIVE)

demo:
	$(MAKE) -C demo

test-demo:
	$(MAKE) -C demo test

wasm: $(RUSTBOX_WASM)

js: build/js/.built

node_modules/.package-lock.json: package.json package-lock.json
	npm ci

build/js/.built: js/tsconfig.json $(JS_SOURCES) js/riscbox.js tools/build_adapter.mjs node_modules/.package-lock.json
	rm -rf build/js/p9 build/js/block
	node_modules/.bin/tsc -p js/tsconfig.json
	node tools/build_adapter.mjs
	@mkdir -p build/js
	@touch $@

js-check: js
	node_modules/.bin/tsc -p js/tsconfig.json --noEmit
	node_modules/.bin/tsc --noEmit --strict --target ES2022 --module ES2022 --moduleResolution node --lib ES2023,DOM tests/adapter_client.ts

$(RUSTBOX_WASM): Cargo.toml Cargo.lock build.rs riscbox-wasm/Cargo.toml $(RUST_SOURCES)
	cargo build --release -p riscbox-wasm --target wasm32-unknown-unknown
	wasm-opt -O3 -o $@ $@

kernel:
	$(MAKE) -C kernel

opensbi:
	$(MAKE) -C opensbi

uboot:
	$(MAKE) -C uboot

dist:
	$(MAKE) wasm js kernel opensbi uboot
	$(MAKE) $(ARCHIVE)

release-path:
	@printf '%s\n' '$(abspath $(ARCHIVE))'

$(ARCHIVE): Makefile $(RUSTBOX_WASM) build/js/.built js/riscbox.js kernel/.asset-name opensbi/.asset-name uboot/.asset-name .github/scripts/package-release.sh README.md STORAGE-ABI.md NINEP.md API.md HOWTO.md CHANGELOG.md LICENSE tools/splitimg.py js/storage.ts js/network/README.md
	.github/scripts/package-release.sh $@

clean:
	cargo clean
	$(MAKE) -C kernel clean
	$(MAKE) -C opensbi clean
	$(MAKE) -C uboot clean
	rm -rf build/releases build/js

clean-all: clean

.PHONY: all release test-unit test check check-release demo test-demo wasm js js-check kernel opensbi uboot dist release-path clean clean-all
