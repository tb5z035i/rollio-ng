# rollio-storage-local

**Move staged episodes onto local disk as opaque per-episode subdirectories.**
The format-agnostic downstream consumer of **`EpisodeReady`** — moves
`{staging_dir}` to `{output_path}/episode_{idx:06}/` and publishes
**`EpisodeStored`**. No merging, no metadata/tb5z035i/workspace rewriting; works for any
single-file format such as MCAP.

For LeRobot v2.1, the staged episode is a tree (`data/tb5z035i/workspace/`, `videos/`, `raw/`,
`meta/*.jsonl`, `meta/info.json`) that needs per-file merging into a shared
data/tb5z035i/workspaceset root. That logic lives in [`rollio-storage-local-lerobot`](../local-lerobot/README.md) instead.

---

## iceoryx2

**Subscribe:** `control/events`; `assembler/episode-ready`.

**Publish:** `storage/episode-stored`.

No `encoder/backpressure` — backpressure is enforced upstream by the
assembler's staging-slot semaphore (`[assembler] staging_slots`).

---

## Lifecycle

**Spawned by:** `rollio collect` child **`storage`** when
`[episode] format` selects a single-file format (currently MCAP) and
`[storage] backend = "local"`.

---

## Built product & dependencies

**Binary:** `rollio-storage-local`. Filesystem permissions must allow
writes to **`output_path`**.
