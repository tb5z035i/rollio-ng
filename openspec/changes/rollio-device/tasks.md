## 1. rollio-bus：设备 topic 配置

- [ ] 1.1 在 `rollio-bus/src/lib.rs` 中添加 `station_device_command_service_name(device_id)` 助手函数
- [ ] 1.2 添加 `station_device_ack_service_name(device_id)` 助手函数
- [ ] 1.3 添加 `STATION_DEVICE_COMMAND_HISTORY_SIZE = 0` 和 `STATION_DEVICE_COMMAND_BUFFER = 32` 常量
- [ ] 1.4 添加 `STATION_DEVICE_ACK_HISTORY_SIZE = 16` 和 `STATION_DEVICE_ACK_BUFFER = 64` 常量
- [ ] 1.5 为两个服务名助手函数添加单元测试

## 2. rollio-types：按设备命令类型

- [ ] 2.1 添加 `DoorAction` `#[repr(C)]` 枚举，含全部 12 个变体（写操作 + 读操作）
- [ ] 2.2 添加 `DoorCommand { request_id: u64, action: DoorAction }` 并实现 `ZeroCopySend`
- [ ] 2.3 添加 `SwitchAction` 枚举（`SwitchStatus { wiring_number: u8 }`）和 `SwitchCommand`
- [ ] 2.4 添加 `SixdofAction` 枚举（`SetPose`、`SetOffset`、`GetPose`）和 `SixdofCommand`
- [ ] 2.5 添加 `DoorHandleCameraAction` 枚举（`ResetHandleReference`、`GetDoorHandleAngle`）和 `DoorHandleCameraCommand`
- [ ] 2.6 为全部四个命令类型实现 `ZeroCopySend` 并添加 `type_name()` 注册样板

## 3. rollio-types：按设备应答类型

- [ ] 3.1 添加 `DoorResult` `#[repr(C)]` 枚举（`Empty`、`Float(f64)`、`Bool(bool)`）和 `DoorAck`
- [ ] 3.2 添加 `SwitchResult`（`Empty`、`On(bool)`）和 `SwitchAck`
- [ ] 3.3 添加 `SixdofResult`（`Empty`、`Pose([f64; 6])`）和 `SixdofAck`
- [ ] 3.4 添加 `DoorHandleCameraResult`（`Empty`、`AngleRad(f64)`、`NotDetected`）和 `DoorHandleCameraAck`
- [ ] 3.5 为全部四个应答类型实现 `ZeroCopySend` 并添加 `type_name()` 注册样板

## 4. 静态断言与冒烟测试

- [ ] 4.1 为每个 `#[repr(C)]` 命令/应答类型添加 `static_assertions::assert_eq_size!` 验证（防止 ctypes 镜像意外漂移）
- [ ] 4.2 添加 round-trip 单元测试：构造每种命令/应答，序列化为字节后反序列化，断言字段一致
