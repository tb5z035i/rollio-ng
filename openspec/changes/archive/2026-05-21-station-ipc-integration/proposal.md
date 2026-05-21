## 为什么

automated-data-station（Python）负责驱动数据采集流程，但它在 rollio iceoryx2 总线上没有任何存在——无法发出 episode 生命周期信号，也无法将设备参数广播到 mcap metadata。以 IPC 邻居模式接入可以填补这些空白，且无需让工站采纳完整的 `rollio-device-*` 二进制契约。

> 注：rollio 内部对工站设备的类型化命令/应答（DoorCommand/Ack 等）已拆分为独立 craft `rollio-device`，与本 craft 并行推进。

## 变更内容

- **rollio-bus**：添加 `episode/metadata/entries` 服务名常量和容量常量（`EPISODE_METADATA_HISTORY_SIZE=64`、`EPISODE_METADATA_BUFFER=256`）
- **rollio-types**：添加 `EpisodeMetadataEntry` `#[repr(C)]` 结构体，含 `episode_index`、`source`、`key`、`value`、`timestamp_us` 字段
- **episode-mcap**：订阅 `episode/metadata/entries`，按 episode 累积条目，在 `ControlEvent::EpisodeKeep` 时按 `source` 字段分组写入 mcap metadata 块
- **station**（独立仓库）：添加 `IpcBridge` + `IpcPublisher`；接入 `AppContainer`、`data_recorder` 和 `device_controller` 以发布 EpisodeCommand 与 metadata 条目

## 能力

### 新增能力

- `ipc-bus-constants`：在 `rollio-bus` 中声明 `episode/metadata/entries` 服务名常量与容量常量
- `station-ipc-message-types`：在 `rollio-types` 中声明 `EpisodeMetadataEntry` `#[repr(C)]` 消息结构体
- `episode-metadata-mcap`：episode-mcap 按 episode 累积 `EpisodeMetadataEntry` 消息，并在落盘时按 `source` 分组写入 mcap metadata 块

### 修改能力

## 影响

- **rollio-bus/src/lib.rs**：1 个新服务名常量、2 个新容量常量
- **rollio-types/src/messages.rs**：1 个新的 `#[repr(C)]` 类型；不修改现有类型
- **episode-mcap/src/runtime.rs**：新增订阅方 + 累加器；现有 metadata 写入路径按 `source` 分组扩展（与现有 `write_metadata("episode", meta)` 调用并列）
- **station 仓库**（`automated-data-station`）：`IpcBridge`、`IpcPublisher`、`AppContainer`/`data_recorder`/`device_controller` 接线变更——在 station 仓库单独追踪
- **iceoryx2 依赖**：station 新增 `iceoryx2==0.8.1` PyPI 依赖；rollio-ng 已依赖 iceoryx2（Rust 侧）
- 不修改任何现有 rollio 消息类型、服务契约或总线容量配置
