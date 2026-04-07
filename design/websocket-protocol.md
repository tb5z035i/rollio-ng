# Visualizer-UI WebSocket Protocol

## Purpose

This document defines the WebSocket protocol used between the Rust
`visualizer` module and the TypeScript `ui` module.

It is the canonical design reference for:

- camera preview frames sent from Visualizer to UI
- robot state updates sent from Visualizer to UI
- stream metadata queries and responses
- UI commands sent back to the Visualizer

This protocol is intentionally multiplexed over a single WebSocket
connection:

- binary messages carry high-bandwidth camera preview frames
- text/JSON messages carry metadata, robot state, and commands

## Roles

- `visualizer`: WebSocket server and protocol producer
- `ui`: WebSocket client and protocol consumer

The Visualizer bridges the iceoryx2 data plane into a WebSocket stream that
the UI can consume without native iceoryx2 bindings.

## Connection Behavior

When a client connects:

1. the Visualizer accepts the WebSocket connection
2. the Visualizer immediately sends a `stream_info` JSON snapshot
3. the Visualizer continues streaming camera preview frames and robot states
4. the UI may re-query metadata at any time with a `command` message

Current metadata query command:

```json
{"type":"command","action":"get_stream_info"}
```

Current metadata response:

```json
{"type":"stream_info", "...":"..."}
```

There is no separate request ID or RPC layer yet. The current command model is
best-effort and fire-and-forget.

## Transport Semantics

- one WebSocket connection carries all message classes
- camera preview frames are lossy under backpressure
- slow clients may skip frames rather than blocking the producer
- robot state and metadata are JSON text messages
- preview frames are binary messages

The Visualizer intentionally favors low latency over guaranteed delivery for
preview frames.

## Binary Messages

### Frame Type Tags

Current binary type tags:

- `0x01`: JPEG preview frame

Only JPEG preview frames are currently implemented on the wire.

### JPEG Preview Frame Layout

All integer fields are little-endian.

| Offset | Size | Field | Type | Meaning |
| --- | ---: | --- | --- | --- |
| 0 | 1 | `frame_type` | `u8` | Must be `0x01` for JPEG preview frames |
| 1 | 2 | `camera_name_len` | `u16` | Byte length of the UTF-8 camera name |
| 3 | `N` | `camera_name` | bytes | UTF-8 camera name |
| `3 + N` | 8 | `timestamp_ns` | `u64` | Source frame timestamp from `CameraFrameHeader.timestamp_ns` |
| `11 + N` | 8 | `frame_index` | `u64` | Source frame index from `CameraFrameHeader.frame_index` |
| `19 + N` | 4 | `width` | `u32` | Original source frame width |
| `23 + N` | 4 | `height` | `u32` | Original source frame height |
| `27 + N` | remaining | `jpeg_payload` | bytes | JPEG preview payload produced by the Visualizer |

Total encoded size:

```text
1 + 2 + camera_name_len + 8 + 8 + 4 + 4 + jpeg_payload_len
```

### Binary Field Semantics

- `timestamp_ns` is the source capture timestamp, not the WebSocket send time.
- `frame_index` is the source frame counter, not a UI-local sequence number.
- `width` and `height` describe the original source frame dimensions before
  preview downsampling/compression.
- `jpeg_payload` is the preview image after Visualizer-side resizing and JPEG
  compression.

The UI should use `timestamp_ns` to estimate end-to-end preview latency.

## JSON Messages

### `robot_state`

Direction:

- Visualizer -> UI

Shape:

```json
{
  "type": "robot_state",
  "name": "robot_0",
  "timestamp_ns": 1712400000000000000,
  "num_joints": 6,
  "positions": [0.0, 0.1, 0.2],
  "velocities": [0.0, 0.0, 0.0],
  "efforts": [0.0, 0.0, 0.0]
}
```

Notes:

- arrays are serialized only up to `num_joints`
- robot state is a snapshot message, not a delta

### `stream_info`

Direction:

- Visualizer -> UI

Sent:

- immediately after client connect
- in response to `{"type":"command","action":"get_stream_info"}`

Shape:

```json
{
  "type": "stream_info",
  "server_timestamp_ns": 1712400000000000000,
  "configured_preview_fps": 60,
  "max_preview_width": 320,
  "max_preview_height": 240,
  "preview_workers": 4,
  "jpeg_quality": 30,
  "cameras": [
    {
      "name": "camera_0",
      "source_width": 640,
      "source_height": 480,
      "latest_timestamp_ns": 1712400000000000000,
      "latest_frame_index": 12345,
      "source_fps_estimate": 59.8,
      "published_fps_estimate": 19.9,
      "last_published_timestamp_ns": 1712400000005000000
    }
  ],
  "robots": ["robot_0"]
}
```

Field meanings:

- `server_timestamp_ns`: Visualizer wall-clock time when the snapshot was built
- `configured_preview_fps`: configured Visualizer-side preview throttle per
  camera; `0` means unthrottled
- `max_preview_width`: configured Visualizer preview resize limit
- `max_preview_height`: configured Visualizer preview height limit
- `preview_workers`: number of Visualizer preview worker threads
- `jpeg_quality`: configured preview JPEG quality
- `cameras`: one entry per known camera stream
- `robots`: configured robot names currently tracked by the Visualizer

Per-camera metadata:

- `source_width`, `source_height`: most recently observed source dimensions
- `latest_timestamp_ns`: most recently observed source timestamp
- `latest_frame_index`: most recently observed source frame index
- `source_fps_estimate`: EMA estimate derived from source frame index and
  source timestamp
- `published_fps_estimate`: EMA estimate of frames actually published over
  WebSocket by the Visualizer
- `last_published_timestamp_ns`: wall-clock time when the last preview frame
  for this camera was published by the Visualizer

### `command`

Direction:

- UI -> Visualizer

Shape:

```json
{
  "type": "command",
  "action": "get_stream_info"
}
```

Currently supported actions:

- `get_stream_info`

The command envelope is expected to grow as more UI-originated control actions
are implemented.

## End-to-End Timing Semantics

The protocol now supports latency measurements across the preview pipeline:

1. source module publishes a camera frame with `timestamp_ns` and
   `frame_index`
2. Visualizer keeps those source values when building the WebSocket binary
   frame
3. UI receives the binary frame and can compare local receive/display time
   against the source `timestamp_ns`

This enables the UI to report:

- receive latency: source timestamp -> WebSocket receipt
- display latency: source timestamp -> actual UI presentation

It also allows the UI to compare:

- source FPS estimate
- Visualizer published FPS estimate
- UI displayed FPS

## Backpressure and Frame Skipping

The preview path is intentionally not reliable in the "deliver every frame"
sense.

Key design choices:

- the Visualizer uses a small broadcast channel
- lagging WebSocket clients skip older preview frames
- the UI may also sample/decode only the latest available frame

This is acceptable because the preview is intended for low-latency monitoring,
not archival fidelity.

## Compatibility Notes

This protocol currently has no explicit version field.

That means any wire-format change must be treated as a coordinated change
between:

- `visualizer/src/protocol.rs`
- `visualizer/src/websocket.rs`
- `ui/terminal/src/lib/protocol.ts`
- `ui/terminal/src/lib/websocket.ts`

Breaking changes should be introduced carefully. Preferred strategies:

- add new JSON message types rather than overloading old ones
- add new binary frame tags rather than changing tag meaning
- append new fields only when all consumers are updated in lockstep

## JavaScript Precision Caveat

Several wire fields are encoded as `u64` nanosecond timestamps or frame
counters.

Examples:

- `timestamp_ns`
- `frame_index`
- `server_timestamp_ns`
- `latest_timestamp_ns`
- `last_published_timestamp_ns`

These values can exceed JavaScript's safe integer range when decoded into
plain `number`.

Current UI code decodes them into JavaScript numbers for convenience. That is
acceptable for coarse latency/FPS/debug display, but it is not lossless.

If exact nanosecond precision becomes a hard requirement, the protocol consumer
should move to one of these approaches:

- parse these fields as `bigint`
- serialize them as decimal strings in JSON
- reduce time precision at the protocol boundary when nanoseconds are not
  actually required

## Source of Truth

Implementation files for the current protocol:

- `visualizer/src/protocol.rs`
- `visualizer/src/websocket.rs`
- `visualizer/src/stream_info.rs`
- `ui/terminal/src/lib/protocol.ts`
- `ui/terminal/src/lib/websocket.ts`

This document is intended to stay in sync with those files and serve as the
human-readable design reference.
