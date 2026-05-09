# rollio-teleop-router

**Mirrors motion between two robot channels** in **teleop** collection mode: read the **leader’s** high-rate **state** stream, optionally compare against the **follower’s** state stream for safe startup, transform according to **`MappingStrategy`**, and write **commands** onto the **follower’s command** topic.

---

## Concepts & behaviors

### One process per teleop **pair**

`ProjectConfig` can define multiple leader/follower pairs; each gets its **own** `rollio-teleop-router` child so a bug or restart in one pair does not block another.

### Modes still live on the devices

The router **never** replaces per-channel **mode** IPC. For teleop to move hardware:

- Each device channel must be in **`command-following`** (or whatever that driver requires) on **`.../control/mode`**.
- The router only **publishes command samples**; the follower driver decides whether to apply them.

### Mapping strategies (high level)

Examples (see `TeleopRuntimeConfigV2` in [`rollio-types`](../rollio-types/README.md)):

- **Direct joint** — map leader joint indices → follower joints (possibly with scaling).
- **Cartesian policies** — require **pose-shaped** leader/follower state streams; FK/IK remain inside **drivers**, not here.

### Internal run phases (not iceoryx2 topics)

Documented in [`src/lib.rs`](src/lib.rs):

1. **Initial syncing** — limits per-tick delta so the follower **eases toward** the leader (safety ramp at teleop engagement).
2. **Pass-through** — after convergence criteria pass, forwards leader targets **without** clamping (allows fast operator motions).

### Subcommands

Only **`run`**:

- **`--config`** / **`--config-inline`** — **one** `TeleopRuntimeConfigV2` stanza for this pair (controller embeds multiple children with different inline TOML).

---

## iceoryx2

**Subscribe:** `control/events`; leader (and optional follower) **`.../states/{kind}`** topics.

**Publish:** follower **`.../commands/{kind}`** topic matching configured command type.

---

## Lifecycle

**Spawned by:** `rollio collect` when `CollectionMode::Teleop`.

**Children:** none.

---

## Built product & dependencies

**Binary:** `rollio-teleop-router`; workspace Rust + iceoryx2 only.

## See also

- [`robots/README.md`](../robots/README.md) (channel independence), device READMEs for command acceptance.
