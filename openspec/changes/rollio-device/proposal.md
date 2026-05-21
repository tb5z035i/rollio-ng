## Why

Rollio 内部需要通过 IPC 总线向自动化工站的物理设备（门、开关、六维台、门把手相机）发送类型化命令并获取应答。当前没有任何 Rust 侧调用方能驱动工站设备，设备命令消息类型和总线配置也尚未定义。

## What Changes

- 在 `rollio-types` 中为四类设备各定义一套 `#[repr(C)]` 命令结构体（`DoorCommand`、`SwitchCommand`、`SixdofCommand`、`DoorHandleCameraCommand`）及对应动作枚举
- 在 `rollio-types` 中为四类设备各定义一套 `#[repr(C)]` 应答结构体（`DoorAck`、`SwitchAck`、`SixdofAck`、`DoorHandleCameraAck`）及结果枚举，`status=0` 表示成功
- 所有类型实现 `ZeroCopySend`，可直接发布在 iceoryx2 共享内存 topic 上
- 在 `rollio-bus` 中添加设备命令/应答 topic 的服务名助手函数及容量常量（command: history=0, buffer=32；ack: history=16, buffer=64）

## Capabilities

### New Capabilities

- `station-device-command-types`：四类设备的 `#[repr(C)]` 命令结构体与动作枚举（Rust 侧发布方使用）
- `station-device-ack-types`：四类设备的 `#[repr(C)]` 应答结构体与结果枚举（工站发布，Rust 侧订阅方使用）
- `station-device-bus-config`：设备命令/应答 topic 的服务名函数与容量常量（在 `rollio-bus` 中声明）

### Modified Capabilities

## Impact

- `rollio-types/src/messages.rs`：新增 ~150 行类型定义
- `rollio-bus/src/lib.rs`：新增 2 个助手函数和 6 个容量常量（与 `station-ipc-integration` 在同一文件中，各自独立追加）
- 工站 Python 侧需在 `ipc/types.py` 中以 `ctypes` 结构体镜像全部新类型（在工站仓库中独立跟踪）
- 无现有接口破坏性变更
