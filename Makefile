RUSTBOX_WASM=target/wasm32-unknown-unknown/release/riscbox_wasm.wasm

all: release

release:
	cargo build --release --workspace

test:
	cargo test --workspace
	uv run -q --script tests/test_splitimg.py
	node --test js/p9.test.mjs js/riscbox.test.cjs

check: test
	cargo clippy --all-targets --workspace -- -D warnings
	uvx --quiet ty check tools/splitimg.py

wasm: $(RUSTBOX_WASM)

$(RUSTBOX_WASM): Cargo.toml Cargo.lock riscbox-wasm/Cargo.toml $(shell find src riscbox-wasm -type f)
	cargo build --release -p riscbox-wasm --target wasm32-unknown-unknown

kernel:
	$(MAKE) -C kernel

dist: wasm kernel

clean:
	cargo clean

clean-all: clean
	$(MAKE) -C kernel clean

.PHONY: all release test check wasm kernel dist clean clean-all
