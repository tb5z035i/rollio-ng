# Frontend–Backend Interface Specification

## Purpose

This document is the canonical reference for any browser-based frontend that
connects to the Rollio backend. It describes every HTTP and WebSocket endpoint,
message shape, and binary wire format a frontend must implement.

Related: `design/websocket-protocol.md` covers the visualizer preview path in
deeper detail (JPEG-only era). This document supersedes it where they conflict.

## Architecture Overview

```text
┌─────────────────────────────────────────────────────────────┐
│  Browser (React SPA served from ui/web/dist)                │
│                                                             │
│  fetch /api/runtime-config ──────────────────────────┐      │
│  WS    /ws/control ──────────────────────────────┐   │      │
│  WS    /ws/preview ──────────────────────────┐   │   │      │
└──────────────────────────────────────────────┼───┼───┼──────┘
                                               │   │   │
┌──────────────────────────────────────────────┼───┼───┼──────┐
│  rollio-web-gateway (Axum, default port 3000)│   │   │      │
│                                              │   │   │      │
│  Static file server (SPA fallback) ──────────┼───┼───┘      │
│  /ws/control  → proxy to control-server ─────┼───┘          │
│  /ws/preview  → proxy to visualizer ─────────┘              │
└─────────────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
┌─────────────────┐          ┌─────────────────────┐
│ control-server  │          │ visualizer           │
│ (port 9091)     │          │ (port 19090)         │
│ iceoryx2 ↔ WS  │          │ iceoryx2 ↔ WS       │
└─────────────────┘          └─────────────────────┘
```

The web-gateway is a **pure proxy** — it does not interpret WebSocket payloads.
It bridges axum↔tokio-tungstenite frames bidirectionally. The upstream services
(control-server, visualizer) each maintain their own iceoryx2 poll threads on
shared memory.

---

## 1. REST Endpoint

### `GET /api/runtime-config`

Fetched once on page load. Returns the bootstrap configuration the SPA needs
before opening WebSocket connections.

**Response** (`Content-Type: application/json`):

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

Field notes:

- `controlWebsocketUrl` / `previewWebsocketUrl` — relative paths. The browser
  constructs the full URL from `window.location` (same host/port as the SPA).
- `episodeKeyBindings` — configurable per-deployment. Defaults shown above.
  The frontend binds these to keyboard shortcuts for episode lifecycle control.

---

## 2. Control WebSocket (`/ws/control`)

All frames are **JSON text messages**. No binary frames on this path.

### 2.1 Connection Lifecycle

1. Browser opens `ws://<host>:<port>/ws/control`.
2. Server immediately sends the latest cached state snapshot (if available).
3. Server pushes state updates as they arrive from iceoryx2.
4. Browser sends commands as needed.
5. On upstream disconnect, the gateway sends a WS close frame. The browser
   should reconnect with backoff.

### 2.2 Inbound Messages (Browser → Server)

All inbound messages share a common envelope:

```json
{"type": "command", "action": "<action_name>", ...extra_fields}
```

#### Setup commands (`setup_*`)

Forwarded verbatim to the iceoryx2 `SetupCommandMessage` service. The server
does not interpret the payload beyond recognizing the `setup_` prefix.

Known actions (non-exhaustive — the setup wizard may add more):

- `setup_next_step`
- `setup_prev_step`
- `setup_toggle_identify` — extra field: `"name": "<camera_name>"`
- `setup_confirm`
- `setup_set_robot_type` — extra field: `"robot_type": "<type>"`

#### Episode commands

Mapped to the `EpisodeCommand` enum:

| Action string      | Enum variant           |
|--------------------|------------------------|
| `episode_start`    | `EpisodeCommand::Start`   |
| `episode_stop`     | `EpisodeCommand::Stop`    |
| `episode_keep`     | `EpisodeCommand::Keep`    |
| `episode_discard`  | `EpisodeCommand::Discard` |

### 2.3 Outbound Messages (Server → Browser)

#### `setup_state` (setup role)

Raw `SetupStateMessage` JSON from iceoryx2. Shape depends on the current
setup wizard step. The frontend should render based on the `step` field.

#### `episode_status` (collect role)

```json
{
  "type": "episode_status",
  "state": "idle" | "recording" | "stopped",
  "episode_count": 3,
  "elapsed_ms": 12345
}
```

- `state` — current episode state machine position.
- `episode_count` — total episodes completed in this session.
- `elapsed_ms` — wall-clock milliseconds since recording started (0 when idle).

#### `backpressure`

```json
{
  "type": "backpressure",
  "process_id": "encoder.camera_top.color",
  "queue_name": "frame_queue"
}
```

Emitted when a downstream process cannot keep up. The frontend should display
a warning indicator for the affected process.

---

## 3. Preview WebSocket (`/ws/preview`)

Mixed **binary + text** frames. Text frames are JSON; binary frames carry
camera preview data (JPEG or encoded video).

### 3.1 Connection Lifecycle

1. Browser opens `ws://<host>:<port>/ws/preview`.
2. Server sends a `stream_info` JSON text frame.
3. If preview output mode is `"encoded"`, server sends one cached
   `ENCODED_CONFIG` binary frame per camera (so WebCodecs can initialize
   decoders without waiting for the next keyframe).
4. Server streams preview frames and robot state continuously.
5. Slow clients may miss frames (lossy under backpressure).

### 3.2 Inbound Messages (Browser → Server)

```json
{"type": "command", "action": "<action>", ...}
```

| Action             | Extra fields              | Effect                          |
|--------------------|---------------------------|---------------------------------|
| `get_stream_info`  | —                         | Server re-sends `stream_info`   |
| `set_preview_size` | `"width": N, "height": N` | Resize preview encoder output   |

`set_preview_size` notes:

- Dimensions are clamped to a max of 1920 on either axis.
- Aligned to 16-pixel boundaries (aspect-ratio preserved).
- Ignored when `scaling_locked` is true for a camera (passthrough mode).
- The UI typically fires this on `ResizeObserver` ticks; redundant values
  are deduplicated server-side.

### 3.3 Outbound Text Messages (Server → Browser)

#### `stream_info`

```json
{
  "type": "stream_info",
  "server_timestamp_ms": 1715900000000,
  "preview_output_mode": "encoded" | "jpeg",
  "active_preview_width": 640,
  "active_preview_height": 480,
  "cameras": [
    {
      "name": "camera_top/color",
      "source_width": 1280,
      "source_height": 720,
      "preview_resizable": true,
      "preview_resize_policy": "fit",
      "latest_timestamp_ms": 1715900000000,
      "latest_frame_index": 54321,
      "received_fps_estimate": 29.8,
      "bytes_per_sec": 245000.0,
      "keyframe_age_ms": 1200,
      "scaling_locked": false
    }
  ],
  "robots": ["leader", "follower"]
}
```

Top-level fields:

- `preview_output_mode` — `"jpeg"` (legacy MJPEG path) or `"encoded"`
  (H.264/WebCodecs path). Determines which binary frame kinds to expect.
- `active_preview_width` / `active_preview_height` — current encoder output
  dimensions (may differ from source after `set_preview_size`).

Per-camera fields:

- `preview_resizable` — whether `set_preview_size` is honored for this camera.
- `preview_resize_policy` — `"fit"`, `"fill"`, or `"stretch"`.
- `scaling_locked` — true when the encoder output is pinned to source dims
  (passthrough backend). UI should suppress resize requests.
- `keyframe_age_ms` — milliseconds since last keyframe. Useful for showing
  decoder health.
- `bytes_per_sec` — EMA bitrate estimate for the encoded stream.

#### `robot_state`

```json
{
  "type": "robot_state",
  "name": "leader",
  "timestamp_us": 1715900000000000,
  "num_joints": 6,
  "values": [0.0, 0.1, 0.2, 0.3, 0.4, 0.5],
  "state_kind": "position",
  "value_min": [-3.14, -3.14, -3.14, -3.14, -3.14, -3.14],
  "value_max": [3.14, 3.14, 3.14, 3.14, 3.14, 3.14]
}
```

- `values` array length equals `num_joints`.
- `state_kind` — `"position"`, `"velocity"`, or `"effort"`.
- `value_min` / `value_max` — joint limits. Omitted (absent) when unknown.

### 3.4 Outbound Binary Messages (Server → Browser)

All binary frames share a common header:

```text
[0]        kind_tag     (u8)
[1..3]     name_len     (u16 LE)
[3..3+N]   name_bytes   (UTF-8 camera name)
[3+N..]    body         (kind-specific, layout below)
```

#### Kind `0x01` — JPEG_FRAME

Used when `preview_output_mode == "jpeg"`.

Body layout (all integers little-endian):

| Offset | Size | Field             | Type  |
|--------|-----:|-------------------|-------|
| 0      |    8 | timestamp_us      | u64   |
| 8      |    8 | frame_index       | u64   |
| 16     |    4 | width             | u32   |
| 20     |    4 | height            | u32   |
| 24     |  var | jpeg_bytes        | bytes |

- `timestamp_us` — source capture timestamp (microseconds since UNIX epoch).
- `frame_index` — monotonic source frame counter.
- `width` / `height` — original source dimensions (before preview resize).
- `jpeg_bytes` — the preview JPEG payload (already resized + compressed).

#### Kind `0x02` — ENCODED_CONFIG

Sent once per camera on connect and whenever the encoder session restarts
(resolution change, codec renegotiation). The browser uses this to configure
its WebCodecs `VideoDecoder`.

Body layout:

| Offset | Size | Field      | Type  |
|--------|-----:|------------|-------|
| 0      |    1 | codec_id   | u8    |
| 1      |    4 | width      | u32   |
| 5      |    4 | height     | u32   |
| 9      |    4 | avcc_len   | u32   |
| 13     |  var | avcc_bytes | bytes |

- `codec_id` — `0` = H.264. Future codecs will use higher values.
- `avcc_bytes` — the codec-specific decoder configuration record:
  - For H.264: an AVCDecoderConfigurationRecord (contains SPS/PPS).
    Pass directly to `VideoDecoder.configure()` as the `description` field.

#### Kind `0x03` — ENCODED_PACKET

Continuous encoded video access units. Feed these to the WebCodecs decoder.

Body layout:

| Offset | Size | Field               | Type  |
|--------|-----:|---------------------|-------|
| 0      |    1 | codec_id            | u8    |
| 1      |    1 | flags               | u8    |
| 2      |   8  | pts_us              | u64   |
| 10     |   8  | sequence            | u64   |
| 18     |   8  | source_timestamp_us | u64   |
| 26     |   4  | payload_len         | u32   |
| 30     |  var | payload_bytes       | bytes |

- `flags` — bit 0: keyframe. When set, the packet can be decoded independently.
- `pts_us` — presentation timestamp in microseconds (monotonic from recording
  start). Use as the `timestamp` field in `EncodedVideoChunk`.
- `sequence` — monotonic packet counter per camera. Gaps indicate dropped
  frames.
- `source_timestamp_us` — camera capture wall-clock (µs since UNIX epoch).
  Use for capture-to-display latency measurement.
- `payload_bytes` — Annex B NAL units for H.264. Pass as `data` to
  `EncodedVideoChunk`.

---

## 4. Connection Behavior & Error Handling

### Reconnection

The web-gateway closes the downstream WebSocket if the upstream service is
unavailable (e.g., visualizer not yet started during setup wizard). The browser
should:

1. Detect close/error events on both WS connections.
2. Retry with exponential backoff (recommended: 500ms → 1s → 2s → 4s cap).
3. On reconnect, expect the full handshake sequence again (snapshot + configs).

### Backpressure

- Preview path: lossy. Slow clients skip frames rather than blocking the
  producer. The visualizer uses a bounded broadcast channel internally.
- Control path: reliable within the broadcast buffer. Lagging clients may
  skip intermediate state snapshots but will always receive the latest.

### Proxy Transparency

The web-gateway does not add headers, rewrite payloads, or buffer frames.
Binary and text frame types are preserved end-to-end. Close frames propagate
in both directions.

---

## 5. Compatibility & Versioning

There is no explicit protocol version field. Breaking changes require
coordinated updates across:

- `control-server/src/protocol.rs`
- `visualizer/src/protocol.rs`
- `visualizer/src/stream_info.rs`
- `visualizer/src/websocket.rs`
- `ui/web/src/` (frontend consumer)

Preferred evolution strategy:

- Add new JSON `type` values rather than changing existing ones.
- Add new binary `kind_tag` values rather than redefining existing tags.
- Append optional fields to existing JSON messages (consumers must tolerate
  unknown fields).

### JavaScript Precision Caveat

`u64` fields (`timestamp_us`, `frame_index`, `sequence`, `source_timestamp_us`)
can exceed `Number.MAX_SAFE_INTEGER`. For coarse latency/FPS display, standard
`number` is acceptable. For exact values, use `BigInt` or `DataView` with
explicit 64-bit reads.

---

## 6. Source of Truth

| Concern | Implementation files |
|---------|---------------------|
| Runtime config struct | `rollio-types/src/config.rs` (`UiRuntimeConfig`) |
| Gateway routing + proxy | `web-gateway/src/main.rs` |
| Control protocol (encode/decode) | `control-server/src/protocol.rs` |
| Control WS server | `control-server/src/websocket.rs` |
| Control IPC bridge | `control-server/src/ipc.rs` |
| Preview binary protocol | `visualizer/src/protocol.rs` |
| Preview stream_info | `visualizer/src/stream_info.rs` |
| Preview WS server | `visualizer/src/websocket.rs` |
| Preview size logic | `visualizer/src/preview_config.rs` |

This document should stay in sync with those files. When in doubt, the Rust
source is authoritative.
