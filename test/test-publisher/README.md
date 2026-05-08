# rollio-test-publisher

**Development helper** that publishes **synthetic** camera frames (`camera/{name}/frames`) and robot states (`robot/{name}/state`) over iceoryx2 — useful when exercising the visualizer, encoder, or other subscribers without real devices.

## CLI

Key flags: **`--cameras`**, **`--robots`**, **`--fps`**, **`--width`**, **`--height`**, optional **`--camera-file`** to loop a video through FFmpeg, **`--camera-device`** for live V4L2-style capture in test setups, etc. (see `src/main.rs`).

## See also

- [`rollio-bus`](../../rollio-bus/README.md) — legacy `camera/` and `robot/` topic helpers used here.
