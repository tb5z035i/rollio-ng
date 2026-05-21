# UI Protocol: WebSocket + HTTP

## Purpose

This document defines the network surface the Rollio web UI talks to. It is
the canonical design reference for:

- camera preview frames (JPEG or H.264) sent from visualizer to UI
- robot state updates sent from visualizer to UI
- stream metadata queries and responses
- per-camera preview-size commands sent UI → visualizer
- episode lifecycle commands and status (UI ↔ control-server)
- setup-wizard state and commands (UI ↔ control-server)
- the one HTTP endpoint the UI hits at startup to discover everything else

There are **two long-lived WebSocket connections** plus one HTTP endpoint.
A static-asset web-gateway proxy in front of both terminates the browser
side and forwards to the Rust servers, so from the browser's perspective
everything lives on the same origin.

## Roles and topology

| Endpoint | Producer | Role |
| --- | --- | --- |
| `GET /api/runtime-config` | `web-gateway` | Returns the browser's runtime config (the two WS URLs and the episode key bindings). Polled once on UI mount. |
| `WS /ws/preview` | `web-gateway` → proxies to `visualizer` | High-bandwidth camera preview frames (binary) + per-stream JSON metadata (`stream_info`, `robot_state`). UI → visualizer commands ride the same socket. |
| `WS /ws/control` | `web-gateway` → proxies to `control-server` (Collect role) | JSON-only control plane: `episode_status` and `backpressure` from the server, `episode_*` commands from the UI. In Setup role, the same socket forwards `setup_state` from the server and `setup_*` commands from the UI. |

The visualizer bridges the iceoryx2 data plane to the preview socket so
the UI does not need native iceoryx2 bindings. The control-server bridges
the iceoryx2 control plane (`episode_command`, `episode_status`,
`setup_command`, `setup_state`, `backpressure`) on a separate socket so
the two planes scale and fail independently.

## HTTP: `/api/runtime-config`

Direction: UI → web-gateway, polled once at app mount.

Response: `200 OK`, `Content-Type: application/json`:

```json
{
  "controlWebsocketUrl": "/ws/control",
  "previewWebsocketUrl": "/ws/preview",
  "episodeKeyBindings": {
    "startKey": "s",
    "stopKey": "e",
    "keepKey": "k",
    "discardKey": "x"
  }
}
```

Each URL is either a relative path (resolved against `window.location` —
default in production) or a fully-qualified `ws://` / `wss://` URL
(useful in dev when the UI is served from a different origin than the
Rust servers). The UI's `resolveWebSocketUrl` normalizes both forms.

`episodeKeyBindings` are single-character lowercase keys; the UI's
`actionForInput` maps them to the corresponding `episode_*` command.

## Connection behavior

When the UI mounts:

1. `GET /api/runtime-config` to learn both WS URLs.
2. Opens `WS /ws/preview` and `WS /ws/control` in parallel.
3. The preview socket immediately receives a `stream_info` JSON snapshot.
4. The control socket starts receiving the latest retained snapshot
   (`episode_status` for Collect role, `setup_state` for Setup role).
5. Both sockets stay open for the lifetime of the page. Either may be
   reconnected by the UI's `useReconnectingSocket` hook without
   disturbing the other.

Both sockets are best-effort: lagging clients skip messages rather than
blocking the producer. There is no request ID or RPC layer; commands are
fire-and-forget.

# Preview socket: `/ws/preview`

## Transport semantics

- One WebSocket multiplexes high-bandwidth binary frames (kind tag at
  byte 0) and low-bandwidth JSON text messages (parsed by `type`).
- Preview frames are lossy under backpressure — the visualizer uses a
  small broadcast channel and lagging clients skip frames.
- `robot_state` and `stream_info` are reliable JSON snapshots.
- All numeric times that fit in an `f64` are surfaced as JavaScript
  numbers; see the [JavaScript precision caveat](#javascript-precision-caveat).

## Binary message header

All binary frames share a common 3 + N-byte header. Integer fields are
little-endian throughout.

| Offset | Size | Field | Meaning |
| --- | ---: | --- | --- |
| 0 | 1 | `kind` (`u8`) | Frame-type tag (see below) |
| 1 | 2 | `name_len` (`u16`) | Byte length of the UTF-8 camera name |
| 3 | `N` | `name` (bytes) | UTF-8 camera name |
| `3+N..` | — | body | Kind-specific payload |

Current kind tags:

| Tag | Name | Emitted when |
| --- | --- | --- |
| `0x01` | `JPEG_FRAME` | `stream_info.preview_output_mode == "jpeg"` |
| `0x03` | `ENCODED_PACKET` | `stream_info.preview_output_mode == "encoded"` |

Tag `0x02` is **reserved/retired** — earlier protocol revisions used
it for a separate codec-config message. The current protocol embeds
codec config inline (see below) so `0x02` is never emitted; receivers
should treat it as an unknown tag.

### `0x01` JPEG preview frame

Body layout (offsets relative to `3+N`):

| Offset | Size | Field | Meaning |
| --- | ---: | --- | --- |
| 0 | 8 | `timestamp_us` (`u64`) | Source capture timestamp, μs since UNIX epoch |
| 8 | 8 | `frame_index` (`u64`) | Source frame counter |
| 16 | 4 | `width` (`u32`) | Source frame width (pre-resize) |
| 20 | 4 | `height` (`u32`) | Source frame height |
| 24 | — | `jpeg_payload` (bytes) | JPEG bytes after visualizer-side resize/compression |

### `0x03` Encoded preview packet

One self-contained access unit from the encoder runtime. Body layout
(offsets relative to `3+N`):

| Offset | Size | Field | Meaning |
| --- | ---: | --- | --- |
| 0 | 1 | `codec_id` (`u8`) | Matches `EncodedCodecId` (0 = H.264, 1 = H.265, 2 = AV1, 3 = RVL, 4 = MJPG) |
| 1 | 1 | `flags` (`u8`) | Bit 0 = keyframe; other bits reserved |
| 2 | 8 | `pts_us` (`u64`) | Codec PTS in μs, monotonic from recording start |
| 10 | 8 | `sequence` (`u64`) | Per-stream packet sequence number, monotonic from session start |
| 18 | 8 | `source_timestamp_us` (`u64`) | Camera capture wall-clock μs since UNIX epoch — propagated from `EncodedPacketHeader.source_timestamp_us` |
| 26 | 4 | `width` (`u32`) | Coded width (lets the UI configure WebCodecs on the first keyframe alone) |
| 30 | 4 | `height` (`u32`) | Coded height |
| 34 | 4 | `payload_len` (`u32`) | Length of the payload that follows |
| 38 | `payload_len` | `payload` (bytes) | Codec-specific access unit (see below) |

**Self-contained-AU contract.** The payload is exactly one access unit:

- H.264 / H.265 — **Annex B framing**. Keyframes (`flags & 1`) MUST
  carry SPS+PPS (H.264) / VPS+SPS+PPS (H.265) inline ahead of the VCL
  NALUs. Delta packets MUST NOT carry parameter sets. The encoder runs
  without `AV_CODEC_FLAG_GLOBAL_HEADER` so libx264 / NVENC / VAAPI
  emit parameter sets inline naturally.
- AV1 — one temporal unit, with the sequence-header OBU inline on
  keyframes.
- MJPG — one self-contained JPEG.
- RVL — one RVL frame; every packet is a keyframe by construction.

Because every keyframe is self-contained, the UI:

- Configures its `VideoDecoder` (in Annex B mode — no `description`)
  on the first keyframe seen per camera, parsing SPS from the payload
  to build the canonical `avc1.PPCCLL` codec string.
- Drops every packet for that camera until that first keyframe arrives
  (`seen_keyframe` gate). This same gate runs server-side in the
  visualizer for defense in depth.
- Re-configures the decoder when the `(codec_id, width, height)` tuple
  changes (e.g. after a `set_preview_size` round-trip restarts the
  preview encoder session at new dims).

No prior out-of-band `description` / config message is needed or sent.

### Binary field semantics

- `timestamp_us` / `source_timestamp_us` are **source capture** times,
  not WS send times. Use them for end-to-end latency vs `Date.now()`.
- `pts_us` is the codec timeline (zero at recording start). Use it for
  WebCodecs ordering, not for wall-clock latency.
- `sequence` resets on each fresh encoder session. A gap is a hard
  error in the recording assembler but tolerable in the preview path
  (clients recover at the next keyframe).
- `width` / `height` on `0x03` are the **coded** dims — what
  WebCodecs needs to `configure()`. They may differ from
  `stream_info.cameras[*].source_width/height` whenever the preview
  encoder downscales.

## JSON messages

All JSON messages share a `type` discriminator. The preview socket
exchanges the following:

### `stream_info` (visualizer → UI)

Sent immediately after WS connect and in response to
`{"type":"command","action":"get_stream_info"}`. The UI also receives a
fresh snapshot whenever it sends `set_preview_size`.

```json
{
  "type": "stream_info",
  "server_timestamp_ms": 1712400000000,
  "preview_output_mode": "encoded",
  "active_preview_width": 832,
  "active_preview_height": 512,
  "cameras": [
    {
      "name": "camera/color",
      "source_width": 1920,
      "source_height": 1080,
      "preview_resizable": true,
      "preview_resize_policy": "dynamic",
      "latest_timestamp_ms": 1712400000005,
      "latest_frame_index": 12345,
      "received_fps_estimate": 29.97,
      "bytes_per_sec": 1200000.0,
      "keyframe_age_ms": 33,
      "scaling_locked": false
    }
  ],
  "robots": ["robot_0"]
}
```

Fields:

- `server_timestamp_ms` — visualizer wall-clock when the snapshot was
  built, ms since UNIX epoch.
- `preview_output_mode` — `"jpeg"` selects the `0x01` binary path,
  `"encoded"` selects `0x03`. The UI uses this to pick a decoder.
- `active_preview_width` / `active_preview_height` — current
  visualizer-side preview output dims after any `set_preview_size`
  clamp. Match `width` / `height` on the most recent `0x03` packet
  for cameras whose preview path is not `scaling_locked`.

Per-camera fields:

- `name` — the channel id (e.g. `"camera/color"`). Same string used as
  `name` in every binary frame for that camera.
- `source_width` / `source_height` — most recently observed source
  dims; `null` until the first packet arrives.
- `preview_resizable` — `true` when the active backend can scale to a
  UI-chosen size. `false` for passthrough channels where the encoder's
  output is pinned to source dims; the UI must hide its size slider
  and must not send `set_preview_size`.
- `preview_resize_policy` — `"dynamic"` (resizable) or
  `"fixed-source"` (pinned, equivalent to `scaling_locked = true`).
- `latest_timestamp_ms` / `latest_frame_index` — most recently
  observed source frame index / capture time; `null` until the first
  packet arrives.
- `received_fps_estimate` — EMA of inter-packet arrival rate at the
  visualizer (post `seen_keyframe` gate; pre-WS broadcast).
- `bytes_per_sec` — EMA of preview payload bytes per second over the
  visualizer → WS edge.
- `keyframe_age_ms` — ms since the most recent keyframe was observed
  for this stream, in encoded mode. `null` in JPEG mode and before the
  first keyframe.
- `scaling_locked` — surfaced from the `0x03` packet flag bit. When
  `true`, `set_preview_size` will be rejected upstream by the encoder
  (the UI must suppress it).

### `robot_state` (visualizer → UI)

A snapshot for one robot, one `state_kind`. The UI aggregates messages
sharing the same `name` into one channel block keyed by `state_kind`.

```json
{
  "type": "robot_state",
  "name": "robot_0",
  "timestamp_us": 1712400000000000,
  "num_joints": 6,
  "values": [0.0, 0.1, 0.2, 0.3, 0.4, 0.5],
  "state_kind": "joint_position",
  "value_min": [-3.14, -3.14, -3.14, -3.14, -3.14, -3.14],
  "value_max": [3.14, 3.14, 3.14, 3.14, 3.14, 3.14]
}
```

Fields:

- `timestamp_us` — wall-clock μs since UNIX epoch when the values
  were sampled by the device driver.
- `num_joints` — element count of `values`; `values.length` is
  authoritative if they disagree.
- `values` — one array per message. Unit depends on `state_kind`
  (rad / rad·s⁻¹ / N·m for joint kinds, m / m·s⁻¹ / N for parallel
  kinds, mixed for poses).
- `state_kind` — discriminator that names the semantic kind. Current
  kinds are produced by the robot driver layer and forwarded verbatim
  (e.g. `joint_position`, `joint_velocity`, `joint_effort`,
  `parallel_position`, `pose`).
- `value_min` / `value_max` — optional per-element envelope reported
  by the device driver. Omitted (or empty) when the driver does not
  publish limits.
- `end_effector_status` / `end_effector_feedback_valid` — optional
  end-effector fields. Present only for messages from drivers that
  populate them.

`robot_state` is a snapshot, not a delta — every message carries a
complete `values` vector.

### `command` (UI → visualizer)

```json
{"type": "command", "action": "get_stream_info"}
```

Supported actions:

| `action` | Extra fields | Effect |
| --- | --- | --- |
| `get_stream_info` | — | Server replies with a fresh `stream_info`. |
| `set_preview_size` | `width: u32`, `height: u32` | Requests that the preview encoder restart at the new dims. The visualizer clamps to its configured min/max + alignment rules and forwards only when the post-clamp dims actually changed (no-op resizes do not tear down the codec session). The server then sends an updated `stream_info` whether it forwarded upstream or not. Rejected silently when the target camera has `scaling_locked = true`. |

Unknown actions are ignored. Commands without their required fields
(e.g. `set_preview_size` without a `width`) are logged at WARN and
dropped.

# Control socket: `/ws/control`

This socket is owned by the `control-server` process, not the
visualizer. It carries only JSON. The web-gateway proxies it onto
`/ws/control` for the browser.

The control-server has two roles, selected at startup:

- **Collect** — forwards `episode_command` (UI → iceoryx2) and
  `episode_status` + `backpressure` (iceoryx2 → UI). This is what the
  browser talks to during a recording session.
- **Setup** — forwards `setup_command` (UI → iceoryx2) and
  `setup_state` (iceoryx2 → UI). Used during the setup wizard.

Both roles use the same JSON envelopes and the same command shape.

## JSON messages

### `episode_status` (server → UI, Collect role)

Latest retained snapshot is replayed to every fresh client; updates
push whenever the underlying iceoryx2 `EpisodeStatus` changes.

```json
{
  "type": "episode_status",
  "state": "recording",
  "episode_count": 3,
  "elapsed_ms": 1234
}
```

- `state` — one of `"idle"`, `"recording"`, `"pending"`. The UI maps
  `"pending"` to the keep/discard prompt.
- `episode_count` — episodes recorded in this session (across all
  cameras).
- `elapsed_ms` — ms since the current `Recording` or `Pending` state
  was entered. `0` while `idle`.

### `backpressure` (server → UI, Collect role)

Emitted whenever a producer marks an iceoryx2 queue as backpressured
(typically an encoder running behind its budget).

```json
{
  "type": "backpressure",
  "process_id": "encoder.camera_top.color",
  "queue_name": "frame_queue"
}
```

There is no recovery message: clients should clear their UI hint after
a short timeout if no further `backpressure` arrives.

### `setup_state` (server → UI, Setup role)

Opaque JSON forwarded verbatim from iceoryx2 `SetupStateMessage`. The
control-server does not parse or rewrite the payload; the shape is
owned by the setup wizard module. The UI matches on `obj.type` to
route the message.

### `command` (UI → server, both roles)

```json
{"type": "command", "action": "episode_start"}
```

Supported actions:

| Role | `action` | Effect |
| --- | --- | --- |
| Collect | `episode_start` | Sends `EpisodeCommand::Start` on iceoryx2. |
| Collect | `episode_stop` | Sends `EpisodeCommand::Stop`. |
| Collect | `episode_keep` | Sends `EpisodeCommand::Keep` (resolves a `Pending` state). |
| Collect | `episode_discard` | Sends `EpisodeCommand::Discard`. |
| Setup | `setup_*` | Forwarded verbatim onto the iceoryx2 `SetupCommandMessage` service (the action string is opaque to the control-server — it just relays the full JSON). |

Unknown actions for the active role are ignored. The control-server
does not currently surface error responses; the UI infers success from
the next `episode_status` / `setup_state` update.

# End-to-end timing semantics

The protocol supports latency measurement across the preview pipeline:

1. Source module publishes a frame with `timestamp_us` / `frame_index`.
2. Encoder propagates `source_timestamp_us` onto every encoded packet.
3. Visualizer keeps those source values when building the binary frame.
4. UI compares local receive time and decoded-frame display time
   against `source_timestamp_us` (or `timestamp_us` in JPEG mode).

This lets the UI compute:

- Receive latency: `source_timestamp_us` → WS receipt time.
- Display latency: `source_timestamp_us` → WebCodecs / canvas commit
  time. Surfaced as `ui.video_decode_latency_ms.{camera_name}` and
  `ui.display_latency_ms.{camera_name}` gauges.

It also lets the UI compare source-FPS estimate (from
`stream_info.cameras[*].received_fps_estimate`) against the actual
UI-displayed FPS counted at the canvas.

# Backpressure and frame skipping

The preview path is deliberately not reliable.

- The visualizer uses a small broadcast channel; lagging WS clients
  skip older preview frames.
- The UI's `usePreviewSocket` may sample only the most recent frame
  per camera between paint ticks.
- In encoded mode, a dropped delta packet causes the WebCodecs decoder
  to emit decode errors for everything downstream until the next
  keyframe. The UI counts these via the
  `ui.preview_decoder_decode_failures_total.{camera_name}` gauge.
- New encoder sessions emit a forced IDR within ~one frame of
  `open_session` (the recording / preview runtimes call
  `CodecSession::request_keyframe()`), so re-sync after a drop or a
  fresh subscriber is bounded by one frame period rather than one GOP.

This trade-off is acceptable because previews are for low-latency
operator monitoring, not archival fidelity.

# Compatibility notes

This protocol has no explicit version field. Wire-format changes are
coordinated lockstep between the producer modules and the consumer
modules:

- `visualizer/src/protocol.rs` + `visualizer/src/websocket.rs` +
  `visualizer/src/stream_info.rs` (preview socket)
- `control-server/src/protocol.rs` + `control-server/src/websocket.rs`
  + `control-server/src/ipc.rs` (control socket)
- `web-gateway/src/main.rs` (`/api/runtime-config` schema)
- `ui/web/src/lib/protocol.ts` + `ui/web/src/lib/websocket.ts` +
  `ui/web/src/lib/runtime-config.ts` (browser side)

Preferred extension strategies:

- Add new JSON `type` discriminators rather than overloading existing
  ones.
- Add new binary frame tags rather than changing a tag's meaning. The
  `0x02` tag is retired; do not reuse it without a coordinated bump
  across every consumer.
- Append fields at the end of a binary body only when every consumer
  is updated to read them in lockstep — the body parsers use fixed
  offsets, not length-prefixed records.

# JavaScript precision caveat

Several wire fields are encoded as 64-bit microsecond timestamps or
sequence counters:

- `timestamp_us` / `source_timestamp_us`
- `pts_us`
- `sequence`
- `frame_index`
- `latest_timestamp_ms`
- `server_timestamp_ms`

For wall-clock μs since UNIX epoch these stay within `Number.MAX_SAFE_INTEGER`
for the next ~280 years, so the current code parses them through
`Number(bigUint64)`. That is acceptable for coarse latency / FPS /
debug display.

Sequence counters can in principle exceed safe-integer range over very
long-running sessions (`u64`, decades at MHz rates). The current UI
treats sequence as opaque (it is used only for equality / increment
checks within a short window), so the precision loss does not change
behavior. If exact integer fidelity becomes a hard requirement, the
parser should switch the affected fields to `bigint` or the protocol
should serialize them as decimal strings.

# Source of truth

Implementation files for the current protocol:

- `visualizer/src/protocol.rs`
- `visualizer/src/websocket.rs`
- `visualizer/src/stream_info.rs`
- `control-server/src/protocol.rs`
- `control-server/src/websocket.rs`
- `control-server/src/ipc.rs`
- `control-server/src/lib.rs`
- `web-gateway/src/main.rs`
- `ui/web/src/lib/protocol.ts`
- `ui/web/src/lib/websocket.ts`
- `ui/web/src/lib/runtime-config.ts`

This document is intended to stay in sync with those files and serve
as the human-readable design reference.
