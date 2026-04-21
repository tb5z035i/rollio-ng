# Rollio packaging

Two artifacts come out of one source tree (target: Ubuntu 24.04 / Python 3.12):

- **`rollio_<version>_<arch>.deb`** — all Rust binaries under `/usr/bin/` (including `rollio-encoder`) plus the C++ RealSense camera driver (`rollio-device-realsense`, built from [`cameras/realsense/`](../cameras/realsense/) by `cmake --build cameras/build`, with [`third_party/librealsense/`](../third_party/librealsense/) statically linked into the executable so there is no `librealsense2.so` runtime dependency) and terminal/web UI bundles under `/usr/share/rollio/ui/{web,terminal}/dist/`. The terminal UI is an esbuild-bundled ESM artifact (`react`, `ink`, `ws` inlined; `sharp` left external because it ships per-arch native bindings). The deb additionally vendors `sharp` and its runtime closure under `/usr/share/rollio/ui/terminal/node_modules/`, and the `rollio-native-ascii.node` N-API addon under `/usr/share/rollio/ui/terminal/native/`, so `node /usr/share/rollio/ui/terminal/dist/index.js` runs without a system `npm install`. Also bundles the AIRBOT Play CAN host setup (helper scripts under `/bin/`, udev rules under `/lib/udev/rules.d/`, and the `slcan@.service` unit under `/lib/systemd/system/`) — vendored verbatim from `third_party/airbot-play-rust/root/` into [`debian/`](../debian/) so packaging is self-contained, with maintainer scripts that load the CAN kernel modules and reload udev/systemd on install. `Depends:` is computed from `dpkg-shlibdeps` over the staged `usr/bin/` ELFs and includes the FFmpeg closure pulled in by the encoder plus `libusb-1.0-0` / `libudev1` for the RealSense driver; on top of that, it pins `can-utils, iproute2, kmod, udev, usbutils`. `nodejs` is intentionally **not** an apt dependency because Ubuntu's `nodejs` package lags what Ink/the terminal UI need; `rollio setup` instead detects `node` at runtime and points the operator at <https://nodejs.org/en/download> if it is missing or unusable.
- **`rollio_device_nero-<version>-py3-none-any.whl`** — Nero hardware driver wheel. Operators install it into a venv when they actually need the AGX Nero driver. The wheel pulls Pinocchio (`pin>=3.0`) and friends from PyPI; those wheels vendor a large C++ closure (Boost, Coal, Octomap, …) via `cmeel` and intentionally stay out of the `.deb`.

## Build and pack

```bash
make build           # rust + C++ + UI (default: release)
make package         # ./build.sh all -- stages + dpkg-deb + uv build
# or one shot:
make package-all
```

`make package` does not compile. It runs `./build.sh all`, which:

1. Asserts `target/release/rollio*`, `cameras/build/realsense/rollio-device-realsense`, `ui/web/dist`, `ui/terminal/dist`, `ui/terminal/native/rollio-native-ascii.node`, and `ui/terminal/.deb-vendor/node_modules/sharp` exist (the last is produced by `npm run build:bundle`, which vendors sharp + its runtime deps including the per-arch `@img/sharp-*` native binding).
2. Stages the Rust binaries plus the C++ camera driver(s) into `.deb-staging/rollio/usr/bin/`, the UI bundles into `.deb-staging/rollio/usr/share/rollio/ui/{web,terminal}/dist/`, the native ASCII addon into `.deb-staging/rollio/usr/share/rollio/ui/terminal/native/`, and the vendored sharp tree into `.deb-staging/rollio/usr/share/rollio/ui/terminal/node_modules/`. Then runs `dpkg-shlibdeps` over `usr/bin/` only.
3. Copies the [`debian/`](../debian/) tree (`DEBIAN/`, `bin/`, `lib/`) wholesale into `.deb-staging/rollio/`, then renders [`debian/DEBIAN/control.in`](../debian/DEBIAN/control.in) into `DEBIAN/control` by substituting `@DEB_VERSION@`, `@DEB_ARCH@`, and `@SHLIBS@` (the `dpkg-shlibdeps` output). Everything that is not produced from build artifacts at pack time lives under `debian/` — edit it there.
4. Builds the Nero wheel via `uv build --wheel --out-dir dist robots/nero` (falls back to `python3 -m build --wheel` if `uv` is missing).
5. Writes the `.deb` with `dpkg-deb --root-owner-group --build` to `dist/`.

Tooling required at pack time:

- `dpkg-dev` (provides `dpkg-deb`, `dpkg-shlibdeps`)
- `uv` for the wheel (recommended): `pipx install uv` — or `python3-build` as fallback
- `make package-deps` installs the apt-side helpers (omits Python — use `uv`)

`./build.sh` accepts subcommands when you only need one artifact: `core`, `nero`, `clean`. Env overrides: `DEB_VERSION` (default `1.0.0-1`), `DEB_ARCH` (default `dpkg --print-architecture`), `DEB_DIST` (default `dist`), `STAGING` (default `.deb-staging`), `TARGET_DIR` (default `target/release`).

Example:

```bash
DEB_VERSION=0.2.0-1 DEB_DIST=/tmp/out ./build.sh core
```

Network access is required at pack time so `uv build` can resolve `pyAgxArm` from GitHub (pinned by SHA in [`robots/nero/pyproject.toml`](../robots/nero/pyproject.toml)).

## Operator install

```bash
sudo apt install ./dist/rollio_*.deb

# Required for `rollio setup` (terminal UI runs under Node.js / Ink).
# The .deb does NOT pull this from apt because Ubuntu's `nodejs` is too
# old. Grab a current build from https://nodejs.org/en/download instead;
# `rollio setup` will print the same URL if `node` is missing on PATH.

# Optional, only if you need the Nero hardware driver:
python3 -m venv ~/rollio-venv
~/rollio-venv/bin/pip install ./dist/rollio_device_nero-*.whl
source ~/rollio-venv/bin/activate                # exposes rollio-device-agx-nero on PATH
rollio collect -c /path/to/config.toml
```

The venv is required because Ubuntu 24.04's system Python is PEP 668 externally-managed. The controller spawns `rollio-device-agx-nero` from `PATH`, so as long as the operator's shell has the venv active (or `~/rollio-venv/bin` on `PATH`), it picks up the wheel-provided console-script.

## Runtime layout (FHS) — `rollio.deb`

| Path | Purpose |
|------|---------|
| `/usr/bin/rollio`, `/usr/bin/rollio-ui-server`, `/usr/bin/rollio-control-server`, `/usr/bin/rollio-encoder`, … | Rust binaries (encoder included) |
| `/usr/bin/rollio-device-realsense` | C++ Intel RealSense camera driver (built from [`cameras/realsense/`](../cameras/realsense/); links [`third_party/librealsense/`](../third_party/librealsense/) statically, so the only runtime deps are `libusb-1.0-0` and `libudev1` from Ubuntu apt) |
| `/usr/share/rollio/ui/web/dist/` | Built web UI |
| `/usr/share/rollio/ui/terminal/dist/` | Built terminal UI (esbuild bundle; run with `node /usr/share/rollio/ui/terminal/dist/index.js`). Includes a `package.json` `{"type":"module"}` ESM marker. |
| `/usr/share/rollio/ui/terminal/node_modules/` | Vendored `sharp` runtime closure (kept external from the bundle because it ships per-arch native bindings) |
| `/usr/share/rollio/ui/terminal/native/rollio-native-ascii.node` | Rust N-API addon for the native ASCII renderer worker |
| `/bin/can_add.sh`, `/bin/slcan_add.sh`, `/bin/bind_airbot_device` | AIRBOT Play CAN helper scripts vendored under [`debian/bin/`](../debian/bin/). On Ubuntu (usrmerge) `/bin` is the same directory as `/usr/bin`. |
| `/lib/udev/rules.d/90-usb-can.rules`, `/lib/udev/rules.d/90-usb-slcan.rules` | udev rules that bring up `can*`/`ttyCAN*` devices via the helpers above |
| `/lib/systemd/system/slcan@.service` | Templated unit started by the `90-usb-slcan` rule for each `ttyCAN%n` |

The `.deb`'s `postinst` runs `modprobe` for `can`, `can_raw`, `slcan`, `can_dev` (best-effort) and reloads udev + `systemd-udevd`; `postrm` reloads udev/systemd and removes any `*airbot*.rules` written into `/etc/udev/rules.d/` by `bind_airbot_device` on `purge`. This mirrors `third_party/airbot-play-rust/scripts/install-system-setup.sh` so operators don't need a separate `airbot-play-rust` install.

The Nero wheel installs into the operator's venv (`<venv>/lib/python3.12/site-packages/rollio_device_nero/`) and exposes `<venv>/bin/rollio-device-agx-nero`.

## Environment variables

| Variable | Meaning |
|----------|---------|
| `ROLLIO_SHARE_DIR` | Directory that **contains** `ui/web/dist/index.html` (same shape as the repo or `/usr/share/rollio`). Overrides auto-detection. |
| `ROLLIO_STATE_DIR` | Writable directory for child process cwd, `rollio-setup-logs`, staging directories, and related state. |
| `ROLLIO_LOG_DIR` | Directory where `rollio collect` writes per-child log files (`device-*.log`, `encoder-*.log`, …). Defaults to `<invocation cwd>/rollio-logs`. |

If neither `ROLLIO_STATE_DIR` nor `XDG_STATE_HOME` / `$HOME` is set, the controller falls back to `<workspace>/target/rollio-state` (compile-time workspace), which suits in-tree development.

`rollio collect` log files default to `${PWD}/rollio-logs/` (the directory you ran the command from), so that `device-camera.log`, `encoder-*.log`, etc. live next to your `config.toml` and recorded dataset. Set `ROLLIO_LOG_DIR` if you would rather collect them elsewhere (e.g. a writable scratch volume).

## Controller share-root resolution order (web UI)

1. `ROLLIO_SHARE_DIR` if valid
2. Compile-time workspace root if `ui/web/dist` exists there (developer checkout)
3. `/usr/share/rollio` if packaged assets are present
