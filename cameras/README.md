# Cameras

In-repo camera drivers live under `cameras/`.

## Layout

- `pseudo/`: synthetic reference camera used by CI and smoke tests.
- `realsense/`: hardware-backed RealSense driver with no-hardware fallback coverage.
- `v4l2/`: Rust V4L2 webcam driver that converts native frames to `rgb24`/`bgr24`.
- `build/`: generated CMake build directory.

## Add A New Camera Driver

1. Create a folder under `cameras/<driver_name>/`.
2. Expose a binary named `rollio-camera-<driver_name>`.
3. Implement the existing driver CLI contract:
   - `probe`
   - `validate`
   - `capabilities`
   - `run --config <path>` or `run --config-inline <toml>`
4. Publish frames to `camera/{name}/frames`.
5. Listen for `control/events` and exit cleanly on shutdown.
6. Add `add_subdirectory(<driver_name>)` to `cameras/CMakeLists.txt` if it is a C++ driver.
7. Add focused tests or at least a no-hardware validation path.

## V4L2 Webcam Driver

The in-repo `v4l2` driver is a Rust binary exposed as `rollio-camera-v4l2`.

- Use `driver = "v4l2"` and set `id` to the Linux device node, for example `/dev/video0`.
- The first version supports a single color stream only. If `stream` is provided, it must be `"color"`.
- Configure `pixel_format` as `rgb24` or `bgr24`. The driver negotiates a native V4L2 format internally and converts in-process before publishing frames.
- RealSense V4L2 nodes are intentionally rejected so users do not accidentally bypass the dedicated `realsense` driver.
- Native passthrough (`mjpeg`, `yuyv`, etc.) is intentionally out of scope for the first pass so the existing preview stack keeps working unchanged.

## Controller Resolution

`rollio collect` first looks for in-repo camera binaries under
`cameras/build/<driver_name>/rollio-camera-<driver_name>`.

For Rust camera drivers built as workspace members, it also checks
`target/debug/rollio-camera-<driver_name>` before falling back to `PATH`.

If no in-repo binary exists, it falls back to `PATH`, so external camera drivers
can be added without changing the controller as long as they follow the same CLI
and iceoryx2 bus contract.
