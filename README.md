# Rollio

CLI framework for hardware discovery, setup, and data collection in robotic
teleoperation workflows.  Records camera streams and robot joint data into
structured LeRobot v2.1/v3.0 episode datasets.

See `design/` for architecture docs and sprint plans.

## Prerequisites

### Build dependencies

| Tool       | Minimum version | Purpose              |
|------------|-----------------|----------------------|
| Rust       | 1.85+           | Cargo workspace      |
| CMake      | 3.16+           | C++ modules          |
| g++ / clang| C++17 support   | C++ modules          |
| Node.js    | 18+             | UI (TypeScript/Ink)  |
| npm        | 9+              | UI dependency mgmt   |
| Python     | 3.10+           | Robot drivers        |

### Optional (development)

| Tool          | Purpose                          |
|---------------|----------------------------------|
| clang-format  | C++ auto-formatting              |
| clang-tidy    | C++ static analysis              |
| ruff          | Python linting & formatting      |
| pre-commit    | Git hook runner                  |

## Getting started

```bash
# Clone with submodules
git clone --recursive <repo-url>
cd rollio-ng

# Rust
cargo build --workspace
cargo test --workspace

# C++
cmake -B cpp/build -S cpp
cmake --build cpp/build

# UI
cd ui && npm install && npm run build && cd ..
```

## Pre-commit hooks (optional)

```bash
pip install pre-commit
pre-commit install
```

This enables automatic checks before each commit:
Rust formatting/linting, C++ formatting, Python linting/formatting,
and general file hygiene (trailing whitespace, TOML/YAML syntax, etc.).

## Project layout

```
rollio-types/         Shared iceoryx2 message types + config schema (Rust lib)
controller/           CLI entry point and process orchestrator (Rust)
visualizer/           iceoryx2 <-> WebSocket bridge (Rust)
teleop-router/        Leader-follower command forwarding (Rust)
encoder/              Video encoding per camera stream (Rust)
episode-assembler/    Assembles episodes from video + state data (Rust)
storage/              Local and remote storage backends (Rust)
monitor/              Health/performance metrics evaluator (Rust)
pseudo-robot/         Mock robot driver for testing (Rust)
test-publisher/       Synthetic iceoryx2 data publisher (Rust)
cpp/                  C++ camera drivers (pseudo, RealSense, V4L2)
ui/                   Terminal UI built with React/Ink (TypeScript)
config/               Example configuration files
design/               Architecture docs and sprint plans
third_party/          iceoryx2 git submodule
```
