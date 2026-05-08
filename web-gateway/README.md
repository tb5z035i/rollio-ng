# rollio-web-gateway

**Browser UI host and WebSocket proxy** (Axum): serves static assets from **`ui/web/dist`** by default, exposes **`/api/runtime-config`** for the SPA, and proxies **`/ws/control`** and **`/ws/preview`** to the upstream Rollio WebSocket URLs in `UiRuntimeConfig`.

## CLI

- **`--config`** / **`--config-inline`** — `UiRuntimeConfig` (upstream WebSocket URLs, browser key bindings, etc.).
- **`--asset-dir`** — Path to built frontend (default `ui/web/dist`).

Use this when you want to use the **web** UI instead of (or alongside) the Ink terminal UI, while keeping the same backend visualizer/control servers.

## See also

- [`ui/web/`](../ui/web/) — frontend source and build output.
