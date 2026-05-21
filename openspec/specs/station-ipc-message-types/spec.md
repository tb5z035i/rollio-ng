## 新增需求

### 需求：EpisodeMetadataEntry 消息类型
`rollio-types` 应定义 `#[repr(C)]` 结构体 `EpisodeMetadataEntry`，字段为：`episode_index: u32`、`source: [u8; 32]`、`key: [u8; 64]`、`value: [u8; 256]`、`timestamp_us: u64`。该结构体应实现 `ZeroCopySend`。

#### 场景：metadata 条目携带 episode 归属
- **WHEN** 工站在收到 `ControlEvent::RecordingStart { episode_index: 42 }` 后广播一条 metadata 条目
- **THEN** 该条目的 `episode_index` 字段等于 42，使 encoder 能将其路由到正确的 episode 累加器

#### 场景：source 字段标识发布方
- **WHEN** 工站以 `source="station"` 发布一条 metadata 条目
- **THEN** episode-mcap 将该条目分组到 `source="station"` 的桶中，最终写入名为 `"station"` 的 mcap metadata record

#### 场景：key/value 以 UTF-8 null-terminated 字节存储
- **WHEN** 发布方写入 `key="door.angle"`、`value="0.82"`
- **THEN** 字节数组中 `door.angle\0` 和 `0.82\0` 后续字节未定义；解码方应在首个 null 字节处截断
