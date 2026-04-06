.PHONY: all build test clean fmt lint
.PHONY: rust cpp ui
.PHONY: rust-build rust-test rust-fmt rust-lint
.PHONY: cpp-build ui-build ui-install ui-test ui-bench-ascii

all: build

# ── Aggregate targets ────────────────────────────────────────────────

build: rust-build cpp-build ui-build

test: rust-test

clean:
	cargo clean
	rm -rf cpp/build
	rm -rf ui/dist

fmt: rust-fmt

lint: rust-lint

# ── Rust ─────────────────────────────────────────────────────────────

rust: rust-build

rust-build:
	cargo build --workspace

rust-test:
	cargo test --workspace

rust-fmt:
	cargo fmt --all

rust-lint:
	cargo clippy --workspace -- -D warnings

# ── C++ ──────────────────────────────────────────────────────────────

cpp: cpp-build

cpp-build:
	cmake -B cpp/build -S cpp
	cmake --build cpp/build

# ── UI ───────────────────────────────────────────────────────────────

ui: ui-build

ui-install:
	cd ui && npm install

ui-build: ui-install
	cd ui && npm run build

ui-test: ui-install
	cd ui && npm test

ui-bench-ascii: ui-install
	cd ui && npm run bench:ascii
