## 背景

Rollio 使用 iceoryx2（POSIX 共享内存 IPC）作为内部总线。automated-data-station 是运行在同一台机器上的长驻 Python 服务，负责控制所有物理自动化设备并驱动数据采集 FSM。当前它在 IPC 上没有任何存在：episode 生命周期通过已废弃的 HTTP 服务（`data_capture`）协调，设备参数从未进入 mcap metadata。

iceoryx2 0.8.1 以 PyPI wheel 形式可用（已在 amd64 验证）。所有参与方运行在同一台机器上——POSIX 共享内存约束得到满足。

> 工站设备的类型化命令/应答（rollio Rust 侧→工站 Python 侧）已拆出独立 craft `rollio-device`，本文档仅涵盖 episode 生命周期与 metadata 路径的基础设施。

## 目标 / 非目标

**目标：**
- 工站可以发布 `EpisodeCommand` 来驱动 rollio episode 状态机
- 工站可以订阅 `ControlEvent` 以追踪当前活跃的 episode index
- 工站为每个设备参数广播 `EpisodeMetadataEntry`；episode-mcap 累积后按 source 分组写入 mcap metadata 块
- 新增的 metadata 服务在 `rollio-bus` 中以显式容量常量声明

**非目标：**
- 工站不采纳 `rollio-device-*` 二进制契约（probe/validate/query/run 子命令）
- 不做工站设备的高频状态流（如门角度 250 Hz 等）
- 不做 dataloop 内存直传（C FFI SDK 只接受文件路径；Keep → 写文件 → 上传仍是唯一路径）
- IPC 服务不做 ACL / 发布方白名单（同机器、同信任域）
- 不在本 craft 中定义按设备命令/应答类型（见 `rollio-device` craft）

## 设计决策

### D1：IPC 邻居模式（A2）而非完整的 device-as-binaries 契约（A1）

device-as-binaries 契约要求每个设备是一个独立的长驻进程，带有 4 个 CLI 子命令（probe/validate/query/run）和固定的总线拓扑。工站的设备与其 FSM 紧耦合，共享单一 ModbusCtrl；拆成多个独立二进制文件需要大量重写，而在当前部署场景（同机器、单一工站进程）下毫无功能收益。

**备选方案**：A1（完整契约）。已拒绝——工站需要变成多个二进制文件并将生命周期控制权交给 rollio。

### D2：录制中发送 metadata，entry 携带显式 episode_index

工站等到收到 `ControlEvent::RecordingStart(episode_index)` 后再广播 `EpisodeMetadataEntry` 条目。每个条目携带 episode_index 字段。这避免了 encoder 侧需要"pending bucket"累加器——条目自己路由到正确的 episode，encoder 重启后也能简单处理。

**备选方案**：在 `EpisodeCommand::Start` 之前广播（β 模式）。已拒绝——encoder 需要有状态的 pending 缓冲区；条目在 Start 被处理之前没有 episode 归属。

### D3：按 `source` 分组写入 mcap，对齐 `write_metadata(name, BTreeMap)` 现有契约

`McapEpisodeWriter::write_metadata(name, map)` 已经被现有 `writer.write_metadata("episode", meta)` 调用使用。新条目按 `entry.source` 分组（如 `"station"`、`"lock_manager"`），每个 source 调用一次该方法。结果：mcap 中出现多个 metadata record，命名清晰，现有 `"episode"` record 内容不变。

**备选方案**：把所有条目合并到 `"episode"` record 内。已拒绝——会和固定的四个 episode key 混杂，且需要 key 命名空间防冲突。

### D4：设备访问统一经过 device_lifecycle 单例

工站所有设备访问（HTTP `DeviceApiHandler`、IpcBridge 命令派发、内部 FSM）统一通过 `device_lifecycle.execute(device_id, action)` 入口。per-device `threading.Lock` 池由 `device_lifecycle` 单例持有，作为入口的内部细节。这避免了把同步原语提升到 `AppContainer`，并强制 HTTP 不直接 touch device 对象。

**备选方案**：把 lock pool 提升到 `AppContainer` 由 HTTP 和 IPC 各自取用。已拒绝——同步原语应与其守护的资源一起封装；HTTP 直接调 device 是历史遗留，要修 HTTP 而不是把锁外移。

### D5：data_recorder 作为 episode 生命周期边界

`data_recorder.start_collection()`、`stop_and_upload()` 和 `stop_and_delete()` 是插入 IPC episode 命令的天然位置。IpcPublisher 在构造时注入。内部映射：

```
start_collection()       → pub EpisodeCommand::Start
stop_and_upload(...)     → pub EpisodeMetadataEntry × N → pub Stop → pub Keep
stop_and_delete()        → pub Stop → pub Discard
```

Keep/Discard 语义：controller 将状态从 `Pending → Idle`；episode-mcap/lerobot 落盘（Keep）或丢弃（Discard）缓冲数据。

## 风险与权衡

**[竞态：metadata 早于 RecordingStart]** → 工站发出 Start 后等待 `ControlEvent::RecordingStart` 再广播 metadata。若 IPC 总线拥堵导致 RecordingStart 延迟，metadata 广播也同等延迟。正常情况下本地 POSIX shm 延迟在微秒级；风险可忽略。

**[control/events 的订阅缓冲区 = 默认值 2]** → IpcBridge poll 线程绝不能阻塞；它将 device.execute() 分派给 ThreadPoolExecutor。若 poll 线程阻塞（如 bug），ControlEvent 样本会在缓冲区只有 2 的情况下静默丢失。缓解措施：严格规定 poll 线程不做任何 I/O 或设备操作。

**[现有服务的容量配置不匹配]** → 工站必须以与 controller 规范声明完全一致的容量参数打开 `control/events` 和 `control/episode-command`（max_pub=4/4，max_sub=32/8，max_nodes=32/8）。不匹配时 open 会失败且错误信息不直观。缓解措施：常量必须硬编码以匹配 `controller/src/collect.rs`；后续清理任务应将其提升到 rollio-bus。

**[Python ctypes 镜像漂移]** → Python IpcBridge 必须逐字节镜像 `#[repr(C)]` Rust 类型。若 rollio-types 的类型发生变更，station 的 ctypes 镜像会静默错误解析数据。缓解措施：ctypes 镜像集中放在 `ipc/types.py`，每个类带 `type_name()` classmethod；添加对每种类型做收发的冒烟测试。

## 迁移计划

1. 落地 rollio-bus + rollio-types 变更（纯增量，不触碰现有代码）
2. 落地 episode-mcap 累加器（订阅方启动，暂无条目到来——安全）
3. 以 IpcBridge 禁用状态部署工站（feature flag 或环境变量 `ROLLIO_IPC_ENABLED=0`）
4. 启用 IpcBridge；运行冒烟测试（`/tmp/iox2-smoke/test.py` 模式）
5. 从 `data_recorder` 中移除已废弃的 `data_capture` HTTP 调用
6. 移除 `ROLLIO_IPC_ENABLED` flag

## 待解事项

- `control/events` 和 `control/episode-command` 的容量配置是否应提升为 rollio-bus 常量（清理任务）？建议是，但不在本次变更范围内。
- aarch64（开发机）wheel 验证待完成——station 部署前需确认 `iceoryx2==0.8.1` 覆盖 aarch64。
