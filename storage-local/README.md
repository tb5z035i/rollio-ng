# rollio-storage-local

**Writes finished episodes to disk** in the operator’s dataset directory. It is the downstream consumer of **`EpisodeReady`**: verify staging tree, commit files, merge metadata, tell the controller **`EpisodeStored`** (or exert **backpressure** if it cannot keep up).

---

## Concepts & behaviors

### Why separation from assembler?

Staging can involve **large copies** across filesystems. Moving that work behind a queue:

- Keeps assembler CPU focused on deterministic Parquet writes.
- Lets storage apply **different backends** later (today: local paths in `StorageRuntimeConfig`).

### Backpressure knob

If the internal FIFO of pending episodes fills, storage publishes **`encoder/backpressure`** so **`rollio`** can **delay starting another episode** until the disk side catches up. New colleagues: this is why episode start buttons occasionally “feel slow” — it is deliberate flow control.

### Subcommands

Only **`run`** with **`StorageRuntimeConfig`** via **`--config`** or **`--config-inline`** (paths can be rewritten relative to the user’s cwd by the controller for intuitive `./output`-style configs).

Worker thread drains the queue; main thread stays responsive to **`control/events`**.

---

## iceoryx2

**Subscribe:** `control/events`; `assembler/episode-ready`.

**Publish:** `storage/episode-stored`; `encoder/backpressure` (when saturated).

---

## Lifecycle

**Spawned by:** `rollio collect` child **`storage`**.

**Children:** worker thread(s) only.

---

## Built product & dependencies

**Binary:** `rollio-storage-local`. Filesystem permissions must allow writes to **`output_path`**.

## See also

- [`rollio-episode-lerobot`](../episode-lerobot/README.md), [`rollio` controller](../controller/README.md).
