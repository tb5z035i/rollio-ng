## 1. rollio-bus：服务常量与容量配置

- [x] 1.1 在 `rollio-bus/src/lib.rs` 中添加 `EPISODE_METADATA_ENTRIES_SERVICE` 字符串常量
- [x] 1.2 添加 `EPISODE_METADATA_HISTORY_SIZE = 64` 和 `EPISODE_METADATA_BUFFER = 256` 常量

## 2. rollio-types：EpisodeMetadataEntry

- [x] 2.1 添加 `EpisodeMetadataEntry` `#[repr(C)]` 结构体，字段：`episode_index: u32`、`source: [u8; 32]`、`key: [u8; 64]`、`value: [u8; 256]`、`timestamp_us: u64`
- [x] 2.2 为 `EpisodeMetadataEntry` 派生或实现 `ZeroCopySend`
- [x] 2.3 按现有消息规范添加 `type_name()` / 类型注册样板代码

## 3. episode-mcap：EpisodeMetadataEntry 累加器

- [x] 3.1 在 `episode-mcap/src/runtime.rs` 启动时添加 `EpisodeMetadataEntry` 订阅方（使用 `EPISODE_METADATA_HISTORY_SIZE` + `EPISODE_METADATA_BUFFER` 开启，早于任何 episode 开始）
- [x] 3.2 在运行时状态中添加按 episode 的累加器（`HashMap<u32, Vec<EpisodeMetadataEntry>>`）
- [x] 3.3 在运行循环中轮询累加器订阅方；将收到的条目追加到对应 episode 的桶中；对未知 episode_index 记录警告并丢弃
- [x] 3.4 收到 `ControlEvent::EpisodeKeep { episode_index }` 时：按 `source` 字段分组，为每个 source 调用一次 `writer.write_metadata(source, BTreeMap)`，与现有 `write_metadata("episode", meta)` 并列；清空对应桶
- [x] 3.5 收到 `ControlEvent::EpisodeDiscard { episode_index }` 时：清空对应桶，不写入任何内容
- [x] 3.6 添加运行时测试：发布两条 `EpisodeMetadataEntry`，发送 Keep，验证它们出现在 mcap metadata 块中

## 4. 清理任务（stretch）

- [x] 4.1 将 `control/events` 容量配置（max_pub=4, max_sub=32, max_nodes=32）提升为 rollio-bus 常量，并更新 `controller/src/collect.rs` 以引用它们
- [x] 4.2 同样将 `control/episode-command` 容量配置（max_pub=4, max_sub=8, max_nodes=8）提升为 rollio-bus 常量
