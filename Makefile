.PHONY: all build test clean fmt lint smoke-pseudo
.PHONY: rust cpp ui python
.PHONY: rust-build rust-test rust-fmt rust-lint
.PHONY: cpp-build cpp-test ui-build ui-install ui-test ui-bench-ascii
.PHONY: python-test python-lint

all: build

# ── Aggregate targets ────────────────────────────────────────────────

build: rust-build cpp-build ui-build

test: rust-test cpp-test ui-test python-test

clean:
	cargo clean
	rm -rf cpp/build
	rm -rf cameras/build
	rm -rf ui/dist
	rm -rf ui/native

fmt: rust-fmt

lint: rust-lint python-lint

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
	cmake -B cameras/build -S cameras -DCMAKE_CXX_COMPILER=g++
	cmake --build cameras/build

cpp-test: cpp-build
	ctest --test-dir cameras/build --output-on-failure

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

# ── Python ────────────────────────────────────────────────────────────

python: python-test

python-test:
	PYTHONPATH="$(CURDIR)/robots/airbot_play/src" python3 -m pytest robots/airbot_play/tests

python-lint:
	ruff check --exclude third_party .
	ruff format --check --exclude third_party .

# ── Smoke ─────────────────────────────────────────────────────────────

smoke-pseudo: build
	cargo run -p rollio -- collect -c config/config.example.toml
