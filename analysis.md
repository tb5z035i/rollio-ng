# Adding cora as a Second Middleware to Rollio-ng

> Analysis of how to update the rollio framework so it can use **cora** (Fast-DDS based, from `/Users/tianbeiwen/Develop/robot/framework`) as message middleware while keeping **iceoryx2** support.

---

## TL;DR

Rollio-ng and cora are **incompatible at every layer**: language (Rust vs C++), message system (iceoryx2 `ZeroCopySend` POD vs ROS2 IDL/fastcdr), wire protocol (iceoryx2 SHM vs RTPS+SHM), and API model (typed `Service` builders vs templated `ChannelReader`/`Writer`). Cora ships **no C ABI** — only an `extern "C"` plugin factory that still returns `std::shared_ptr<Node>` (Itanium C++ ABI). There is also **no transport abstraction in rollio today** — every crate calls `iceoryx2::prelude::*` directly.

So "use cora as middleware while keeping iceoryx2" really means *one of three things*. Pick before estimating, because effort varies by ~5×.

---

## Current state (quick reference)

### Rollio side (`/Users/tianbeiwen/Develop/rollio-ng`)

- 14 workspace crates use `iceoryx2` directly; only `rollio-bus` is transport-agnostic (just names + buffer constants).
- Messages in `rollio-types/src/messages.rs` derive `ZeroCopySend` + `#[repr(C)]`, fixed-capacity arrays (e.g. `JointVector15`, `FixedString4096`, `CameraFrameHeader`).
- All "RPC" is layered on top of pub/sub (e.g. `SetupCommandMessage` ↔ `SetupStateMessage`). iceoryx2's native request/response is unused.
- One `Node<ipc::Service>` per OS process; coordination = OS process tree + string topic names.
- UI uses WebSockets to `visualizer` / `control-server`; no native iceoryx2 dependency in the UI process.

### Cora side (`/Users/tianbeiwen/Develop/robot/framework`)

- Builds as `libcora_framework.so`, linkable via `find_package(cora)` (`framework/cmake/coraConfig.cmake` lines 12–34).
- API is C++ templates: `ChannelWriter<T, TPubSubType>::send`, `ChannelReader<T, TPubSubType>::receive`, `RPCClient::call` (`framework/channel.h` 64–80, 112–145; `rpc_channel.h` 221–240).
- RPC topics are `rpc/<service>/{request,response}` (`rpc_channel.h` 330–353) — easy to model from the rollio side.
- Underlying: Fast-DDS (pinned to internal package version `v1.4.0` via `deps/manifest.py`). SHM transport on by default; `use_udp` togglable in `DDSSystemConfig` (`framework.h` 20–31).
- 54 `.idl` files compiled with `fastddsgen -typeros2 -replace -cs` (`framework/CMakeLists.txt` 51–57). ROS2-compatible CDR wire format.
- Python wrapper exists (`cora._cora` pybind11) but is **not** pure Python — still requires `libcora_framework.so`.
- **No C ABI** for pub/sub/RPC. Only the plugin loader (`framework/plugin_loader.h` 22–23) uses `extern "C"`, and only for symbol naming.

---

## Three strategic options

| # | Approach | What it really means | Pros | Cons |
|---|----------|----------------------|------|------|
| **A** | **Bridge process** (recommended for a *first cut*) | One new C++ binary that links `cora::framework` + iceoryx2 C++; subscribes on one side, republishes on the other. Per-topic mapping. | No Rust↔C++ FFI, no rollio code changes, fastest path to "cora coexists" | Extra hop = lost zero-copy, double serialization on bridged topics, one place to maintain mappings, latency on bridged paths only |
| **B** | **Rust binding to cora + transport trait** | Build a `cora-sys`/`cora` Rust crate via `cxx` over a hand-written C++ shim. Add a `Transport` trait in a new `rollio-transport` crate. Refactor all 14 binaries to use it. Configure backend per topic. | Cleanest long-term, single message types in pure Rust, per-topic backend choice, future-proof for adding Zenoh etc. | Largest refactor; you own the FFI shim and IDL→Rust codegen for any cora-typed topic; fastrtps v1.4.0 ABI surface |
| **C** | **Sidecar = cora C++ subsystem only** | Don't expose cora to Rust at all. Write the *new* nodes you need cora for as cora C++ plugins. Have those plugins talk to rollio via a single bridge (a special case of A, narrower) or via files/sockets/Python. | Smallest change in rollio; clean process boundary | Doesn't actually let rollio Rust crates "use cora" — only side-by-side |

The choice depends on **why** you want cora in rollio. If it's mainly to interoperate with existing cora nodes (camera drivers, control nodes already written in cora), **A** or **C** is the right answer. If you want every rollio service to optionally choose its bus, you need **B**.

The full procedure for **A** and **B** is below; **C** is a degenerate case of **A**.

---

## Approach A — Bridge process

### Goal

A standalone Linux binary `rollio-cora-bridge` that, for an explicit per-topic config, copies messages between iceoryx2 and cora in one or both directions.

### Procedure

1. **Add submodule + build cora SDK locally.** Vendor `framework/` as a git submodule (or use prebuilt `cora_x86_64` SDK + wheel from `./build.sh cora_x86_64 --release`). Drop it under `third_party/cora/` (mirror the iceoryx2 placement). *Verify:* `find_package(cora REQUIRED)` works in a hello-world CMake test.

2. **Pick the bridge implementation language.** Strongly prefer **C++**, because:
   - cora is C++ first-class.
   - iceoryx2 C++ bindings already work (`cameras/realsense/src/main.cpp:1` shows `#include "iox2/iceoryx2.hpp"`).
   - You skip the Rust↔C++ FFI question entirely.

3. **Define the bridged-topic catalog.** A YAML/TOML file like:

   ```toml
   [[bridge]]
   direction = "iox2_to_cora"   # or "cora_to_iox2"
   iox2_service = "robot/leader/state"
   cora_topic = "/robot/leader/state"
   message = "rollio_msgs::JointVector15"  # IDL name
   qos = "reliable"
   ```

   *Verify:* parser unit test with a fixture file.

4. **Generate matching IDL for every rollio message you want bridged.** For each `#[repr(C)]` Rust struct, write a one-to-one `.idl` (e.g. `rollio_msgs/JointVector15.idl`) and run `fastddsgen -typeros2`. Layout *cannot* be auto-derived — write a small table of equivalences (`f64`→`double`, fixed array→bounded sequence/array, `FixedString4096`→`string<4096>`).

5. **Implement the bridge per topic.** Use C++ templates:

   ```cpp
   template <typename T>
   class Iox2ToCora {
     iox2::Subscriber<T> in_;
     framework::ChannelWriter<T, TPubSubType> out_;
   };
   ```

   For each entry in the catalog, register an instance. Run a thread per direction (or a coroutine/`stdexec` task per pair).

6. **Handle camera frames specially.** `CameraFrameHeader` + `[u8]` is not a fixed POD — bridge it as `sensor_msgs/CompressedImage` with header fields mapped to `cora_msgs/CameraFrameHeader.idl`. This is the largest single message and worth its own benchmark.

7. **Add to `runtime_plan.rs`.** New `ChildSpec` for `rollio-cora-bridge`, conditional on a `cora_bridge.toml` existing or a `--cora` flag.

8. **Document the operating model.** README section: "Bridged topics lose zero-copy and add ~100µs latency; reserve cora for slow control plane / external interop."

### Workload — Approach A

| Phase | Task | Days (1 senior eng.) |
|---|---|---|
| 1 | Vendor cora as submodule, get SDK building inside rollio | 2 |
| 1 | CMake glue, link against `cora::framework` + iceoryx2 C++ | 1 |
| 2 | Bridge skeleton in C++ (catalog parser, runner) | 2 |
| 3 | IDL files for ~10 most-used rollio messages | 3 |
| 3 | C++ ↔ Rust struct equivalence audit (corner cases) | 2 |
| 4 | Implement 5 simplest topic bridges + smoke test | 3 |
| 4 | Camera frame bridge with `[u8]` payload mapping | 3 |
| 4 | RPC-style bridges (request/response topic pairs) | 2 |
| 5 | `runtime_plan.rs` integration + config knobs | 1 |
| 5 | Latency / throughput benchmark (vs direct iox2 path) | 2 |
| 5 | Docs + ops runbook | 1 |
| | **Subtotal** | **22 person-days (~4.5 weeks)** |

Add ~3 days buffer for fastrtps v1.4.0 packaging issues (it's an internal version pin; collisions with system Fast-DDS are likely).

---

## Approach B — Rust binding to cora + transport trait

### Goal

Every rollio crate calls `rollio_transport::Publisher<T>` / `Subscriber<T>`. Backend is selected per topic at runtime via config: `iceoryx2` (default) or `cora`.

### Procedure

1. **Build a Rust crate `cora-sys` (low level FFI).**
   - Hand-written C wrapper `cora_c.h` / `cora_c.cpp` over the C++ API. You cannot bindgen the C++ headers directly — templates and `std::shared_ptr` don't cross the C boundary. The wrapper exposes opaque pointers + flat functions:

     ```c
     typedef struct cora_writer cora_writer_t;
     cora_writer_t* cora_writer_create(const char* topic, int qos_id, uint32_t type_id);
     int cora_writer_send(cora_writer_t*, const void* data, size_t len);
     void cora_writer_destroy(cora_writer_t*);
     ```

   - For each cora-typed message, the wrapper instantiates the right `ChannelWriter<T, TPubSubType>` template — i.e. you compile per-type C wrappers (or a generic byte-blob wrapper that you fastcdr-encode in Rust).
   - *Recommendation:* go **byte-blob first** — `cora_writer_send_bytes(handle, ptr, len)`. Defer typed wrappers until you have a benchmark proving the byte path is too slow.
   - Build glue: `build.rs` runs CMake on the wrapper, links `cora::framework`, exposes a `cora-sys` crate with `extern "C"` declarations.
   - *Verify:* a Rust integration test publishes "hello" on a topic and a C++ cora subscriber prints it.

2. **Build `cora` (high-level Rust crate).** Wraps `cora-sys` with safe types. RAII `Drop` on writers/readers. Async wrapper if needed.

3. **Generate Rust types from IDL.** Either:
   - (a) Use `fastcdr` C++ + a new `cora-msggen` codegen tool that emits Rust structs + serde-cdr code, or
   - (b) Adopt a maintained Rust CDR crate (e.g. `cdr` crate) and hand-write the structs once. With ~10–20 bridged types this is tractable.

4. **Introduce `rollio-transport` crate** with:

   ```rust
   pub trait TopicPublisher<T>: Send + Sync {
       fn send(&self, msg: &T) -> Result<(), TransportError>;
   }
   pub trait TopicSubscriber<T>: Send + Sync {
       fn try_recv(&self) -> Result<Option<T>, TransportError>;
   }
   pub trait Transport {
       fn publisher<T: Message>(&self, topic: &str, opts: PubOpts) -> Box<dyn TopicPublisher<T>>;
       fn subscriber<T: Message>(&self, topic: &str, opts: SubOpts) -> Box<dyn TopicSubscriber<T>>;
   }
   ```

   Provide `Iceoryx2Transport` and `CoraTransport` impls.

5. **Refactor `rollio-types/messages.rs`.** Split into:
   - Pure data structs (no transport derives).
   - A `messages-iceoryx2` feature that adds `ZeroCopySend` + `#[type_name]`.
   - A `messages-cora` feature that adds CDR encode/decode.

   This is the most invasive change — every `#[derive(ZeroCopySend, ...)]` becomes conditional. Be aware: `ZeroCopySend` requires layout guarantees that the cora path doesn't share, and the abstraction over both will likely force you to use *byte transport*, not zero-copy, on the cora side.

6. **Refactor each crate** (`controller`, `visualizer`, `encoder`, `episode-assembler`, `storage`, `teleop-router`, `control-server`, `monitor`, `cameras/v4l2`, `robots/pseudo`, `robots/airbot_play_rust`, `test/test-publisher`, `test/bus-tap`) to:
   - Replace `iceoryx2::prelude::*` with `rollio_transport::*`.
   - Replace `NodeBuilder`, `service_builder().publish_subscribe()`, `publisher_builder()`, `subscriber_builder()` with the trait calls.
   - Hide all iceoryx2-specific tuning (`max_publishers`, `max_subscribers`, `max_nodes`) behind `PubOpts`/`SubOpts`.
   - **Keep** existing iceoryx2 behavior as the default backend so behavior is unchanged when the config doesn't enable cora.

7. **Add per-topic backend config.** Extend `rollio-types/config.rs` with:

   ```toml
   [transport]
   default = "iceoryx2"
   [transport.overrides]
   "robot/leader/state" = "cora"
   ```

8. **Re-implement camera frame path carefully.** The `publish_subscribe::<[u8]>().user_header::<CameraFrameHeader>()` pattern (e.g. `cameras/v4l2/src/main.rs:626-645`) doesn't translate cleanly to the typed-DDS model. Likely needs a `CameraFrame` IDL with a bounded byte sequence + benchmark.

9. **Verify each binary** still passes its existing integration tests under (a) iceoryx2-only and (b) cora-only configs. Add new tests for mixed config (some topics on each).

### Workload — Approach B

| Phase | Task | Days (1 senior eng.) |
|---|---|---|
| 1 | C wrapper for cora (byte-blob API + RPC) | 5 |
| 1 | `cora-sys` Rust crate, build.rs, CMake glue | 3 |
| 1 | `cora` safe Rust wrapper crate | 3 |
| 1 | Rust integration test (talker/listener) against cora C++ | 2 |
| 2 | CDR encode/decode for ~10 message types in Rust | 4 |
| 3 | `rollio-transport` trait crate + iceoryx2 backend impl | 4 |
| 3 | cora backend impl on top of `cora` crate | 4 |
| 4 | Refactor `rollio-types` to remove iceoryx2 from public API | 3 |
| 5 | Refactor 14 binaries to use `rollio-transport` | 8 |
| 6 | Camera frame path redesign + benchmark | 4 |
| 6 | RPC-as-topic helper (matches existing rollio control-plane idiom) | 2 |
| 7 | Per-topic backend config + factory | 2 |
| 8 | UI integration verification (visualizer, control-server) | 2 |
| 9 | Cross-config integration tests + CI | 3 |
| 9 | Documentation, migration guide | 2 |
| | **Subtotal** | **51 person-days (~10–11 weeks)** |

With buffer for FFI debugging, fastrtps v1.4.0 packaging, and CDR codec edge cases: **realistically 12–14 weeks** for one senior engineer, or **6–8 weeks** for two.

---

## Recommendation

**Approach A first, Approach B if/when warranted.**

Reasoning:

- The motivation for adding cora is almost certainly **interop with existing cora nodes** (the robot repo has a substantial sensor/control plugin set). A bridge solves that without touching rollio's hot path.
- Rollio's iceoryx2 use is *deeply* baked in (no transport trait, `ZeroCopySend` in message types, all 14 binaries direct-coupled). Approach B is a 10+ week refactor whose benefit is "future flexibility" — not concrete user value.
- Approach A is cleanly reversible. If you later decide bridging is too costly for some topic, you can migrate that *single* topic to a Rust-native cora path on a per-topic basis — that's a reasonable evolution of A toward B.
- Critical risk in B: making types compatible with both iceoryx2 (`ZeroCopySend`) and cora (CDR) typically forces the cora path to **not be zero-copy**, eroding the main reason rollio chose iceoryx2 in the first place.

---

## Open questions to answer before committing

1. **What's the actual goal?** "Interop with existing cora nodes" vs "give services a backend choice" vs "long-term migration off iceoryx2"? The right approach depends on this.
2. **Which topics need cora?** A small set → bridge wins. All topics → forces B.
3. **Is fastrtps `v1.4.0` (cora's pin) compatible with whatever Fast-DDS version is on your deployment hosts?** If you ship rollio as a `.deb`, this becomes a packaging problem; the cora SDK bundles its own `libfastrtps.so` but conflicts with system installs are common.
4. **C++ build chain on rollio's target hosts.** Cora wants Clang 17+ (per `robot/CLAUDE.md` §8); rollio's `cameras/` already uses g++. Pin a single compiler before building the bridge or FFI shim.
