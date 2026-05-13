# AGENTS.md

## Cursor Cloud specific instructions

### Overview

Rollio is a multi-process CLI framework for robotic teleoperation data collection. It is a Cargo workspace (Rust crates under the repo root and `test/`) + C++ camera drivers under `cameras/` with shared interop headers in `cpp/common/` + a TypeScript/React Ink terminal UI (`ui/terminal/`). All inter-process communication uses iceoryx2 (zero-copy shared-memory IPC) via a git submodule at `third_party/iceoryx2`. The `ascii-video-renderer` submodule lives at `third_party/ascii-video-renderer` and is used by the terminal UI’s native ASCII N-API addon (not a root workspace member).

The project is in early development — most binary crates are stubs. The `rollio-types` library crate has real integration tests for config parsing and message types.

### Build & test commands

All verbs accept `TARGET_ARCH=amd64|arm64` (defaults to the host's `dpkg --print-architecture`).

**Top-level verbs** (fan out across all four languages):

| Verb | What it does |
|------|--------------|
| `make build` | Compile Rust workspace + C++ camera drivers + UI bundle |
| `make test` | `cargo test` + `ctest` + `npm test` (`ui/terminal` + `ui/web`) + `pytest`. Compile-only / skipped when `TARGET_ARCH` != host. |
| `make lint` | `cargo +nightly fmt --check` + `cargo clippy -- -D warnings` + `clang-format --dry-run --Werror` + `tsc --noEmit` + `eslint` + `ruff check` + `ruff format --check` |
| `make fmt` | Write-mode formatters: `cargo +nightly fmt` + `clang-format -i` + `ruff format` |
| `make package` | `./build.sh all` -> `rollio_*_<arch>.deb` + `rollio_device_nero-*-py3-none-any.whl` under `dist/`. **Pure packaging**: does NOT recompile; run `make build` first with the same `BUILD_TYPE`/`TARGET_ARCH` |

Two parameters drive every verb:

- **`BUILD_TYPE`** = `debug` (default) or `release`. Maps to cargo's `--release` flag, cmake's `CMAKE_BUILD_TYPE`, and the corresponding `target/<triple>/{debug,release}/` subdir.
- **`TARGET_ARCH`** = `amd64` (default: host) or `arm64`. Cross-compile target.

Examples:

```
make                                       # debug, host
make BUILD_TYPE=release                    # release, host
make TARGET_ARCH=arm64                     # debug, arm64 cross
make BUILD_TYPE=release TARGET_ARCH=arm64  # release, arm64 cross
make package BUILD_TYPE=release            # pack release amd64 deb
make package BUILD_TYPE=release TARGET_ARCH=arm64
```
| `make deps` | One-time apt install of host build dependencies (sudo). Adds `:arm64` multiarch + `crossbuild-essential-arm64` when `TARGET_ARCH=arm64`. |
| `make clean` | Remove build outputs (`target/`, `cameras/build*/`, `dist/`, `.deb-staging/`, `ui/*/dist`). Keeps caches. |
| `make distclean` | `clean` + drop regenerable caches (every nested `target/`, `node_modules/`). |

**Per-language fan-outs** (same `TARGET_ARCH` semantics, useful for fast single-language loops):

| | `build` | `test` | `lint` | `fmt` |
|--|--|--|--|--|
| Rust | `rust-build` | `rust-test` | `rust-lint` | `rust-fmt` |
| C++ | `cpp-build` | `cpp-test` | `cpp-lint` | `cpp-fmt` |
| UI | `ui-build` (`ui-install` is the dep) | `ui-test` | `ui-lint` | (no formatter wired) |
| Python | (no compile step) | `python-test` | `python-lint` | `python-fmt` |

E.g. `make rust-lint`, `make ui-test`, `make cpp-fmt`. The aggregate verbs are just dispatchers (`lint = rust-lint cpp-lint ui-lint python-lint`).

`./build.sh` does no compiling -- it stages already-built artifacts. Set `DEB_ARCH`, `TARGET_DIR`, `CAMERAS_BUILD_DIR` via env (the Make recipe does this for you).

UI runtime: `cd ui/terminal && node dist/index.js` (after `make build`).

### Non-obvious caveats

- **Git submodules required:** Initialize `third_party/iceoryx2`, `third_party/ascii-video-renderer`, and `third_party/librealsense` (statically linked into `rollio-device-realsense`) before builds that compile the full stack. Run `git submodule update --init --recursive` if directories are empty.
- **C/C++ toolchain is clang.** Both the host build (Makefile drives `cmake -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++`) and the arm64 cross build (`cmake/aarch64-linux-gnu.cmake` -> clang `--target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu` + `-fuse-ld=lld`) use clang. `make deps` installs `clang`, `lld`, and `libstdc++-13-dev` (the C++ headers clang searches under `/usr/include/c++/13/`). `g++` stays in the apt list only so libstdc++ runtime + linker symlinks resolve.
- **Rust linker is clang.** [`.cargo/config.toml`](.cargo/config.toml) sets `linker = "clang"` for both the host (`x86_64-unknown-linux-gnu`) and the cross (`aarch64-unknown-linux-gnu`) target, with rustflags forwarding `--target` / `--sysroot` / `-fuse-ld=lld` for the cross build. cc-crate / cmake-rs / cxx-build pick up `CC_<target>` / `CXX_<target>` from the same file and route through clang.
- **Rust 1.88+:** The workspace requires Rust 1.88 or newer. The VM default may be older; use `rustup install 1.88.0 && rustup default 1.88.0` if needed.
- **libstdc++ symlink:** If clang's linker still fails with `cannot find -lstdc++` after `make deps` (rare, but reported on minimal Ubuntu containers where `g++` is missing), create it manually: `sudo ln -sf /usr/lib/gcc/x86_64-linux-gnu/13/libstdc++.so /usr/lib/x86_64-linux-gnu/libstdc++.so`.
- **No Docker or external services required.** The entire stack runs locally without databases, containers, or network services.

### Cross-compile (aarch64)

Opt-in path that produces a `linux/arm64` `.deb` on an x86_64 Ubuntu 24.04 host. The amd64 build is unchanged.

- **One-time setup:** `make deps TARGET_ARCH=arm64` (installs `crossbuild-essential-arm64`, `qemu-user-static`, `dpkg-cross`, `pkg-config-aarch64-linux-gnu`, and the `:arm64` multiarch dev libs that bindgen / pkg-config / cmake-rs probe). The apt step does not touch rustup -- run `rustup target add aarch64-unknown-linux-gnu` separately.
- **Build:** `make BUILD_TYPE=release TARGET_ARCH=arm64` (Rust + C++ + UI all cross-built; release for shipping).
- **Pack:** `make package BUILD_TYPE=release TARGET_ARCH=arm64` produces `dist/rollio_<ver>_arm64.deb` from the already-built artifacts (no recompile). Set `ROLLIO_DEB_SHLIBDEPS_CHROOT=/path/to/arm64-rootfs` to run the dpkg-shlibdeps step inside a hermetic chroot instead of relying on the host's multiarch admin DB.
- **Cross plumbing:** [`.cargo/config.toml`](.cargo/config.toml) carries the clang linker, `BINDGEN_EXTRA_CLANG_ARGS_aarch64-unknown-linux-gnu`, `PKG_CONFIG_LIBDIR_aarch64-unknown-linux-gnu`, and the `--target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu -fuse-ld=lld` flags baked into `CC_<target>`/`CXX_<target>`. CMake uses [`cmake/aarch64-linux-gnu.cmake`](cmake/aarch64-linux-gnu.cmake). The Pinocchio sub-build forwards cross flags via [`third_party/airbot-play-rust/build.rs`](third_party/airbot-play-rust/build.rs) (set `AIRBOT_PINOCCHIO_CMAKE_TOOLCHAIN_FILE` to override).
- **Vendored patches:** workspace [`Cargo.toml`](Cargo.toml) declares `[patch.crates-io] ffmpeg-sys-next = { path = "third_party/ffmpeg-sys-next" }`. The vendored submodule is patched by [`patches/ffmpeg-sys-next-cross-target.patch`](patches/ffmpeg-sys-next-cross-target.patch) so the FFmpeg version-detection probe (`check.c`) compiles for the *target* arch (the resulting cross binary then runs transparently via `qemu-user-static` + `binfmt_misc` -- both pulled in by `make deps TARGET_ARCH=arm64`). Patch + the existing Pinocchio static patch are applied by `make apply-vendored-patches`, which runs as a dep of every Rust build/test/lint recipe. Idempotent.
- **Sharp arm64 binding:** the Makefile's `_ui-install` recipe runs `npm ci --cpu=arm64 --os=linux --libc=glibc --include=optional` for `ui/terminal` when `TARGET_ARCH=arm64`, so npm picks `@img/sharp-linux-arm64` instead of the host x64 binding. `build.sh` asserts the expected arch is in the vendored tree before packing.
- **What is *not* cross-supported yet:** `static-ffmpeg` Cargo feature on aarch64 (needs Jetson CUDA-arm64 toolchain). `aarch64-unknown-linux-musl`. Bit-for-bit reproducibility.
