.PHONY: all build test clean fmt lint smoke-pseudo package package-all package-deps wheel
.PHONY: apply-airbot-pinocchio-patch
.PHONY: rust cpp ui python
.PHONY: rust-build rust-test rust-fmt rust-lint
.PHONY: cpp-build cpp-test ui-build ui-install ui-test ui-bench-ascii
.PHONY: python-test python-lint

# Default to release binaries; override with `make build CARGO_BUILD_ARGS=` for debug.
CARGO_BUILD_ARGS ?= --release
CARGO_RUN_ARGS ?= $(CARGO_BUILD_ARGS)

all: build

# ── Aggregate targets ────────────────────────────────────────────────

build: rust-build cpp-build ui-build

test: rust-test cpp-test ui-test python-test

clean:
	cargo clean
	rm -rf cpp/build
	rm -rf cameras/build
	rm -rf ui/terminal/dist
	rm -rf ui/terminal/native
	rm -rf ui/web/dist
	rm -rf .deb-staging dist

fmt: rust-fmt

lint: rust-lint python-lint

# ── Rust ─────────────────────────────────────────────────────────────

rust: rust-build

# airbot_play_rust builds vendored Pinocchio; without this patch the libs are
# shared-only, rollio-device-airbot-play links libpinocchio_*.so, and
# dpkg-shlibdeps fails when packing the deb. The patch lives in the submodule;
# see third_party/airbot-play-rust/README.md.
AIRBOT_PINOCCHIO_PATCH := third_party/airbot-play-rust/patches/pinocchio-static-build.patch
AIRBOT_PINOCCHIO_DIR := third_party/airbot-play-rust/third_party/pinocchio

apply-airbot-pinocchio-patch:
	@test -f "$(AIRBOT_PINOCCHIO_PATCH)" || (echo "missing $(AIRBOT_PINOCCHIO_PATCH)" >&2; exit 1)
	@test -d "$(AIRBOT_PINOCCHIO_DIR)" || (echo "missing $(AIRBOT_PINOCCHIO_DIR); run git submodule update --init --recursive" >&2; exit 1)
	@if ! grep -Fq 'option(BUILD_SHARED_LIBS "Build Pinocchio libraries as shared objects" ON)' "$(AIRBOT_PINOCCHIO_DIR)/CMakeLists.txt"; then \
		patch --batch -d "$(AIRBOT_PINOCCHIO_DIR)" -p1 < "$(AIRBOT_PINOCCHIO_PATCH)"; \
	fi

rust-build: apply-airbot-pinocchio-patch
	cargo build --workspace $(CARGO_BUILD_ARGS)

rust-test: apply-airbot-pinocchio-patch
	cargo test --workspace

rust-fmt:
	cargo fmt --all

rust-lint: apply-airbot-pinocchio-patch
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
	cd ui/terminal && npm install
	cd ui/web && npm install

ui-build: ui-install
	cd ui/terminal && npm run build
	cd ui/web && npm run build

ui-test: ui-install
	cd ui/terminal && npm test
	cd ui/web && npm test

ui-bench-ascii: ui-install
	cd ui/terminal && npm run bench:ascii

# ── Python ────────────────────────────────────────────────────────────

python: python-test

python-test:
	PYTHONPATH="$(CURDIR)/robots/airbot_play/src" python3 -m pytest robots/airbot_play/tests

python-lint:
	ruff check .
	ruff format --check .

# ── Smoke ─────────────────────────────────────────────────────────────

smoke-pseudo: build
	cargo run $(CARGO_RUN_ARGS) -p rollio -- collect -c config/config.example.toml

# ── Packaging ─────────────────────────────────────────────────────────
# All packaging logic lives in ./build.sh. These targets are thin wrappers.
# Run `make build` before `make package` (or use `make package-all`).

package:
	./build.sh all

package-all: build package

wheel:
	./build.sh nero

# Optional: install apt helpers (dpkg-deb/shlibdeps + ffmpeg + bindgen toolchain).
# Rust (`cargo`/`rustc`) and Node (`node`/`npm`) come from rustup/nvm/etc.
# `uv` (for the Nero wheel) install separately, e.g. `pipx install uv`.
package-deps:
	sudo apt-get update
	sudo apt-get install -y --no-install-recommends \
	  patch build-essential dpkg-dev \
	  cmake pkg-config nasm clang libclang-dev llvm-dev \
	  libavcodec-dev libavformat-dev libavutil-dev libswscale-dev \
	  liburdfdom-dev \
	  libconsole-bridge-dev \
	  libeigen3-dev \
	  libboost-filesystem-dev libboost-serialization-dev \
	  g++ git
