RUSTBOX_WASM=target/wasm32-unknown-unknown/release/riscbox_wasm.wasm
VERSION := $(shell python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
ARCHIVE := build/releases/riscbox-$(VERSION).tar.gz
JS_SOURCES := $(shell find js/block js/network js/p9 -name '*.ts' -type f)
RUST_SOURCES := $(shell find src riscbox-wasm tinyemu-core -type f)

all: dist

release:
	cargo build --release --workspace

test-unit: js
	cargo test --workspace
	uv run -q --script tests/test_splitimg.py
	uv run -q --script tests/test_image_deployment.py
	node --test js/block.test.mjs js/network.test.mjs js/p9.test.mjs js/riscbox.test.cjs
	node --test tests/risclet_input.test.mjs
	node --test tests/ninep_wasm.test.mjs
	node --test tests/ninep_abi.test.mjs

test: test-unit wasm
	node --test tests/network_browser.test.mjs
	RISCBOX_TEST_BROWSER=1 node --test tests/ninep_wasm.test.mjs
	RISCBOX_TEST_BROWSER=1 node --test tests/ninep_abi.test.mjs

check: test
	$(MAKE) js-check
	cargo clippy --all-targets --workspace -- -D warnings
	uvx --quiet ty check tools/splitimg.py tools/image_deployment.py

test-images:
	$(MAKE) -C images/risclet
	$(MAKE) -C images/alpine
	$(MAKE) -C images/xv6-profile
	cargo test --release --test platform_acceptance alpine_reaches_login_and_shuts_down -- --ignored
	node --test tests/risclet_browser.test.mjs

wasm: $(RUSTBOX_WASM)

js: build/js/.built

build/js/.built: js/tsconfig.json $(JS_SOURCES)
	rm -f build/js/p9/session.js build/js/p9/session.d.ts
	images/risclet/ui/node_modules/.bin/tsc -p js/tsconfig.json
	@mkdir -p build/js
	@touch $@

js-check:
	images/risclet/ui/node_modules/.bin/tsc -p js/tsconfig.json --noEmit

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

$(ARCHIVE): Makefile $(RUSTBOX_WASM) build/js/.built js/riscbox.js kernel/.asset-name opensbi/.asset-name uboot/.asset-name .github/scripts/package-release.sh README.md CHANGELOG.md LICENSE js/p9/README.md js/network/README.md
	.github/scripts/package-release.sh $@

clean:
	cargo clean
	$(MAKE) -C kernel clean
	$(MAKE) -C opensbi clean
	$(MAKE) -C uboot clean
	rm -rf build/releases build/js

clean-all: clean

.PHONY: all release test-unit test check test-images wasm js js-check kernel opensbi uboot dist clean clean-all
