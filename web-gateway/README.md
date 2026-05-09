# rollio-web-gateway

**Browser-facing front door:** serves the built SPA from **`ui/web/dist`**, exposes a tiny JSON **`/api/runtime-config`** hook, and **proxies** WebSocket URLs so the browser only talks to one origin while the real **`rollio-control-server`** / **`rollio-visualizer`** keep listening on loopback elsewhere.

---

## Concepts & behaviors

### Why this exists

Browsers cannot always connect to raw `ws://127.0.0.1:randomPort` endpoints due to packaging, HTTPS upgrades, or remote port forwarding. The gateway **terminates HTTP** where the user expects it and **forwards** `/ws/control` and `/ws/preview` to the actual Rollio sockets listed in **`UiRuntimeConfig`**.

### What it does **not** do

- **No iceoryx2** — it never joins the shared-memory graph.
- **No episode policy** — it does not interpret dataset logic; it just moves bytes.

### CLI

- **`--config`** / **`--config-inline`** — provide `UiRuntimeConfig` (upstream WebSocket URLs, browser keybindings for episode actions).
- **`--asset-dir`** — static files (defaults to `ui/web/dist` relative to cwd).

You must **`npm run build`** (or equivalent) in `ui/web` before collecting if the packaged deb expects assets at the default path.

---

## iceoryx2

None.

---

## Lifecycle

**Spawned by:** `rollio collect` as the `ui` web child when configured.

**Children:** none.

---

## Built product & dependencies

**Binary:** `rollio-web-gateway` (Axum + tokio). No special apt deps beyond workspace build.

## See also

- [`ui/web/`](../ui/web/), [`rollio-control-server`](../control-server/README.md), [`rollio-visualizer`](../visualizer/README.md).
