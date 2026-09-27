RUSTBOX_WASM=target/wasm32-unknown-unknown/release/riscbox_wasm.wasm

all: release

release:
	cargo build --release --workspace

test-unit: js
	cargo test --workspace
	uv run -q --script tests/test_splitimg.py
	uv run -q --script tests/test_image_deployment.py
	node --test js/block.test.mjs js/network.test.mjs js/p9.test.mjs js/riscbox.test.cjs

test: test-unit wasm
	node --test tests/network_browser.test.mjs

check: test
	$(MAKE) js-check
	cargo clippy --all-targets --workspace -- -D warnings
	uvx --quiet ty check tools/splitimg.py tools/image_deployment.py

wasm: $(RUSTBOX_WASM)

js:
	images/risclet/ui/node_modules/.bin/tsc -p js/tsconfig.json

js-check:
	images/risclet/ui/node_modules/.bin/tsc -p js/tsconfig.json --noEmit

$(RUSTBOX_WASM): Cargo.toml Cargo.lock riscbox-wasm/Cargo.toml $(shell find src riscbox-wasm -type f)
	cargo build --release -p riscbox-wasm --target wasm32-unknown-unknown

kernel:
	$(MAKE) -C kernel

opensbi:
	$(MAKE) -C opensbi

uboot:
	$(MAKE) -C uboot

dist: wasm js kernel opensbi uboot

clean:
	cargo clean

clean-all: clean
	$(MAKE) -C kernel clean
	$(MAKE) -C opensbi clean
	$(MAKE) -C uboot clean

.PHONY: all release test-unit test check wasm js js-check kernel opensbi uboot dist clean clean-all
