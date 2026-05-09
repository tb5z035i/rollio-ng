# rollio-episode-lerobot

**Episode assembler (“staging”)**: during **`rollio collect`**, this process turns **raw encoded media files + time-aligned robot samples** captured over iceoryx2 into the **LeRobot-ish dataset layout** your `ProjectConfig` describes, then hands off a **`EpisodeReady`** envelope to storage.

---

## Concepts & behaviors

### Where it sits in the pipeline

1. Devices stream **sensor** data; encoders write **video/depth** artifacts when recording.
2. **`rollio` (controller)** publishes **`ControlEvent`** boundaries (start/stop/keep/discard).
3. Encoders signal **`VideoReady`** when their files for an episode are flushed and named predictably.
4. **This assembler** subscribes those signals, drains the observation/action ICE topics referenced in **`AssemblerRuntimeConfigV2`**, builds Parquet / sidecars, stages them under a temp directory tree.
5. It publishes **`EpisodeReady`** so **`rollio-storage-local`** can atomically bless the episode into your dataset folder.

### Multi-channel robots (mental model)

A single physical robot may contribute **many** ICE streams (arm joints, gripper parallel position, …). Assembler config explicitly lists **which** `{bus_root}/{channel}/states/{kind}` and **`.../commands/{kind}`** rows to log. Missing config → data never hits the dataset even if it is on the bus.

**Channel modes** (free-drive vs following) are **not** inferred here; the assembler **records** what appears on the configured topics. Policy belongs to operators + drivers.

### Subcommands

Only **`run`**:

- **`--config`** / **`--config-inline`** — accept **`AssemblerRuntimeConfigV2`** (the controller embeds the full project TOML inside that struct for traceability).

Heavy CPU / disk work runs on a **worker thread** so iceoryx2 subscribers can still drain high-rate rings.

---

## iceoryx2

**Subscribe:** `control/events`; `encoder/video-ready`; each configured observation `.../states/...`; each configured action `.../commands/...`.

**Publish:** `assembler/episode-ready`.

---

## Lifecycle

**Spawned by:** `rollio collect` as child **`assembler`**.

**Children:** internal worker only.

---

## Built product & dependencies

**Binary:** `rollio-episode-lerobot`. Rust + **Parquet/Arrow** + iceoryx2.

**APT / system:** standard workspace toolchain; no extra daemons.

## See also

- [`rollio-storage-local`](../storage-local/README.md), [`rollio-encoder`](../encoder/README.md).
