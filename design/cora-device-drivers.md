---
name: Cora Device Drivers
overview: 把 Cora（Fast-DDS 框架，C++ SDK）的若干 ROS 风格 topic 透传成 rollio device，每种消息一个独立二进制：rollio-device-imu-cora、rollio-device-tactile-cora、rollio-device-gripper-cora。每个 device crate 自带 cpp/ shim + build.rs + 本地 Rust FFI 封装，互不依赖。
todos:
  - id: imu-cora
    content: 新建 sensors/imu_cora（bin rollio-device-imu-cora），订阅 sensor_msgs/Imu，发布 SensorStateKind::ImuAccelGyro (6×f32) 到 samples/imu_accel_gyro。自带 cpp/ + build.rs（env var 发现 SDK）。
    status: pending
  - id: tactile-cora
    content: 新建 sensors/tactile_cora（bin rollio-device-tactile-cora），订阅 sensor_msgs/PointCloud2，按 pointcloud_field_map 投影 6 列、严格 FLOAT32 + 小端 + 固定 N 校验，发布 SensorStateKind::TactilePointCloud2 到 samples/tactile_point_cloud2。自带 cpp/ + build.rs。
    status: pending
  - id: gripper-cora
    content: 新建 robots/gripper_cora（bin rollio-device-gripper-cora），订阅 sensor_msgs/JointState (dof=1)，按 joint_name 名字解析、复制到 JointVector15[0]，按 publish_states 发到 states/{kind}。自带 cpp/ + build.rs。
    status: pending
  - id: example-config-and-docs
    content: 更新 config/config.example.toml 增加三段 [[devices]]；每个 device crate 写 README 写明 env var 发现机制与打包 RPATH 选项
    status: pending
isProject: true
---

# Cora 设备透传 —— 按消息类型拆分的多 device 方案

## 背景

rollio 框架通过独立 device 二进制（`robots/pseudo`、`robots/airbot_play_rust`、`cameras/v4l2`）把数据发布到 iceoryx2 总线上。部署环境另有一套 **Cora** 框架（Fast-DDS 发布订阅，插件化模型，提供 C++ SDK）。

本计划新增三个 **独立** device 驱动，每种 Cora 消息一个二进制，沿用 rollio "一个 device 一种数据来源" 的物理隔离风格。本期 **不做相机**（CompressedVideo），等三种小设备落地稳定后再单独立项。

| Device binary | crate 路径 | Cora msg | rollio kind | rollio state |
| --- | --- | --- | --- | --- |
| `rollio-device-imu-cora` | `sensors/imu_cora/` | `sensor_msgs::msg::Imu` | sensor | `ImuAccelGyro`（6 × f32） |
| `rollio-device-tactile-cora` | `sensors/tactile_cora/` | `sensor_msgs::msg::PointCloud2` | sensor | `TactilePointCloud2`，shape `[N, 6]` |
| `rollio-device-gripper-cora` | `robots/gripper_cora/` | `sensor_msgs::msg::JointState` | robot, dof=1 | `joint_position`/`joint_velocity`/`joint_effort` 任选 |

所有 IDL header 由 Cora SDK 自带（`include/cora/dds/generated/sensor_msgs/msg/{Imu,JointState,PointCloud2,PointField}.h`），无需 IDL 编译流程。

**关键边界约束**：rollio 框架的代码与构建系统 **不依赖** 仓库内 `examples/cora_sdk/` 下任何文件或路径——该目录纯粹是开发期参考资料。SDK 在编译期通过环境变量（`CORA_SDK_ROOT` 等）从仓库外指定，详见设计决策 5。

**前置基础（已落地）**：`be749ad feat(devices): add Sensor channel kind with IMU and tactile point cloud` 提供了 `DeviceType::Sensor`、`SensorStateKind::{ImuAccelGyro, TactilePointCloud2}`、`SensorFrameHeader`、`channel_sample_service_name`、伪设备 `run_sensor_channel`、LeRobot 多维列、Web UI sensor 侧边栏。

**本期不在范围内**：

- 相机透传（`foxglove_msgs::msg::CompressedVideo`）—— 后续单立项目 `rollio-device-camera-cora`。
- 多关节臂透传（`sensor_msgs::msg::JointState` dof > 1）—— gripper 是 dof=1 特例。
- `Twist`、`Odometry`、`PoseStamped`、`TFMessage`、`NavSatFix`、`CameraInfo`、`BatteryState`、`Joy`。
- 双向桥接（rollio → Cora）。

## 设计决策

### 1. 每个 device crate 自包含；不抽公共 cora-bridge lib

每个 cora-* device crate 自带：

- `cpp/` —— C++ shim：`bridge.cpp` 做 `DDSParticipant + CallbackExecutor` 生命周期 + 一种 `ChannelReader<MsgT, MsgTPubSubType>` 订阅 + 把字段转成扁平 C 回调。
- `build.rs` —— env var 发现 SDK、驱动 cmake、bindgen 生成 FFI、emit 链接指令 + RPATH。
- `src/cora.rs` —— 本地 Rust FFI 安全封装（Bridge::new/start/stop/subscribe + 该 device 所需的强类型 Sample struct + C trampoline）。
- `src/{main,config,probe,query,validate,run}.rs` —— 标准 rollio device CLI。

**为什么不抽公共 lib**：

- 每个 device 只需要 1 种 message subscriber，公共 crate 强制把另外 2 种 IDL 模板实例化进每个 device 的链接路径，编译时间纯浪费。
- DDS 生命周期 ~30 行 C++ + build.rs ~80 行 Rust 是 stable boilerplate，复制 3 份的 drift 风险低于跨 crate API 重构风险。
- 每个 device 可以独立调 QoS、独立升 SDK 版本，互不影响。
- 加 `camera-cora` 时复制 `imu_cora` 改 subscriber 类型 + sample struct 即可，新 device 无需先改公共库 API。

### 2. C++ shim 用 `DDSParticipant + CallbackExecutor + ChannelReader::setCallback`，不走 `framework::Framework`

`Framework::initialize(library_path, configs_path)` 期望磁盘上有插件 `.so` + JSON 配置目录，对一个无 Node 的透传来说过度。`DDSParticipant + CallbackExecutor + ChannelReader` 是正确原语，回调由 SDK 内置的 `MutuallyExclusive` 回调组在 worker 线程上派发（参考 `examples/cora_sdk/cora_x86_64/include/cora/channel.h:336-363`）。

每个 device 的 C++ shim 暴露扁平 C ABI，只包含本 device 需要的 subscriber 函数。模板：

```c
// 例如 imu_cora/cpp/include/cora_bridge.h
cora_bridge_ctx_t* cora_bridge_create(const cora_bridge_config_t* config);
int  cora_bridge_start(cora_bridge_ctx_t*);
int  cora_bridge_stop(cora_bridge_ctx_t*);
void cora_bridge_destroy(cora_bridge_ctx_t*);

int32_t cora_bridge_subscribe_imu(
    cora_bridge_ctx_t*, const char* topic, int qos_reliable,
    cora_imu_cb_t cb, void* user);
```

三个 device 的 `bridge.cpp`（DDS 生命周期）几乎一样；subscriber 部分根据 message 类型不同（IMU / JointState / PointCloud2）。

### 3. 三个 device binary 各自一套 CLI 与配置

每个 device 实现 `probe / query / validate / run` 四个子命令（参考 `robots/pseudo/src/bin/device.rs`），controller 看待它们和看待 pseudo 完全一致。

每个 device 只暴露 **一种 channel kind / 一种 state kind**：

- `rollio-device-imu-cora`：probe 只返回 `kind=Sensor` + `imu_accel_gyro`。
- `rollio-device-tactile-cora`：probe 只返回 `kind=Sensor` + `tactile_point_cloud2`。
- `rollio-device-gripper-cora`：probe 只返回 `kind=Robot` + 支持的 RobotStateKind 子集（`joint_position`、`joint_velocity`、`joint_effort`），固定 `dof=1`。

配置示例（每个 device 都有自己一段 `[[devices]]`）：

```toml
# ---- IMU ----
[[devices]]
name = "imu_head"
driver = "imu-cora"
id = "imu_cora_0"
bus_root = "imu_head"
[devices.extra]
cora_domain_id = 0
cora_participant_name = "rollio_imu_head"
cora_use_shared_memory = true
cora_callback_threads = 2

[[devices.channels]]
channel_type = "imu"
kind = "sensor"
enabled = true
sample_rate_hz = 200
publish_states = ["imu_accel_gyro"]
[devices.channels.extra]
cora_topic = "rt/imu/head/data"
cora_qos = "reliable"

# ---- Tactile ----
[[devices]]
name = "tactile_left"
driver = "tactile-cora"
id = "tactile_cora_0"
bus_root = "tactile_left"
[devices.extra]
cora_domain_id = 0
cora_participant_name = "rollio_tactile_left"

[[devices.channels]]
channel_type = "tactile"
kind = "sensor"
enabled = true
sample_rate_hz = 30
publish_states = ["tactile_point_cloud2"]
[devices.channels.extra]
cora_topic = "rt/tactile/left/points"
cora_qos = "best_effort"
tactile_point_count = 1024
pointcloud_field_map = ["x","y","z","fx","fy","fz"]

# ---- Gripper ----
[[devices]]
name = "gripper_right"
driver = "gripper-cora"
id = "gripper_cora_0"
bus_root = "gripper_right"
[devices.extra]
cora_domain_id = 0
cora_participant_name = "rollio_gripper_right"

[[devices.channels]]
channel_type = "gripper"
kind = "robot"
enabled = true
dof = 1
publish_states = ["joint_position", "joint_velocity", "joint_effort"]
[devices.channels.extra]
cora_topic = "rt/gripper/right/state"
cora_qos = "reliable"
joint_name = "gripper_right_finger"   # 单值；从 JointState.name[] 里挑这一项
```

**device-level `[devices.extra]` 字段**（所有三个 device 通用）：

- `cora_domain_id: i32`（默认 0）
- `cora_participant_name: String`（默认 `rollio_<device.name>`）
- `cora_use_shared_memory: bool`（默认 true）
- `cora_use_udp: bool`（默认 true）
- `cora_callback_threads: u32`（默认 2）

**channel-level `[devices.channels.extra]` 字段**（按 device 类型分别要求）：

- 公共：`cora_topic: String`（必填），`cora_qos: "reliable" | "best_effort"`（默认 reliable）
- tactile-cora 额外：`tactile_point_count: u32`（必填，> 0），`pointcloud_field_map: [String; 6]`（必填，长度 6，元素 `""` 表示 slot 留 0）
- gripper-cora 额外：`joint_name: String`（必填，从 `JointState.name[]` 里挑出这一关节）

### 4. 转换器策略

- **IMU** (`(ax,ay,az,gx,gy,gz)` 六个 double → `SensorFrameHeader { sensor_kind: ImuAccelGyro, dtype: F32, ndim: 1, shape: [6,0,...] }` + 6 个 f32 payload)：orientation 在 Phase 1 丢弃，首条消息时 info-log 一次。`timestamp_us` 优先取 `header.stamp`，否则 fall back 到 `MessagePtr::timestamp()`。
- **PointCloud2** → `TactilePointCloud2`：所有命名字段必须 `datatype == FLOAT32`，否则丢弃 + warn-once；必须 `width * height == tactile_point_count`，否则丢弃 + 告警；按 `data[i*point_step + field.offset .. +4]` 当 LE f32 读取，Phase 1 拒收 `is_bigendian == true`；6 个 slot 的值写入长度 `6 * N` 的 `Vec<f32>`，未映射 slot 填 0.0；产出 `SensorFrameHeader { ndim: 2, shape: [N, 6, 0,...] }`。
- **JointState** → 单关节 `JointVector15[0]`：首条消息在 `name[]` 中查找 `joint_name`，不存在则让通道失败并报错；后续按缓存索引取值复制到 `JointVector15[0]`，slot 1–14 零；按 `publish_states` 决定对哪种 state 发 publish；空数组 + 请求该 state → 跳过 + warn-once。

### 5. SDK 通过环境变量从仓库外部发现；`examples/cora_sdk/` 不参与编译/运行链路

**关键约束**：`examples/cora_sdk/` 仅作开发期参考资料，rollio 框架代码与各 device 的 `build.rs` 不能硬编码这个路径。

每个 device 的 `build.rs` 通过 **环境变量** 找 SDK，按以下顺序：

1. `CORA_SDK_ROOT`（通用覆盖，开发者本地一键指过去）。
2. `CORA_SDK_X86_64_ROOT` / `CORA_SDK_AARCH64_ROOT`（按目标架构分别配置，CI 与交叉构建场景用）。
3. 若以上都未设置，让 CMake `find_package(cora REQUIRED)` 走默认搜索路径（系统装的 `/usr/local/lib/cmake/cora` 等）。
4. 若 CMake 也找不到，编译失败并打印明确指引。

build.rs 具体做的事：

1. 解析上述变量；若找到，向 cmake 传 `CMAKE_PREFIX_PATH=$root/lib/cmake/cora`。
2. 在本 device 的 `cpp/` 上跑 `cmake`（`find_package(cora REQUIRED)`）。
3. 输出 `cargo:rustc-link-search=native=$root/lib`，链接 `cora_framework`、`fastrtps`、`fastcdr`、`stdc++`。
4. 输出 `cargo:rustc-link-arg=-Wl,-rpath,$root/lib` 指向 SDK lib（同时支持 `CORA_SDK_RUNTIME_RPATH` 环境变量覆盖，留给 deb/image 打包步骤换成 `$ORIGIN/../lib/cora` 之类）。
5. `cargo:rerun-if-env-changed=CORA_SDK_ROOT`、`CORA_SDK_X86_64_ROOT`、`CORA_SDK_AARCH64_ROOT`、`CORA_SDK_RUNTIME_RPATH`，以及 `cargo:rerun-if-changed=cpp/`。**不**写 `rerun-if-changed=examples/cora_sdk/` —— `examples/` 与编译链路解耦。
6. 非 Linux 目标（macOS dev 机）只跑 bindgen、跳过 cmake/链接，让 `cargo check` 在开发机上能过。

开发者本地最简体验：`export CORA_SDK_ROOT=$(pwd)/examples/cora_sdk/cora_x86_64` 一行搞定。

## 实施大纲

每个 device 的目录结构都是：

```
<crate_root>/
├── Cargo.toml
├── README.md
├── build.rs                     # env var 发现 SDK + cmake + bindgen + 链接 + RPATH
├── cpp/
│   ├── CMakeLists.txt           # find_package(cora REQUIRED) → 静态库 cora_bridge_shim
│   ├── include/cora_bridge.h    # 扁平 C ABI（仅本 device 用到的 subscriber）
│   └── src/
│       ├── bridge.cpp           # DDSParticipant + CallbackExecutor 生命周期
│       └── subscriber.cpp       # 一种 ChannelReader<MsgT, MsgTPubSubType>
└── src/
    ├── main.rs                  # clap CLI: probe / query / validate / run
    ├── cora.rs                  # 本地 Rust FFI 安全封装 + trampoline + Sample struct
    ├── config.rs                # BinaryDeviceConfig 反序列化 + extra 字段
    ├── probe.rs / query.rs / validate.rs
    └── run.rs                   # 顶层编排：iceoryx2 publisher + Cora subscription + signal handler
```

`bridge.cpp` 在三个 device 中几乎相同；差异仅在 `subscriber.cpp`、`cora.rs` 的 Sample struct 与 trampoline。

### A. `sensors/imu_cora/`

- `cpp/`：DDS lifecycle + `framework::ChannelReader<sensor_msgs::msg::Imu, sensor_msgs::msg::ImuPubSubType>`，回调把 `linear_acceleration.{x,y,z}`、`angular_velocity.{x,y,z}`、`orientation.{x,y,z,w}`、`header.stamp` 直交付 C 回调。
- `src/cora.rs`：`ImuSample { ts_us, accel: [f64;3], gyro: [f64;3], orientation: [f64;4] }`，trampoline 把 C 字段转成 ImuSample 再调闭包。
- `src/run.rs`：
  - iceoryx2 publisher：`channel_sample_service_name(bus_root, channel_type, "imu_accel_gyro")`，dynamic payload，header `SensorFrameHeader`。
  - `Bridge::subscribe_imu(topic, qos, |sample| publisher_tx.send(...))`。
  - 转换器：6 个 f64 → 6 个 f32，填 `SensorFrameHeader { sensor_kind: ImuAccelGyro, dtype: F32, ndim: 1, shape: [6,0,...] }`。
  - 安装 signal handler + `control/events` 订阅，触发 bridge.drop + publisher join。

### B. `sensors/tactile_cora/`

- `cpp/`：DDS lifecycle + `framework::ChannelReader<sensor_msgs::msg::PointCloud2, sensor_msgs::msg::PointCloud2PubSubType>`，回调传 `width / height / point_step / row_step / fields[] / data[] / is_bigendian / is_dense / ts`。
- `src/cora.rs`：`PointCloud2Sample { ts_us, width, height, point_step, row_step, fields: Vec<PointField>, data: Vec<u8>, is_bigendian, is_dense }`，`PointField { name, offset, datatype, count }`。
- `src/run.rs`：
  - iceoryx2 publisher：`channel_sample_service_name(..., "tactile_point_cloud2")`，dynamic payload，header `SensorFrameHeader`。
  - `Bridge::subscribe_point_cloud2(...)`。
  - 转换器：校验 width\*height == N、字段 FLOAT32、`!is_bigendian`；按 `pointcloud_field_map` 投 6 列；输出 `Vec<f32>` 长度 `6*N`；`SensorFrameHeader { ndim: 2, shape: [N, 6, ...] }`。

### C. `robots/gripper_cora/`

- `cpp/`：DDS lifecycle + `framework::ChannelReader<sensor_msgs::msg::JointState, sensor_msgs::msg::JointStatePubSubType>`，回调传 `names[]`、`positions[]`、`velocities[]`、`efforts[]` 各自的指针 + 长度 + ts。
- `src/cora.rs`：`JointStateSample { ts_us, names: Vec<String>, positions: Vec<f64>, velocities: Vec<f64>, efforts: Vec<f64> }`。
- `src/run.rs`：
  - 对每个 publish_state 起一个 publisher：`channel_state_service_name(bus_root, channel_type, kind)`，类型 `JointVector15`。
  - `Bridge::subscribe_joint_state(...)`。
  - 转换器（`gripper_mapper.rs`）：首条消息找 `joint_name` 在 `sample.names` 中的索引，缓存；不存在则 `tracing::error!` + 让通道失败。后续每条消息按缓存索引取 position/velocity/effort，写入 `JointVector15[0]`，slot 1–14 零；按 state kind 调对应 publisher tx。空数组 + 请求该 state → 跳过 + warn-once。

### D. workspace `Cargo.toml`

新增 members（按 PR 顺序逐个追加，避免半成品破坏 workspace）：

```toml
members = [
    # ... 现有 ...
    "sensors/imu_cora",
    "sensors/tactile_cora",
    "robots/gripper_cora",
]
```

### E. `config/config.example.toml`

新增设计决策 3 中的三段 `[[devices]]`。

### F. controller、episode-lerobot、web UI —— 预计零改动

三个 device 走标准 rollio 设备 CLI，发布到标准 iceoryx2 服务名。`be749ad` 已经让 assembler 能消费 `samples/` / `states/`、Web UI 能反射 sensor 通道。如果实施时发现某处需要驱动特异化（比如 controller setup wizard 想问 `cora_topic`），单独开后续 PR。

## PR 顺序

worktree（待 `be749ad` 合到 `main`）：

```bash
git worktree add ../rollio-ng-cora-devices -b dev/cora-devices origin/main
cd ../rollio-ng-cora-devices
```

1. **PR-A —— `sensors/imu_cora`**。自带 cpp/ + build.rs + 本地 cora.rs FFI + 全功能 device CLI。端到端集成测试用 `examples/cora_sdk/cora_x86_64/examples/cpp/talker_node.cpp` 模式裁一个 IMU talker。第一个 device 先 settle 整个模式（cpp 布局、build.rs 模板、cora.rs 形状），后续 device 照搬。
2. **PR-B —— `sensors/tactile_cora`**。复制 PR-A 的 cpp/build.rs/cora.rs 模板，换 subscriber 与 Sample struct，加 PointCloud2 转换器 + 集成测试。
3. **PR-C —— `robots/gripper_cora`**。复制模板，换 JointState subscriber 与转换器 + 集成测试。
4. **PR-D —— 样例配置 + 部署文档**。`config/config.example.toml` 三段；三个 device 各自的 README 写明 env var 发现机制 + 打包 RPATH 选项。

每个 PR 单独可审。

## 验收

### 构建健全性
- 设置好 `CORA_SDK_ROOT` 后，`cargo build --workspace` 在 x86_64 与 aarch64 Linux 上都成功。
- 不设置 env var 时，`cargo build -p imu-cora` 给出清晰的 SDK 未发现错误。
- macOS dev 机：`cargo check -p imu-cora`（以及 tactile/gripper）通过；正式构建必须在 Linux 上。

### probe / validate（每个 device 各一）
- `./target/debug/rollio-device-imu-cora probe --json` 输出 `kind=sensor`、`supported_sensor_kinds=["imu_accel_gyro"]`。
- `./target/debug/rollio-device-tactile-cora probe --json` 输出 `kind=sensor`、`supported_sensor_kinds=["tactile_point_cloud2"]`。
- `./target/debug/rollio-device-gripper-cora probe --json` 输出 `kind=robot`、`dof=1`、`supported_states` 列出 joint_*。
- 每个 device `validate --config-inline <错误 toml>` 非零退出 + JSON 错误。

### IMU 端到端
- 在 `examples/cora_sdk/cora_x86_64/examples/cpp/` 模式下裁一个 IMU talker（`sensor_msgs::msg::Imu`）。
- 用 `cora_app` 跑 talker 在 `rt/imu/head/data` 上以 200 Hz 发布。
- 启 controller，载入 `config.example.toml` 的 `imu_head` device。
- `cargo run -p bus-tap -- --channel imu_head/imu --kind sample` 看到 `SensorFrameHeader { sensor_kind: ImuAccelGyro, dtype: F32, ndim: 1, shape: [6,0,...] }`，~200 Hz，数值匹配 talker。

### Tactile 端到端
- Cora talker 在 `rt/tactile/left/points` 上发 1024 点 `PointCloud2`，x/y/z/fx/fy/fz 全 FLOAT32。
- `bus-tap` 订阅 `tactile_left/tactile/samples/tactile_point_cloud2`，看到 `SensorFrameHeader { ndim: 2, shape: [1024, 6, ...] }`，payload 与源一致。
- `cargo run -p rollio -- collect --config config/config.example.toml --max-episodes 1` 写出的 Parquet 列 `observation.sensor.tactile.tactile_point_cloud2` shape `[1024, 6]`。

### Gripper 端到端
- Cora talker 在 `rt/gripper/right/state` 上发 `JointState`，`name = ["gripper_right_finger"]`、`position = [0.42]`、`velocity = [0.0]`、`effort = [1.5]`。
- `bus-tap` 订阅 `gripper_right/gripper/states/joint_position` 看到 `JointVector15 { [0]=0.42, [1..15]=0.0 }`；其他 publish_state 同理。
- `rollio collect` 写出 Parquet 列 `observation.state.gripper.joint_position` shape `[15]`，行 0 为 0.42、其余 0。

### 生命周期
- 对每个 device 二进制 SIGINT，进程 < 1s 内退出（destroy → CallbackExecutor::stop → DDSParticipant::shutdown）。
- controller 在 `control/events` 广播 `ControlEvent::Shutdown` 触发同样的路径。

## 关键改动文件清单

**每个 device 新增同样布局**（替换 `<imu_cora>` / `<tactile_cora>` / `<gripper_cora>` 与上级目录 `sensors/` / `robots/`）：

- `sensors/imu_cora/Cargo.toml`
- `sensors/imu_cora/build.rs`
- `sensors/imu_cora/cpp/CMakeLists.txt`
- `sensors/imu_cora/cpp/include/cora_bridge.h`
- `sensors/imu_cora/cpp/src/{bridge.cpp,subscriber.cpp}`
- `sensors/imu_cora/src/{main,config,cora,probe,query,validate,run}.rs`
- `sensors/imu_cora/README.md`
- （同理 `sensors/tactile_cora/`、`robots/gripper_cora/`）

**修改**
- workspace `Cargo.toml`（追加三个 member）
- `config/config.example.toml`（追加三段 `[[devices]]`）

## 复用的现有工具（不重写）

- `rollio-bus::channel_state_service_name` / `channel_sample_service_name` —— iceoryx2 服务名解析。
- `rollio-bus` 的 ring buffer / publisher 常量（`SAMPLE_BUFFER`、`STATE_BUFFER`）。
- `rollio-types::messages::{SensorFrameHeader, JointVector15, SensorDType}` —— 线协议信封。
- `rollio-types::config::{DeviceChannelConfigV2, ChannelStateKind, SensorStateKind, RobotStateKind, ResolvedSensorChannel}` —— 配置 + 解析。
- `robots/pseudo/src/bin/device.rs`（`run_sensor_channel` / robot 通道 publisher loop）—— CLI 子命令结构与 publish loop 样板。
- `examples/cora_sdk/cora_x86_64/examples/cpp/{listener_node.cpp,talker_node.cpp,CMakeLists.txt,framework_config.json}` —— Cora SDK 用法样板（仅参考，构建链路不依赖）。
- `examples/cora_sdk/cora_x86_64/include/cora/{channel.h,framework.h,dds/dds_participant.h,dds/callback_executor.h,dds/dds_qos.h}` —— SDK API 表面（仅参考）。

## 实施时待定问题

- **打包时的运行时 RPATH**：开发期 `build.rs` 通过绝对 `-Wl,-rpath` 让二进制找到 SDK 库；落到 deb / image 打包时 rpath 要相对安装位置。PR-A 在 imu_cora README 里写明选项：(a) 烘焙 `$ORIGIN/../lib/cora` 并由打包步骤把 SDK 库一起拷过去；(b) SDK 装到系统路径 + `LD_LIBRARY_PATH`。后续 device 直接复用。
- **PointCloud2 datatype 扩展**：实际 LiDAR 的 `intensity` 有时是 `UINT16` 不是 `FLOAT32`。如果阻塞落地，在 `sensors/tactile_cora/src/run.rs` / 转换器加 datatype cast 路径，作为 Phase 1.5 小补丁。
- **断线重连 / 错误恢复**：Phase 1 出错时记录日志并退出该通道。Phase 2 再决定是自动重连还是把失败通过 `DeviceChannelMode::Disabled` info 显式上报给 controller。
- **DDS QoS 颗粒度**：当前只 `reliable` / `best_effort` 两种。若部署需要 history depth / durability 这类细 QoS，把 `cora_qos` 提升为带显式字段的结构体。
- **多关节臂 device**：本计划不做。若后续需要 `rollio-device-arm-cora`，复制 gripper_cora，把 `joint_name: String` 换成 `joint_order: Vec<String>` 并放宽 dof 校验到 ≤ 15。
- **bridge.cpp drift**：三个 device 的 DDS lifecycle C++ 几乎相同。如果发现一处 bug 改三处，是 per-device 模式的代价；3 处尚可接受，> 5 处时再考虑抽公共 lib。
