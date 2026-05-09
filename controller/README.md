# rollio (controller)

Cargo package **`rollio`**, binary **`rollio`**. This is the **orchestrator**: it does not stream camera pixels or joint torques itself. It parses your project config, discovers hardware, runs an interactive **setup** wizard, or launches a full **collect** session with many child processes that talk to each other over **iceoryx2**.

---

## Concepts & behaviors

### What “setup” vs “collect” mean

- **`rollio setup`** — **Configuration time.** You discover what hardware exists, pair cameras to arms, name channels, and write a `ProjectConfig` (often `config.toml`). It can spawn a **preview stack** (real devices + encoders + visualizer) so you see live video and move robots while configuring. Nothing is written to your dataset output tree yet in the sense of finalized episodes; you are shaping *how* later runs will behave.

- **`rollio collect`** — **Runtime.** You already have a saved config. Rollio spawns the full pipeline: devices, encoders (one per configured camera stream), assembler, storage, UI bridges, optional teleop routers, etc. The controller owns the **episode lifecycle** (start recording, stop, keep vs discard) by translating UI commands into **`ControlEvent`** messages every subscriber understands.

If you are new to the stack: think of **setup** as “install + calibrate the graph,” and **collect** as “run that graph and record data.”

### Why the controller almost never touches camera/robot payload topics

Devices publish frames and joint states under each logical **`bus_root`** (see [`robots/README.md`](../robots/README.md)). The controller listens on the **global control plane** (`control/*`, `setup/*`, storage/assembler handshakes) instead. That keeps orchestration logic small and lets drivers stay replaceable.

### Subcommands

#### `rollio setup`

- **Purpose:** Walk through discovery, validation, optional teleop pairing, and emit a config file (`--output`, default `config.toml`).
- **Inputs:** Optional starting config (`--config` / `--config-inline`), optional **`--accept-defaults`** for non-interactive flows.
- **`--sim-pseudo N`:** Injects **synthetic** device IDs so the wizard can be exercised without hardware; those map to [`rollio-device-pseudo`](../robots/pseudo/README.md) (not auto-discovered otherwise).
- **Side effect:** May run **`cargo build`** for a fixed list of dev packages on first use from a workspace `target/` layout — see source if your environment forbids that.

#### `rollio collect`

- **Purpose:** Run a full capture session from a **complete** `ProjectConfig`.
- **Requires:** `--config` or `--config-inline`.
- **Before children start:** Refreshes per-device **`value_limits`** by calling each driver’s **`query --json`** so the UI can clamp overlays safely.
- **Episode loop (conceptual):** UI sends **`EpisodeCommand`** → control-server → controller subscribes → controller publishes **`ControlEvent`** (recording boundaries, shutdown) on `control/events` and **`EpisodeStatus`** for progress. Encoders, assembler, and storage react to those events in parallel.

#### Device discovery (not `rollio` subcommands)

There is no `rollio device` command. Instead the controller **execs** each `rollio-device-*` binary with **`probe`**, **`validate`**, **`query`**, or **`run`** as documented in that driver’s README. Same contract for Rust and Python drivers.

---

## iceoryx2

**`rollio collect`** creates its IPC node **before** spawning children so it can set generous `max_nodes` / caps on shared services.

| Direction | Service | Role |
|-----------|---------|------|
| Subscribe | `control/episode-command` | Commands from UI via [`rollio-control-server`](../control-server/README.md). |
| Publish | `control/events` | `ControlEvent` fan-out (shutdown, recording, …). |
| Publish | `control/episode-status` | Progress for UI. |
| Subscribe | `encoder/backpressure` | May block starting a new episode if the pipeline is saturated. |
| Subscribe | `storage/episode-stored` | Advances episode bookkeeping after disk commit. |

**`rollio setup`** (wizard path in [`setup.rs`](src/setup.rs)):

| Direction | Service | Role |
|-----------|---------|------|
| Subscribe | `setup/command` | Wizard/UI → controller. |
| Publish | `setup/state` | Controller → UI snapshots. |
| Publish | `control/events` | e.g. `Shutdown` when swapping preview compositions. |
| Publish | `{bus_root}/{channel_type}/control/mode` | `DeviceChannelMode` when the wizard changes how a **channel** behaves (per channel — independent). |

---

## Lifecycle

**Launched by:** the operator only (not spawned by another Rollio binary).

**Children (typical):**

| Mode | Processes |
|------|-----------|
| **`collect`** | `rollio-visualizer`, each `rollio-device-* run`, each `rollio-encoder run`, optional `rollio-teleop-router run`, `rollio-control-server` (collect role), `rollio-episode-lerobot`, `rollio-storage-local`, `rollio-web-gateway`, terminal UI (`node` + Ink bundle). |
| **`setup`** | Overlapping preview stack + `rollio-control-server` (setup role) + wizard UI; optional `cargo build` for dev binaries — see [`setup.rs`](src/setup.rs). |

Children use the Rollio state directory as cwd; paths in config that look relative are resolved from **your** invocation directory where possible.

---

## Built product & dependencies

- **Artifact:** `rollio` executable.
- **Runtime:** children resolved via [`runtime_paths.rs`](src/runtime_paths.rs) (`PATH` or beside `rollio`).
- **`rollio setup`:** expects a modern **Node.js** for the terminal UI ([`ensure_node_available`](src/setup.rs)).
- **APT / system:** [`AGENTS.md`](../AGENTS.md) (`make deps`) covers the workspace toolchain.

## See also

- [`design/components.md`](../design/components.md), [`AGENTS.md`](../AGENTS.md).
