.PHONY: build test lint fmt package set-env deps clean distclean

# Top-level verbs. `make` with no args runs `build` -- it is the first
# target declared below.
#
#   make build         # compile Rust + C++ + UI
#   make test          # run all language test suites
#   make lint          # fmt-check + clippy + clang-format + ruff + tsc/eslint
#   make fmt           # cargo fmt + clang-format -i + ruff format  (write mode)
#   make package       # ./build.sh all -> deb + wheel (does NOT compile)
#   make set-env       # print shell exports for the in-tree build
#                      # (use as: `eval "$(make set-env)"`)
#   make deps          # apt prerequisites for the chosen target
#   make clean         # remove build outputs (target/, build dirs, dist, staging)
#   make distclean     # clean + drop regenerable caches (node_modules, nested target/)
#
# Per-language fan-outs (target named <lang>-<verb>; same parameter semantics):
#
#   make rust-build  rust-test  rust-lint  rust-fmt
#   make cpp-build   cpp-test   cpp-lint   cpp-fmt
#   make ui-install  ui-build   ui-test    ui-lint
#   make python-test python-lint python-fmt
#
# Two parameters drive every verb:
#
#   TARGET_ARCH = amd64 (default: host) | arm64
#                 Cross-compile target arch.
#   BUILD_TYPE  = debug (default) | release
#                 Compiler optimization profile. Mapped to cargo's
#                 --release flag, cmake's CMAKE_BUILD_TYPE, and the
#                 corresponding target/<triple>/{debug,release}/ subdir.
#
# Examples:
#   make                                        # debug, host
#   make BUILD_TYPE=release                     # release, host
#   make TARGET_ARCH=arm64                      # debug, arm64 cross
#   make BUILD_TYPE=release TARGET_ARCH=arm64   # release, arm64 cross
#
# `make package` is a *pure* packaging step -- it does NOT recompile.
# Run `make` (with the same BUILD_TYPE/TARGET_ARCH) first; if any
# expected artifact is missing, ./build.sh aborts with a helpful error.

# ── Build profile (BUILD_TYPE) ───────────────────────────────────────
# debug = cargo's default (faster compile, larger + slower binaries).
# release = -O3 / -DNDEBUG (slower compile, optimized binaries; what
# packaged debs typically ship). Mapped to:
#   * CARGO_BUILD_ARGS         -- empty (debug) or --release
#   * BUILD_PROFILE_SUBDIR     -- target/<triple>/debug or .../release
#   * CMAKE_BUILD_TYPE         -- Debug or Release

BUILD_TYPE ?= debug

ifeq ($(BUILD_TYPE),debug)
CARGO_BUILD_PROFILE   :=
BUILD_PROFILE_SUBDIR  := debug
CMAKE_BUILD_TYPE      := Debug
else ifeq ($(BUILD_TYPE),release)
CARGO_BUILD_PROFILE   := --release
BUILD_PROFILE_SUBDIR  := release
CMAKE_BUILD_TYPE      := Release
else
$(error Unsupported BUILD_TYPE=$(BUILD_TYPE); supported: debug release)
endif

# `CARGO_BUILD_ARGS` is the historical override knob for "extra cargo
# args". Default it to whatever BUILD_TYPE picked; users can still
# override (e.g. `make CARGO_BUILD_ARGS=--locked`).
CARGO_BUILD_ARGS ?= $(CARGO_BUILD_PROFILE)

# ── Target architecture (TARGET_ARCH) ────────────────────────────────
# Defaults to the host arch. Set TARGET_ARCH=arm64 to cross-compile to
# linux/aarch64 via cmake/aarch64-linux-gnu.cmake + the env-var-only
# Cargo wiring in .cargo/config.toml. Adding more arches: register
# here, drop a matching toolchain file under cmake/, extend the :<arch>
# apt list in `deps` below.

TARGET_ARCH ?= $(shell dpkg --print-architecture 2>/dev/null || echo amd64)
HOST_ARCH   := $(shell dpkg --print-architecture 2>/dev/null || echo amd64)

ifeq ($(TARGET_ARCH),amd64)
RUST_TARGET          :=
CARGO_TARGET_ARGS    :=
CMAKE_TOOLCHAIN_ARGS := -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++
NPM_PLATFORM_ARGS    :=
DEB_ARCH             := amd64
TARGET_BUILD_DIR     := target/$(BUILD_PROFILE_SUBDIR)
else ifeq ($(TARGET_ARCH),arm64)
RUST_TARGET          := aarch64-unknown-linux-gnu
CARGO_TARGET_ARGS    := --target $(RUST_TARGET)
CMAKE_TOOLCHAIN_ARGS := -DCMAKE_TOOLCHAIN_FILE=$(CURDIR)/cmake/aarch64-linux-gnu.cmake -DLIBUSB_LIB=/usr/lib/aarch64-linux-gnu/libusb-1.0.so -DLIBUSB_INC=/usr/include/libusb-1.0
NPM_PLATFORM_ARGS    := --cpu=arm64 --os=linux --libc=glibc
DEB_ARCH             := arm64
TARGET_BUILD_DIR     := target/$(RUST_TARGET)/$(BUILD_PROFILE_SUBDIR)
else
$(error Unsupported TARGET_ARCH=$(TARGET_ARCH); supported: amd64 arm64)
endif

# CMake build dir is per-arch + per-profile so all four (arch, profile)
# permutations coexist without blowing each other's cache away.
CAMERAS_BUILD_DIR := cameras/build-$(TARGET_ARCH)-$(BUILD_TYPE)

# Parallel jobs: cargo, cmake, and the airbot_play_rust Pinocchio cmake
# sub-build (`AIRBOT_PINOCCHIO_BUILD_JOBS`) all share this knob. Lower it
# if rustc + native C++ thrashes CPU or OOMs.
ifeq ($(origin CMAKE_BUILD_JOBS),command line)
BUILD_JOBS := $(CMAKE_BUILD_JOBS)
else
BUILD_JOBS ?= $(shell nproc 2>/dev/null || echo 4)
endif
ifeq ($(strip $(BUILD_JOBS)),)
BUILD_JOBS := 4
endif

# ── Aggregate verbs ──────────────────────────────────────────────────

build: rust-build cpp-build ui-build

# `cargo test` / `ctest` / `npm test` / `pytest` cannot run aarch64
# binaries on an amd64 host without a runner shim, so cross-target tests
# are compile-only. Each language verb decides its own behavior.
test: rust-test cpp-test ui-test python-test

lint: rust-lint cpp-lint ui-lint python-lint

fmt: rust-fmt cpp-fmt python-fmt

# `./build.sh all` is a *pure* packaging step -- it stages already-built
# artifacts into a deb + builds the Nero wheel. It deliberately does NOT
# depend on `build`; if you want to compile then pack, run
# `make build && make package` (with the same BUILD_TYPE/TARGET_ARCH).
# `./build.sh assert_built` errors with a helpful message if any expected
# artifact is missing under TARGET_DIR / CAMERAS_BUILD_DIR.
package:
	DEB_ARCH=$(DEB_ARCH) \
	  TARGET_DIR=$(TARGET_BUILD_DIR) \
	  CAMERAS_BUILD_DIR=$(CAMERAS_BUILD_DIR) \
	  ./build.sh all

# Print shell `export` lines that put the freshly-built dev binaries on
# PATH and point ROLLIO_SHARE_DIR at the in-tree ui/web/dist bundle.
# Honours BUILD_TYPE / TARGET_ARCH, so release / cross builds resolve to
# the right target/ subdir. Use it from your shell as:
#
#     eval "$$(make set-env)"
#
# After that, `rollio collect --config ./config.toml` runs straight out
# of the working tree without `make package` or installing into /usr.
set-env:
	@echo 'export PATH="$(CURDIR)/$(TARGET_BUILD_DIR):$$PATH"'
	@echo 'export ROLLIO_SHARE_DIR="$(CURDIR)"'

# Apt-side prereqs. The host (amd64) list is always installed; arm64
# layers the cross toolchain + multiarch :arm64 dev libs on top.
# Rust (`cargo`/`rustc`) and Node (`node`/`npm`) come from rustup/nvm/etc.
# `uv` (for the Nero wheel) installs separately, e.g. `pipx install uv`.
deps:
	@bash $(CURDIR)/scripts/check-cross-apt.sh $(TARGET_ARCH)
	sudo apt-get update
	# Each entry below maps onto something concrete the build needs:
	#   patch              -- apply-vendored-patches (Pinocchio static + ffmpeg-sys-next cross)
	#   dpkg-dev, file     -- dpkg-deb / dpkg-shlibdeps / ELF detection in build.sh
	#   cmake, ninja-build -- C++ camera build + airbot Pinocchio sub-build
	#   pkg-config         -- pkg-config crate + cmake's FindPkgConfig
	#   nasm               -- turbojpeg-sys (visualizer) + ffmpeg-sys-next assembler
	#   clang, lld         -- canonical C/C++ compiler + linker (host & cross)
	#   clang-format       -- `make cpp-lint` / `make cpp-fmt`
	#   libclang-dev       -- bindgen runtime (loads libclang.so)
	#   llvm               -- unversioned `llvm-ar` on PATH; AR_<target>=llvm-ar
	#                         in .cargo/config.toml. llvm-NN alone only ships
	#                         the suffixed binary (llvm-ar-18).
	#   libstdc++-13-dev   -- C++ headers clang searches (and owns
	#                         /usr/lib/gcc/x86_64-linux-gnu/13/libstdc++.so)
	#   libav*-dev         -- ffmpeg-next dynamic link (rollio-encoder)
	#   liburdfdom-dev     -- airbot Pinocchio (auto-pulls libconsole-bridge-dev
	#                         and liburdfdom-headers-dev)
	#   libtinyxml2-dev    -- urdfdom link dep; explicit for the target-arch
	#                         linker symlink (libtinyxml2.so)
	#   libeigen3-dev      -- pinocchio + cxx-build include
	#   libboost-*-dev     -- pinocchio link deps
	#   libusb-1.0-0-dev,
	#   libudev-dev        -- librealsense static link (cameras/realsense). libudev-dev
	#                         transitively pulls libudev1, which apt keeps version-locked
	#                         with systemd-dev (same source pkg) -- harmless side effect.
	#   git                -- cmake FetchContent of jrl-cmakemodules in pinocchio
	sudo apt-get install -y --no-install-recommends \
	  patch dpkg-dev file \
	  cmake ninja-build pkg-config nasm \
	  clang clang-format lld libclang-dev llvm \
	  libstdc++-13-dev \
	  libavcodec-dev libavformat-dev libavutil-dev libavfilter-dev \
	  libavdevice-dev libswscale-dev \
	  liburdfdom-dev \
	  libtinyxml2-dev \
	  libeigen3-dev \
	  libboost-filesystem-dev libboost-serialization-dev \
	  libusb-1.0-0-dev libudev-dev \
	  git
ifeq ($(TARGET_ARCH),arm64)
	# Cross-arch validation already ran via scripts/check-cross-apt.sh
	# at the top of this recipe (it auto-skips on native targets).
	#
	# crossbuild-essential-arm64 supplies binutils-aarch64-linux-gnu
	# (ar, ranlib, strip, ld) AND populates the cross sysroot at
	# /usr/aarch64-linux-gnu/ via libc6-dev-arm64-cross + libstdc++-13-dev-arm64-cross.
	# clang --target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu
	# resolves through that tree (see cmake/aarch64-linux-gnu.cmake +
	# .cargo/config.toml). The unused gcc-aarch64-linux-gnu binary is
	# the cost of getting the sysroot in one apt call.
	#
	# Note: Ubuntu 24.04 has no `pkg-config-aarch64-linux-gnu` package.
	# Cross pkg-config is handled via PKG_CONFIG_LIBDIR_<target> in
	# .cargo/config.toml + the `find_program(... NAMES ... pkg-config)`
	# fallback in cmake/aarch64-linux-gnu.cmake.
	sudo apt-get install -y --no-install-recommends \
	  crossbuild-essential-arm64 \
	  qemu-user-static binfmt-support dpkg-cross \
	  libstdc++-13-dev:arm64 \
	  libavcodec-dev:arm64 libavformat-dev:arm64 \
	  libavutil-dev:arm64 libavfilter-dev:arm64 \
	  libavdevice-dev:arm64 libswscale-dev:arm64 \
	  liburdfdom-dev:arm64 \
	  libtinyxml2-dev:arm64 \
	  libeigen3-dev:arm64 \
	  libboost-filesystem-dev:arm64 libboost-serialization-dev:arm64 \
	  libusb-1.0-0-dev:arm64 libudev-dev:arm64
endif

# ── Cleanup ──────────────────────────────────────────────────────────
# `clean` keeps caches that meaningfully speed up subsequent builds
# (node_modules, nested non-workspace target/ dirs); `distclean` drops
# them too, leaving the tree at "fresh-checkout" baseline.

clean:
	cargo clean
	rm -rf cameras/build cameras/build-*
	rm -rf ui/terminal/dist ui/terminal/native ui/terminal/.deb-vendor
	rm -rf ui/web/dist
	rm -rf .deb-staging dist
	rm -rf logs/ output/ rollio-logs/

distclean: clean
	@# Rust: remove sibling target/ for every Cargo.toml (skips .git, node_modules, target, vendor).
	@echo "Removing nested Rust target/ directories..."
	@find . \( -name .git -o -name node_modules -o -name target -o -name vendor \) -prune -o \
		-name Cargo.toml -print0 | \
		xargs -0 -n1 sh -c 'd=$$(dirname "$$1"); if [ -d "$$d/target" ]; then rm -rf "$$d/target"; fi' sh
	@# Node: remove sibling node_modules/ for every package.json (same skips minus vendor).
	@echo "Removing nested Node node_modules/ directories..."
	@find . \( -name .git -o -name node_modules -o -name target \) -prune -o \
		-name package.json -print0 | \
		xargs -0 -n1 sh -c 'd=$$(dirname "$$1"); if [ -d "$$d/node_modules" ]; then rm -rf "$$d/node_modules"; fi' sh

# ── Per-language sub-targets ─────────────────────────────────────────
# Composed into the aggregate verbs above; also runnable individually
# (e.g. `make rust-lint`, `make ui-test`) for fast single-language loops.

.PHONY: apply-vendored-patches apply-airbot-pinocchio-patch apply-ffmpeg-sys-cross-patch
.PHONY: rust-build rust-test rust-lint rust-fmt
.PHONY: cpp-build  cpp-test  cpp-lint  cpp-fmt
.PHONY: ui-install ui-build  ui-test   ui-lint
.PHONY: python-test python-lint python-fmt

# Vendored upstream patches. Each `apply-*-patch` target is idempotent
# (checks for a marker that's only present after the patch was applied;
# skips if found). `apply-vendored-patches` is the aggregate that the
# Rust build/test/lint recipes depend on.
apply-vendored-patches: apply-airbot-pinocchio-patch apply-ffmpeg-sys-cross-patch

# airbot_play_rust builds vendored Pinocchio; without this patch the libs
# are shared-only, rollio-device-airbot-play links libpinocchio_*.so, and
# dpkg-shlibdeps fails when packing the deb. The patch lives in the
# submodule; see third_party/airbot-play-rust/README.md.
AIRBOT_PINOCCHIO_PATCH := third_party/airbot-play-rust/patches/pinocchio-static-build.patch
AIRBOT_PINOCCHIO_DIR   := third_party/airbot-play-rust/third_party/pinocchio

apply-airbot-pinocchio-patch:
	@test -f "$(AIRBOT_PINOCCHIO_PATCH)" \
		|| (echo "missing $(AIRBOT_PINOCCHIO_PATCH)" >&2; exit 1)
	@test -d "$(AIRBOT_PINOCCHIO_DIR)" \
		|| (echo "missing $(AIRBOT_PINOCCHIO_DIR); run git submodule update --init --recursive" >&2; exit 1)
	@if ! grep -Fq 'option(BUILD_SHARED_LIBS "Build Pinocchio libraries as shared objects" ON)' "$(AIRBOT_PINOCCHIO_DIR)/CMakeLists.txt"; then \
		patch --batch --no-backup-if-mismatch -d "$(AIRBOT_PINOCCHIO_DIR)" -p1 < "$(AIRBOT_PINOCCHIO_PATCH)"; \
	fi

# ffmpeg-sys-next 8.1.0 hard-codes the version-detection probe to compile
# for the host arch (`.target(env::var("HOST"))`). Cross builds need it
# compiled for the target arch instead -- on Linux hosts with
# qemu-user-static + binfmt_misc registered (installed by `make deps
# TARGET_ARCH=arm64`), the resulting target ELF runs transparently via
# QEMU. Workspace Cargo.toml's [patch.crates-io] points
# `ffmpeg-sys-next` at the vendored submodule; this target applies the
# one-line fix to that submodule's build.rs.
FFMPEG_SYS_CROSS_PATCH := patches/ffmpeg-sys-next-cross-target.patch
FFMPEG_SYS_DIR         := third_party/ffmpeg-sys-next

apply-ffmpeg-sys-cross-patch:
	@test -f "$(FFMPEG_SYS_CROSS_PATCH)" \
		|| (echo "missing $(FFMPEG_SYS_CROSS_PATCH)" >&2; exit 1)
	@test -d "$(FFMPEG_SYS_DIR)" \
		|| (echo "missing $(FFMPEG_SYS_DIR); run git submodule update --init --recursive" >&2; exit 1)
	@# Idempotency marker: the patched line replaces .target(env::var("HOST"))
	@# with .target(env::var("TARGET")). We grep for the latter at the
	@# call site (note the `.target(&env::var(` prefix -- it disambiguates
	@# from the unrelated `let target = env::var("TARGET").unwrap();` at
	@# build.rs:295).
	@if ! grep -Fq '.target(&env::var("TARGET").unwrap())' "$(FFMPEG_SYS_DIR)/build.rs"; then \
		patch --batch --no-backup-if-mismatch -d "$(FFMPEG_SYS_DIR)" -p1 < "$(FFMPEG_SYS_CROSS_PATCH)"; \
	fi

# Rust ----------------------------------------------------------------

rust-build: apply-vendored-patches
	AIRBOT_PINOCCHIO_BUILD_JOBS=$(BUILD_JOBS) \
		cargo build --workspace --exclude rollio-encoder-x5 $(CARGO_BUILD_ARGS) $(CARGO_TARGET_ARGS) -j $(BUILD_JOBS)

rust-test: apply-vendored-patches
ifeq ($(TARGET_ARCH),$(HOST_ARCH))
	AIRBOT_PINOCCHIO_BUILD_JOBS=$(BUILD_JOBS) \
		cargo test --workspace --exclude rollio-encoder-x5 -j $(BUILD_JOBS)
else
	@echo "rust-test: cross TARGET_ARCH=$(TARGET_ARCH) on HOST_ARCH=$(HOST_ARCH); compile-only (no native runner)"
	AIRBOT_PINOCCHIO_BUILD_JOBS=$(BUILD_JOBS) \
		cargo test --workspace --exclude rollio-encoder-x5 --no-run $(CARGO_TARGET_ARGS) -j $(BUILD_JOBS)
endif

# rustfmt.toml uses the unstable `ignore` directive (skips third_party/),
# so fmt-check must run on nightly to honor it. Without nightly, stable
# rustfmt would try to reformat the vendored submodules. CI installs
# nightly rustfmt explicitly; locally, error with a clear message if it's
# missing.
rust-lint: apply-vendored-patches
	@if cargo +nightly fmt --version >/dev/null 2>&1; then \
		cargo +nightly fmt --all -- --check; \
	else \
		echo "rust-lint: nightly rustfmt missing; install with"; \
		echo "  rustup toolchain install nightly --profile minimal --component rustfmt"; \
		exit 1; \
	fi
	AIRBOT_PINOCCHIO_BUILD_JOBS=$(BUILD_JOBS) \
		cargo clippy --workspace --exclude rollio-encoder-x5 $(CARGO_TARGET_ARGS) -j $(BUILD_JOBS) -- -D warnings

rust-fmt:
	@if cargo +nightly fmt --version >/dev/null 2>&1; then \
		cargo +nightly fmt --all; \
	else \
		echo "rust-fmt: nightly rustfmt missing; install with"; \
		echo "  rustup toolchain install nightly --profile minimal --component rustfmt"; \
		exit 1; \
	fi

# C++ -----------------------------------------------------------------

cpp-build:
	cmake -B $(CAMERAS_BUILD_DIR) -S cameras $(CMAKE_TOOLCHAIN_ARGS) \
	  -DCMAKE_BUILD_TYPE=$(CMAKE_BUILD_TYPE)
	cmake --build $(CAMERAS_BUILD_DIR) --parallel $(BUILD_JOBS)

cpp-test: cpp-build
ifeq ($(TARGET_ARCH),$(HOST_ARCH))
	ctest --test-dir $(CAMERAS_BUILD_DIR) --output-on-failure
else
	@echo "cpp-test: cross TARGET_ARCH=$(TARGET_ARCH) on HOST_ARCH=$(HOST_ARCH); compile-only (no native runner)"
endif

# clang-format check / write-back over our C++ tree (cameras/* + cpp/common/*).
# Mirrors the existing CI step. xargs -r so an empty find is not an error
# (e.g. on a sparse checkout that excludes cameras/).
CPP_FORMAT_GLOB := cameras cpp/common

cpp-lint:
	@if command -v clang-format >/dev/null 2>&1; then \
		find $(CPP_FORMAT_GLOB) \( -name '*.cpp' -o -name '*.h' -o -name '*.hpp' \) -print0 \
			| xargs -0 -r clang-format --dry-run --Werror --style=file; \
	else \
		echo "cpp-lint: clang-format not on PATH; install via 'make deps'"; exit 1; \
	fi

cpp-fmt:
	@if command -v clang-format >/dev/null 2>&1; then \
		find $(CPP_FORMAT_GLOB) \( -name '*.cpp' -o -name '*.h' -o -name '*.hpp' \) -print0 \
			| xargs -0 -r clang-format -i --style=file; \
	else \
		echo "cpp-fmt: clang-format not on PATH; install via 'make deps'"; exit 1; \
	fi

# UI ------------------------------------------------------------------
# ui/terminal carries the Ink CLI plus the native ASCII N-API addon (cross
# flags matter -- `sharp` ships per-arch native bindings under
# @img/sharp-linux-* and the addon is a Rust cdylib). ui/web is a pure
# Vite/React web app -- no native bindings of our own, but Vite's bundler
# `rolldown` ships per-platform native bindings via optionalDependencies.
#
# `npm install --include=optional` is used instead of `npm ci` for ui/web
# because npm 11's `npm ci` follows package-lock.json strictly: if the
# lockfile was committed without the host's specific @rolldown/binding-*
# entry (a real bug we've hit on linux-x64-gnu), `npm ci` skips it and
# the subsequent `npm run build` errors with MODULE_NOT_FOUND. `npm
# install` resolves optionals fresh against the host's cpu/os/libc.
#
# ui/terminal stays on `npm ci` -- its lockfile carries the right sharp
# entries because we explicitly seed them with --cpu/--os/--libc.

ui-install:
	cd ui/terminal && npm ci --include=optional $(NPM_PLATFORM_ARGS)
	cd ui/web && npm install --include=optional --no-audit --no-fund

ui-build: ui-install
	cd ui/terminal && ROLLIO_NATIVE_TARGET=$(RUST_TARGET) npm run build
	cd ui/web && npm run build

ui-test: ui-install
ifeq ($(TARGET_ARCH),$(HOST_ARCH))
	cd ui/terminal && npm test
	cd ui/web && npm test
else
	@echo "ui-test: cross TARGET_ARCH=$(TARGET_ARCH) on HOST_ARCH=$(HOST_ARCH); skipped (no native runner)"
endif

# UI lint = TS typecheck for the Ink CLI + eslint for the web app.
ui-lint: ui-install
	cd ui/terminal && npm run typecheck
	cd ui/web && npm run lint

# Python --------------------------------------------------------------

python-test:
ifeq ($(TARGET_ARCH),$(HOST_ARCH))
	PYTHONPATH="$(CURDIR)/robots/airbot_play/src" python3 -m pytest robots/airbot_play/tests
else
	@echo "python-test: cross TARGET_ARCH=$(TARGET_ARCH) on HOST_ARCH=$(HOST_ARCH); skipped (no native runner)"
endif

python-lint:
	ruff check .
	ruff format --check .

python-fmt:
	ruff format .
