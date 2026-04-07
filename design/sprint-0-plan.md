# Sprint 0 -- Project Scaffolding and Shared Types

## Implemented State

Root Cargo workspace with 10 Rust crates, a C++ CMake skeleton, a
TypeScript/Ink UI skeleton, shared iceoryx2 message types, TOML config
schema with validation, unit tests, CI pipeline, and linting/formatting
tooling for all languages.

## Directory Layout

```
rollio-ng/
  Cargo.toml                    # workspace root (Rust crates only)
  rustfmt.toml                  # Rust formatter config
  .clang-format                 # C++ formatter config
  .clang-tidy                   # C++ linter config
  pyproject.toml                # Python linter/formatter config (ruff)
  .pre-commit-config.yaml       # pre-commit hooks for all languages
  .gitignore
  config/
    config.example.toml
  rollio-types/                 # shared lib crate (message types + config schema)
    Cargo.toml
    src/
      lib.rs
      messages.rs               # iceoryx2 shared types
      config.rs                 # TOML config schema + validation
    tests/
      messages.rs               # 16 message round-trip tests
      config.rs                 # 7 config validation tests
  controller/                   # Controller (CLI entry point) — stub
    Cargo.toml
    src/main.rs
  visualizer/                   # Visualizer (iceoryx2 <-> WebSocket bridge) — stub
    Cargo.toml
    src/main.rs
  teleop-router/                # Teleop Router — stub
    Cargo.toml
    src/main.rs
  encoder/                      # Video Encoder — stub
    Cargo.toml
    src/main.rs
  episode-assembler/            # Episode Assembler — stub
    Cargo.toml
    src/main.rs
  storage/                      # Storage backend — stub
    Cargo.toml
    src/main.rs
  monitor/                      # Health/metrics Monitor — stub
    Cargo.toml
    src/main.rs
  pseudo-robot/                 # Pseudo Robot (mock device) — stub
    Cargo.toml
    src/main.rs
  test/test-publisher/          # Test publisher utility (Sprint 1) — stub
    Cargo.toml
    src/main.rs
  cpp/                          # C++ modules
    CMakeLists.txt
    common/
      include/rollio/types.h    # C++ header matching Rust shared types
    pseudo-camera/
      CMakeLists.txt
      src/main.cpp
  ui/terminal/                  # Terminal UI (TypeScript / Ink)
    package.json
    tsconfig.json
    src/
      index.tsx
  design/                       # design docs
  third_party/                  # iceoryx2 git submodule
  .github/
    workflows/
      ci.yml                    # CI: Rust, C++, UI, Python
```

## 1. Rust Workspace

A root `Cargo.toml` workspace groups all Rust crates.  `third_party/` is
excluded so the iceoryx2 workspace does not interfere.  Each crate still
has its own `Cargo.toml` and can be built individually
(`cargo build -p rollio-types`).

### `rollio-types`

The core deliverable of Sprint 0.  A `lib` crate containing shared message
types and the TOML config schema.  All other Rust modules depend on it.

Dependencies: `iceoryx2` (path to submodule), `serde`, `toml`, `thiserror`.

**Shared iceoryx2 message types** (all `#[repr(C)]`, `#[derive(ZeroCopySend)]`,
with `#[type_name(...)]`):

- `CameraFrameHeader` -- timestamp, width, height, pixel format, frame index.
  Used as a user header with `publish_subscribe::<[u8]>()` so the raw pixel
  payload stays zero-copy.
- `RobotState` -- timestamp, `[f64; 16]` arrays for positions/velocities/efforts
  with `num_joints`, optional EE pose `[f64; 7]`.
- `RobotCommand` -- timestamp, mode (Joint/Cartesian), joint targets or
  Cartesian target.
- `ControlEvent` -- `#[repr(C)]` enum: `RecordingStart`, `RecordingStop`,
  `EpisodeKeep`, `EpisodeDiscard`, `Shutdown`, `ModeSwitch`.
- `MetricsReport` -- process ID, timestamp, up to 32 metric entries.
- `WarningEvent` -- process ID, metric name, current value, explanation.
- `VideoReady` -- process ID, episode index, file path.
- `BackpressureEvent` -- process ID, queue name.

Helper types `FixedString64` / `FixedString256` provide fixed-size byte
strings for use in `#[repr(C)]` shared-memory structs.

**TOML config schema** (serde structs with validation):

- `Config` top-level: `episode`, `devices`, `pairing`, `encoder`, `storage`,
  `monitor`.
- Validation: missing `[[devices]]` rejected; invalid values (`fps = 0`)
  rejected per-field; duplicate device names rejected; unknown codecs
  rejected; pairing references to nonexistent devices rejected.

### Stub modules

Each of the 9 binary crates (`controller`, `visualizer`, `teleop-router`,
`encoder`, `episode-assembler`, `storage`, `monitor`, `pseudo-robot`,
`test/test-publisher`) has a minimal `main.rs` that prints the crate name.

## 2. C++ Skeleton

- `cpp/CMakeLists.txt` — top-level CMake project (C++17).
- `cpp/common/include/rollio/types.h` — C++ structs matching every Rust
  shared type, with `IOX2_TYPE_NAME` constants for iceoryx2 cross-language
  compatibility.
- `cpp/pseudo-camera/` — stub binary accepting `probe|validate|capabilities|run`.

## 3. UI Skeleton (TypeScript / Ink)

- `ui/terminal/package.json` — dependencies: `ink`, `react`, `ws`, TypeScript.
- `ui/terminal/tsconfig.json` — ES2022, Node16, JSX for Ink.
- `ui/terminal/src/index.tsx` — minimal Ink app stub.
- `npm run build` compiles TypeScript to `dist/`.

## 4. Example Config

`config/config.example.toml` with 2 pseudo cameras, 2 pseudo robots
(leader/follower), a pairing entry, encoder (libx264), local storage, and
monitor thresholds.

## 5. Tests

23 unit tests in `rollio-types` (`cargo test -p rollio-types`):

- 16 message tests: round-trip byte identity for every type, payload size
  calculations, enum variant coverage, FixedString edge cases.
- 7 config tests: parse example config, reject missing devices, reject
  invalid fps, reject duplicate names, reject unknown codecs, reject bad
  pairing references, verify monitor threshold parsing.

## 6. Linting and Formatting

| Language   | Formatter        | Linter            | Config file            |
|------------|------------------|-------------------|------------------------|
| Rust       | `cargo fmt`      | `cargo clippy`    | `rustfmt.toml`         |
| C++        | `clang-format`   | `clang-tidy`      | `.clang-format`, `.clang-tidy` |
| Python     | `ruff format`    | `ruff check`      | `pyproject.toml`       |

### Pre-commit hooks

`.pre-commit-config.yaml` enforces all checks before every commit:

- Trailing whitespace, EOF fixer, YAML/TOML syntax, merge conflict markers.
- `cargo fmt --check` and `cargo clippy` for Rust.
- `clang-format` for C++ files under `cpp/`.
- `ruff check` and `ruff format` for Python.

Install with `pre-commit install` (one-time setup).

## 7. CI Pipeline

`.github/workflows/ci.yml` with four independent jobs:

| Job    | Steps                                                     |
|--------|-----------------------------------------------------------|
| Rust   | `cargo fmt --check`, `cargo build`, `cargo test`, `cargo clippy` |
| C++    | `clang-format --dry-run --Werror`, `cmake configure`, `cmake build` |
| UI     | `npm ci`, `npm run build`                                 |
| Python | `ruff check`, `ruff format --check`                       |
