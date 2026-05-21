## 新增需求

### 需求：metadata 服务名在 rollio-bus 中声明
`rollio-bus/src/lib.rs` 应导出字符串常量 `EPISODE_METADATA_ENTRIES_SERVICE`，值为 `"episode/metadata/entries"`。

#### 场景：episode-mcap 与工站使用相同服务名
- **WHEN** episode-mcap 和工站均通过 `EPISODE_METADATA_ENTRIES_SERVICE` 常量打开服务
- **THEN** 两侧解析到的服务名一致，能在同一 topic 上完成 pub/sub

### 需求：metadata 服务容量常量在 rollio-bus 中声明
`rollio-bus/src/lib.rs` 应导出以下容量常量，供所有打开 `episode/metadata/entries` 服务的参与方使用：

| 常量 | 值 | 对应服务 |
|------|---|---------|
| `EPISODE_METADATA_HISTORY_SIZE` | 64 | episode/metadata/entries |
| `EPISODE_METADATA_BUFFER` | 256 | episode/metadata/entries |

该服务的 `max_publishers`、`max_subscribers`、`max_nodes` 应使用现有 `STATE_MAX_PUBLISHERS`、`STATE_MAX_SUBSCRIBERS`、`STATE_MAX_NODES` 常量（均为 16）。

#### 场景：Metadata 缓冲区支持晚加入的 encoder
- **WHEN** episode-mcap 在工站已发布条目之后才订阅 `episode/metadata/entries`
- **THEN** 订阅方通过历史回放最多收到 64 条历史条目
