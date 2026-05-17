# rollio-storage-local-lerobot

**Persists finished LeRobot v2.1 episodes to disk** by merging each staged
episode into a shared data/tb5z035i/workspaceset root. It is the LeRobot-aware downstream consumer
of **`EpisodeReady`**: verify staging tree, merge `data/tb5z035i/workspace/`, `videos/`, `raw/`,
append `meta/*.jsonl`, union `meta/info.json` features, then publish
**`EpisodeStored`**.

---

## Concepts & behaviors

### Why this crate is LeRobot-specific

A LeRobot v2.1 data/tb5z035i/workspaceset is a *shared root* across episodes: Parquet rows
accumulate, `meta/episodes.jsonl` appends, `meta/info.json` carries the union
of every feature ever recorded. None of that applies to MCAP (one self-
contained file per episode) or future HTTP uploads, so the per-format merge
logic lives here instead of in the generic
[`rollio-storage-local`](../local/README.md) mover.

### Backpressure

`rollio-storage-local-lerobot` no longer emits `encoder/backpressure`. The
assembler's staging-slot semaphore (`staging_slots` in the `[assembler]`
section) is the single source of backpressure for the local pipeline.

### Subcommands

Only **`run`** with **`StorageRuntimeConfig`** via **`--config`** or
**`--config-inline`**. Worker thread drains an unbounded queue; main thread
stays responsive to **`control/events`**.

---

## iceoryx2

**Subscribe:** `control/events`; `assembler/episode-ready`.

**Publish:** `storage/episode-stored`.

---

## Lifecycle

**Spawned by:** `rollio collect` child **`storage`** when
`[episode] format` is `lerobot-v2.1` (or `lerobot-v3.0`) and
`[storage] backend = "local"`.

**Children:** worker thread(s) only.

---

## Built product & dependencies

**Binary:** `rollio-storage-local-lerobot`. Filesystem permissions must
allow writes to **`output_path`**.

## See also

- [`rollio-storage-local`](../local/README.md) — generic per-episode mover used for non-LeRobot formats (e.g. MCAP).
- [`rollio-episode-lerobot`](../../episode-lerobot/README.md), [`rollio` controller](../../controller/README.md).
