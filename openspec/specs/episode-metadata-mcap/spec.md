## 新增需求

### 需求：episode-mcap 订阅 EpisodeMetadataEntry 流
`episode-mcap` 运行时应在启动时、任何 episode 开始之前，以 `history_size = EPISODE_METADATA_HISTORY_SIZE` 和 `subscriber_max_buffer_size = EPISODE_METADATA_BUFFER` 打开 `EPISODE_METADATA_ENTRIES_SERVICE` 作为订阅方。

#### 场景：订阅方在首个 episode 前就绪
- **当** episode-mcap 启动且尚未有 episode 开始
- **则** 订阅方已打开，可以接收工站为第一个 episode 发布的条目

### 需求：按 episode 累积条目
episode-mcap 应维护一个按 episode 划分的累加器（`HashMap<u32, Vec<EpisodeMetadataEntry>>`）。每收到一条 `EpisodeMetadataEntry`，应将其追加到对应 `episode_index` 的桶中。`episode_index` 与任何已知 episode（活跃或最近停止）都不匹配的条目，应静默丢弃并记录警告日志。

#### 场景：条目路由到正确的 episode
- **当** 工站发布 `EpisodeMetadataEntry { episode_index: 7, key: "door.angle", value: "0.82" }`
- **则** 该条目被追加到累加器的第 7 号桶中

#### 场景：陈旧条目被丢弃
- **当** 一条 `episode_index: 3` 的条目在 episode 3 已经落盘后才到达
- **则** 该条目被丢弃并记录警告；已有的 mcap 文件不受影响

### 需求：EpisodeKeep 时将 metadata 条目写入 mcap
收到 `ControlEvent::EpisodeKeep { episode_index }` 时，episode-mcap 应将该 episode 累积的所有 `EpisodeMetadataEntry` 条目按 `source` 字段分组，为每个 source 调用一次 `writer.write_metadata(source, BTreeMap<String, String>)`，其中 map 包含该 source 所有条目的 `(key, value)` 对。

写入格式与现有 `write_metadata("episode", meta)` 调用对齐（同为 `McapEpisodeWriter::write_metadata`），产生若干额外的 MCAP metadata record，每个 record 的 `name` 等于 `entry.source`（如 `"station"`）。现有 `"episode"` record 内容不变。

```
// 现有（不变）
writer.write_metadata("episode", {
    "episode_index": "42",
    "start_time_us": "...",
    "stop_time_us":  "...",
    "config_toml":   "...",
})?;

// 新增（按 source 分组，每个 source 一次调用）
writer.write_metadata("station", {
    "door.angle":    "0.82",
    "sixdof.pose.x": "100.0",
    ...
})?;
```

`entry.key` 和 `entry.value` 以 UTF-8 解码至首个 null 字节后截断，作为 `BTreeMap` 键值插入。写入完成后清空对应累加器桶。

#### 场景：工站 metadata 出现在落盘的 mcap 中，与 episode record 并列
- **当** 工站为 episode 7 广播 `source="station", key="door.angle", value="0.82"` 和 `key="sixdof.pose.x", value="100.0"`，然后发送 Stop + Keep
- **则** episode 7 落盘的 mcap 包含 `name="episode"` record（四个标准 key 不变）以及 `name="station"` record（包含 `door.angle` 和 `sixdof.pose.x`）

#### 场景：多个 source 产生多个 metadata record
- **当** 累积条目来自 `source="station"` 和 `source="lock_manager"` 两个来源
- **则** 落盘的 mcap 包含两个额外 record：`name="station"` 和 `name="lock_manager"`，各自独立

#### 场景：Keep 后累加器清空
- **当** episode 7 通过 Keep 落盘
- **则** episode 7 的累加器为空；后续到达的 episode 7 条目被丢弃并记录警告

### 需求：Discard 时清空累加器但不写入
收到 `ControlEvent::EpisodeDiscard { episode_index }` 时，episode-mcap 应清空该 episode 的累加器桶，不向磁盘写入任何 metadata 条目。

#### 场景：被丢弃的 episode 在磁盘上不留 metadata
- **当** 工站对 episode 8 发送 Stop + Discard
- **则** episode 8 累积的所有 metadata 条目被丢弃；不为 episode 8 写入任何 mcap 文件
