## 新增需求

### 需求：设备命令/应答服务名助手函数
`rollio-bus/src/lib.rs` 应导出两个助手函数：`station_device_command_service_name(device_id: &str) -> String` 返回 `"station/devices/{device_id}/command"`；`station_device_ack_service_name(device_id: &str) -> String` 返回 `"station/devices/{device_id}/ack"`。

#### 场景：命令服务名助手函数返回正确字符串
- **WHEN** 调用 `station_device_command_service_name("door_ctrl")`
- **THEN** 返回 `"station/devices/door_ctrl/command"`

#### 场景：应答服务名助手函数
- **WHEN** 调用 `station_device_ack_service_name("sixdof_ctrl")`
- **THEN** 返回 `"station/devices/sixdof_ctrl/ack"`

### 需求：设备命令/应答 topic 容量常量
`rollio-bus/src/lib.rs` 应导出以下容量常量，供所有打开设备命令/应答 topic 的参与方使用：

| 常量 | 值 | 对应服务 |
|------|---|---------|
| `STATION_DEVICE_COMMAND_HISTORY_SIZE` | 0 | station/devices/*/command |
| `STATION_DEVICE_COMMAND_BUFFER` | 32 | station/devices/*/command |
| `STATION_DEVICE_ACK_HISTORY_SIZE` | 16 | station/devices/*/ack |
| `STATION_DEVICE_ACK_BUFFER` | 64 | station/devices/*/ack |

所有设备 topic 的 `max_publishers`、`max_subscribers`、`max_nodes` 均应使用现有 `STATE_MAX_PUBLISHERS`、`STATE_MAX_SUBSCRIBERS`、`STATE_MAX_NODES` 常量（均为 16）。

#### 场景：命令 topic 的 history_size 为零
- **WHEN** 工站重启并重新订阅某设备命令 topic
- **THEN** 重启前的陈旧命令不会被重放（history_size = 0）

#### 场景：应答 topic 保留 16 条历史
- **WHEN** 调用方在工站发布 ack 之后才订阅 ack topic
- **THEN** 订阅方通过历史回放最多可收到 16 条历史 ack
