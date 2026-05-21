## Context

Rollio 的 IPC 总线已有 `control/events` 和 `control/episode-command` 两条 pub/sub 通道，连接 controller 与 episode 处理组件。`station-ipc-integration` craft 正在建立工站参与 episode 生命周期的基础设施（元数据条目、bus 常量）。本 craft 在此基础上，为 Rust 侧调用方定义向工站物理设备发送命令并读取结果的类型合同。

当前状态：工站设备控制完全在 Python 侧。没有任何 Rust 组件能读取门角度、设置六维台姿态或触发锁操作。设备动作词表散落在 Python HTTP handler 中，没有跨语言合同。

## Goals / Non-Goals

**Goals:**
- 在 `rollio-types` 中定义四类设备的强类型 `#[repr(C)]` 命令与应答结构体
- 在 `rollio-bus` 中声明对应 topic 的服务名助手函数和容量常量
- 所有类型实现 `ZeroCopySend`，可在 iceoryx2 共享内存 topic 上零拷贝传输

**Non-Goals:**
- 不在本 craft 中实现任何 Rust 侧调用方逻辑（那是使用方的职责）
- 不实现工站 Python 侧的 ctypes 镜像（在工站仓库中独立跟踪）
- 不做设备高频状态流（如 250 Hz 门角度）
- 不采用 iceoryx2 RequestResponse 原语

## Decisions

### D1：pub/sub + ack 而非 iceoryx2 RequestResponse

工站命令为低频操作（每步一条）。pub/sub + ack 只需约 30 行封装代码（request_id 字典 + 超时），且 ack 可被第三方旁路订阅（诊断、日志）。iceoryx2 RequestResponse 在 Python binding 中的支持面不确定，且引入第二种原语增加整体认知负担。

**备选**：iceoryx2 RequestResponse。已拒绝——与 rollio 全栈 pub/sub 体系不一致。

### D2：按设备独立 topic 和类型（而非单一通用联合体）

`station/devices/{device_id}/command` 承载对应设备类型（`DoorCommand` 承载 `door_ctrl` topic），而不是带 `device_id` 字段的通用 `StationDeviceCommand`。好处：topic 级别类型安全，订阅者只收到自己关心的设备流量；按设备结果类型精确镜像各自不对称的结果形状。

**备选**：单一 topic + 全局动作联合体。已拒绝——失去 topic 级别类型安全。

### D3：DoorStatus 返回 `Bool(bool)`，`NotDetected` 作为独立枚举变体

`DoorOpenStatus`/`DoorCloseStatus` 操作结果为 bool（开/关状态），归入 `DoorResult::Bool(bool)`。`DoorHandleCameraResult::NotDetected` 不映射到错误码（`status=0`）——它是合法的瞬态状态（ArUco marker 当前帧未检测到），与采集失败的错误场景明确区分。

### D4：command topic history_size = 0

工站重启后不应重放陈旧命令。history_size=0 确保新建订阅方不会收到历史命令队列。ack topic history_size=16 保留足够的历史供调用方晚加入时匹配 request_id。

## Risks / Trade-offs

**[ctypes 镜像漂移]** → Python 侧 ctypes 结构体必须逐字节镜像 `#[repr(C)]` Rust 类型。Rust 类型变更会导致 Python 侧静默错误解析。缓解：ctypes 镜像集中放在 `ipc/types.py`，每个类带 `type_name()` classmethod；添加每种类型收发冒烟测试。

**[repr(C) 枚举对齐]** → `DoorAction` 等枚举含有不同大小的字段（`bool`、`f64`、`u32`），联合体大小由最大变体决定。Python ctypes 需要精确匹配 Rust 编译器的布局。缓解：在 Rust 侧添加 `static_assertions` 验证各类型 `size_of` 符合预期。

**[SixdofAction::SetOffset 字段未完整定义]** → 规范中 SetOffset 字段标注为"…（同 SetPose）"，暗示与 SetPose 字段相同。若工站实现有差异需要在实现阶段对齐。

## Migration Plan

1. 落地本 craft（纯增量，不触碰现有代码）
2. 工站侧在 `ipc/types.py` 中添加 ctypes 镜像
3. 运行每种设备的冒烟测试（发布命令 → 工站处理 → 收到 ack）
4. Rust 侧调用方按需集成

## Open Questions

- `SixdofAction::SetOffset` 字段是否与 `SetPose` 完全相同？需在实现阶段与工站工程师确认。
