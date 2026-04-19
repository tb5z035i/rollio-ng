# Cameras

In-repo camera drivers live under `cameras/`. Cameras are just *device drivers*
that happen to expose `kind: camera` channels in their `query --json` response;
the framework no longer special-cases them vs robot drivers.

## Layout

- `pseudo/`: synthetic reference camera (`rollio-device-pseudo-camera`) used
  by C++ integration tests.
- `realsense/`: hardware-backed RealSense driver (`rollio-device-realsense`)
  with no-hardware fallback coverage.
- `v4l2/`: Rust V4L2 webcam driver (`rollio-device-v4l2`) that converts native
  frames to `rgb24`/`bgr24`.
- `build/`: generated CMake build directory.

## Add A New Camera Driver

Same convention as any other device driver — see
[`robots/README.md`](../robots/README.md) for the full contract. Briefly:

1. Create a folder under `cameras/<driver_name>/` (or `robots/`, doesn't
   matter to the framework).
2. Expose a binary named `rollio-device-<driver_name>`.
3. Implement the device CLI contract (`probe`, `validate`, `query`, `run`).
4. Publish frames to `{bus_root}/{channel_type}/frames`.
5. Listen for `control/events` and exit cleanly on shutdown.
6. Add `add_subdirectory(<driver_name>)` to `cameras/CMakeLists.txt` if it
   is a C++ driver.

## V4L2 Webcam Driver

The in-repo `v4l2` driver is a Rust binary exposed as `rollio-device-v4l2`.

- Use `driver = "v4l2"` and set `id` to the Linux device node, for example `/dev/video0`.
- The first version supports a single color stream only. If `stream` is provided, it must be `"color"`.
- Configure `pixel_format` as `rgb24` or `bgr24`. The driver negotiates a native V4L2 format internally and converts in-process before publishing frames.
- RealSense V4L2 nodes are intentionally rejected so users do not accidentally bypass the dedicated `realsense` driver.
- Native passthrough (`mjpeg`, `yuyv`, etc.) is intentionally out of scope for the first pass so the existing preview stack keeps working unchanged.

## Controller Resolution

`rollio collect` resolves camera drivers exactly the same way it resolves any
other device driver. Looks for `rollio-device-<driver>`:

1. In the workspace `target/debug/`
2. In the controller's own directory or `cameras/build/<driver>/`
3. Anywhere on `$PATH`

External camera drivers can be added without changing the controller as long
as they follow the unified `rollio-device-*` naming convention and the
iceoryx2 bus contract.
