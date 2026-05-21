## 新增需求

### 需求：DoorAck 应答结构体与 DoorResult 枚举
`rollio-types` 应定义 `#[repr(C)]` 枚举 `DoorResult`，含变体 `Empty`、`Float(f64)`、`Bool(bool)`。并定义 `DoorAck { request_id: u64, status: i32, error_msg: [u8; 256], result: DoorResult }` 实现 `ZeroCopySend`。`status == 0` 表示成功；非零表示错误，错误描述写入 `error_msg`。

#### 场景：读门角度返回 Float
- **WHEN** 工站执行 `DoorAction::GetDoorAngle` 且门角度为 0.82 rad
- **THEN** 工站发布 `DoorAck { request_id: <匹配请求>, status: 0, result: DoorResult::Float(0.82), error_msg: <空> }`

#### 场景：DoorOpenStatus 返回 Bool
- **WHEN** 工站执行 `DoorAction::DoorOpenStatus` 且门当前为打开状态
- **THEN** 工站发布 `DoorAck { status: 0, result: DoorResult::Bool(true) }`

#### 场景：写操作返回 Empty
- **WHEN** 工站执行 `DoorAction::OpenDoorAngle { pos: 1.57 }` 并成功完成
- **THEN** 工站发布 `DoorAck { status: 0, result: DoorResult::Empty }`

#### 场景：错误情况携带 error_msg
- **WHEN** 工站执行 `DoorAction::GetDoorAngle` 但 Modbus 通讯超时
- **THEN** 工站发布 `DoorAck { status: <非零>, result: DoorResult::Empty, error_msg: <UTF-8 编码的错误描述> }`

### 需求：SwitchAck 应答结构体与 SwitchResult 枚举
`rollio-types` 应定义 `#[repr(C)]` 枚举 `SwitchResult`，含变体 `Empty`、`On(bool)`。并定义 `SwitchAck { request_id: u64, status: i32, error_msg: [u8; 256], result: SwitchResult }` 实现 `ZeroCopySend`。

#### 场景：SwitchStatus 返回开关状态
- **WHEN** 工站执行 `SwitchAction::SwitchStatus { wiring_number: 3 }` 且该路开关闭合
- **THEN** 工站发布 `SwitchAck { status: 0, result: SwitchResult::On(true) }`

### 需求：SixdofAck 应答结构体与 SixdofResult 枚举
`rollio-types` 应定义 `#[repr(C)]` 枚举 `SixdofResult`，含变体 `Empty`、`Pose([f64; 6])`。并定义 `SixdofAck { request_id: u64, status: i32, error_msg: [u8; 256], result: SixdofResult }` 实现 `ZeroCopySend`。`Pose` 数组顺序为 `[roll, pitch, yaw, x, y, z]`。

#### 场景：GetPose 返回六维度
- **WHEN** 工站执行 `SixdofAction::GetPose` 且当前姿态为 `[0.1, 0.2, 0.3, 100.0, 200.0, 300.0]`
- **THEN** 工站发布 `SixdofAck { status: 0, result: SixdofResult::Pose([0.1, 0.2, 0.3, 100.0, 200.0, 300.0]) }`

#### 场景：SetPose 完成返回 Empty
- **WHEN** 工站执行 `SixdofAction::SetPose { … }` 并完成运动
- **THEN** 工站发布 `SixdofAck { status: 0, result: SixdofResult::Empty }`

### 需求：DoorHandleCameraAck 应答结构体与 DoorHandleCameraResult 枚举
`rollio-types` 应定义 `#[repr(C)]` 枚举 `DoorHandleCameraResult`，含变体 `Empty`、`AngleRad(f64)`、`NotDetected`。并定义 `DoorHandleCameraAck { request_id: u64, status: i32, error_msg: [u8; 256], result: DoorHandleCameraResult }` 实现 `ZeroCopySend`。`NotDetected` 是合法瞬态状态（`status=0`），与错误明确区分。

#### 场景：成功检测到把手返回 AngleRad
- **WHEN** 工站执行 `DoorHandleCameraAction::GetDoorHandleAngle` 且 ArUco marker 被识别，角度为 0.42 rad
- **THEN** 工站发布 `DoorHandleCameraAck { status: 0, result: DoorHandleCameraResult::AngleRad(0.42) }`

#### 场景：NotDetected 不是错误
- **WHEN** 工站执行 `DoorHandleCameraAction::GetDoorHandleAngle` 但本帧 ArUco 未识别
- **THEN** 工站发布 `DoorHandleCameraAck { status: 0, result: DoorHandleCameraResult::NotDetected, error_msg: <空> }`

#### 场景：相机硬件错误携带非零 status
- **WHEN** 工站执行 `DoorHandleCameraAction::GetDoorHandleAngle` 但相机抓帧失败（V4L2 错误）
- **THEN** 工站发布 `DoorHandleCameraAck { status: <非零>, result: DoorHandleCameraResult::Empty, error_msg: <UTF-8 错误描述> }`

### 需求：所有应答携带 request_id 用于匹配
四类应答结构体均含 `request_id: u64` 字段，工站必须将其设置为对应命令的 `request_id`，使调用方可在收到 ack 后匹配回原请求。

#### 场景：request_id 在命令与应答之间保持一致
- **WHEN** 调用方发布 `DoorCommand { request_id: 7777, action: DoorAction::GetDoorAngle }`
- **THEN** 工站回传的 `DoorAck` 中 `request_id == 7777`
