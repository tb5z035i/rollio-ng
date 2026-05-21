## 新增需求

### 需求：DoorCommand 命令结构体与 DoorAction 枚举
`rollio-types` 应定义 `#[repr(C)]` 枚举 `DoorAction`，含以下 12 个变体：`OpenDoorAngle { pos: f64 }`、`SetDoorTorque { torque: f64 }`、`MoveDoorLockPosition { pos: f64 }`、`OpenLockPlate`、`SetMagneticAttractionForce { sign: bool }`、`ChangeLock`、`DoorlockHome`、`Sleep { ms: u32 }`、`DoorOpenStatus`、`DoorCloseStatus`、`GetDoorAngle`、`GetDoorLockPosition`。并定义 `DoorCommand { request_id: u64, action: DoorAction }` 实现 `ZeroCopySend`。

#### 场景：DoorCommand 携带 request_id 与动作
- **WHEN** 调用方构造 `DoorCommand { request_id: 1234, action: DoorAction::GetDoorAngle }`
- **THEN** 结构体的 `request_id` 字段等于 1234，`action` 判别为 `GetDoorAngle`

#### 场景：带参数动作编码正确
- **WHEN** 调用方构造 `DoorAction::OpenDoorAngle { pos: 1.57 }`
- **THEN** 枚举变体判别正确，`pos` 字段读回 1.57

### 需求：SwitchCommand 命令结构体与 SwitchAction 枚举
`rollio-types` 应定义 `#[repr(C)]` 枚举 `SwitchAction`，含变体 `SwitchStatus { wiring_number: u8 }`。并定义 `SwitchCommand { request_id: u64, action: SwitchAction }` 实现 `ZeroCopySend`。

#### 场景：SwitchCommand 编码 wiring_number
- **WHEN** 调用方构造 `SwitchCommand { request_id: 99, action: SwitchAction::SwitchStatus { wiring_number: 3 } }`
- **THEN** `wiring_number` 字段读回为 3

### 需求：SixdofCommand 命令结构体与 SixdofAction 枚举
`rollio-types` 应定义 `#[repr(C)]` 枚举 `SixdofAction`，含变体：`SetPose { roll: f64, pitch: f64, yaw: f64, x: f64, y: f64, z: f64, time_ms: u32 }`、`SetOffset { roll: f64, pitch: f64, yaw: f64, x: f64, y: f64, z: f64, time_ms: u32 }`、`GetPose`。并定义 `SixdofCommand { request_id: u64, action: SixdofAction }` 实现 `ZeroCopySend`。

#### 场景：SetPose 编码六维度参数
- **WHEN** 调用方构造 `SixdofAction::SetPose { roll: 0.1, pitch: 0.2, yaw: 0.3, x: 100.0, y: 200.0, z: 300.0, time_ms: 500 }`
- **THEN** 所有字段读回值与输入一致

### 需求：DoorHandleCameraCommand 命令结构体与动作枚举
`rollio-types` 应定义 `#[repr(C)]` 枚举 `DoorHandleCameraAction`，含变体 `ResetHandleReference` 和 `GetDoorHandleAngle`。并定义 `DoorHandleCameraCommand { request_id: u64, action: DoorHandleCameraAction }` 实现 `ZeroCopySend`。

#### 场景：DoorHandleCameraCommand 编码动作变体
- **WHEN** 调用方构造 `DoorHandleCameraCommand { request_id: 7, action: DoorHandleCameraAction::GetDoorHandleAngle }`
- **THEN** 动作判别为 `GetDoorHandleAngle`，`request_id` 为 7
