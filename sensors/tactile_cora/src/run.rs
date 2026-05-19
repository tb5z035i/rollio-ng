//! Runtime: starts the Cora bridge, opens one iceoryx2 publisher per enabled
//! tactile channel (service `samples/tactile_point_cloud2` with
//! `SensorFrameHeader` user header), and forwards every `PointCloud2Sample`
//! into a per-channel `crossbeam_channel`; a dedicated publisher thread drains
//! it, projects six float32 slots per point according to `pointcloud_field_map`,
//! and emits a `SensorFrameHeader { ndim: 2, shape: [N, 6, ...] }` sample plus
//! `4 * 6 * N` payload bytes.
//!
//! Strict Phase 1 acceptance: every named field must be FLOAT32, the message
//! must be little-endian, and `width * height` must equal `tactile_point_count`.
//! Out-of-spec messages are dropped with a warn-once log line.

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iceoryx2::prelude::*;
use rollio_bus::{
    channel_mode_info_service_name, channel_sample_service_name, CONTROL_EVENTS_SERVICE,
    SAMPLE_BUFFER, SAMPLE_MAX_NODES, SAMPLE_MAX_PUBLISHERS, SAMPLE_MAX_SUBSCRIBERS,
};
use rollio_types::config::{
    BinaryDeviceConfig, DeviceChannelConfigV2, DeviceType, SensorStateKind,
};
use rollio_types::messages::{
    ControlEvent, DeviceChannelMode, SensorDType, SensorFrameHeader, SENSOR_FRAME_MAX_DIMS,
};

use crate::config::{DeviceExtra, TactileChannelExtra};
use crate::cora::{Bridge, PointCloud2Sample};
use crate::driver_name;

pub struct RunArgs {
    pub config: Option<PathBuf>,
    pub config_inline: Option<String>,
    pub dry_run: bool,
}

/// `sensor_msgs::msg::PointField::FLOAT32`.
const POINTFIELD_FLOAT32: u8 = 7;

pub fn run(args: RunArgs) -> Result<(), Box<dyn Error>> {
    let device = load_device(&args)?;
    if args.dry_run {
        return Ok(());
    }
    run_device(device)
}

fn load_device(args: &RunArgs) -> Result<BinaryDeviceConfig, Box<dyn Error>> {
    let device = if let Some(path) = &args.config {
        BinaryDeviceConfig::from_file(path)?
    } else if let Some(inline) = &args.config_inline {
        inline.parse::<BinaryDeviceConfig>()?
    } else {
        return Err("run requires either --config or --config-inline".into());
    };
    if device.driver != driver_name() {
        return Err(format!(
            "device \"{}\" uses driver \"{}\", expected {}",
            device.name,
            device.driver,
            driver_name()
        )
        .into());
    }
    Ok(device)
}

fn run_device(device: BinaryDeviceConfig) -> Result<(), Box<dyn Error>> {
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;

    let device_extra = DeviceExtra::parse(&device)?;
    let bridge = Bridge::new(device_extra.to_bridge_config())?;
    bridge.start()?;
    tracing::info!(
        "tactile-cora: bridge started (domain_id={}, participant={})",
        device_extra.cora_domain_id,
        device_extra.cora_participant_name
    );

    let mut handles = Vec::new();
    let mut subscriptions = Vec::new();

    for channel in device.channels.iter().filter(|c| c.enabled).cloned() {
        validate_channel(&device, &channel)?;
        let channel_extra = TactileChannelExtra::parse(&device, &channel)?;
        let n = channel_extra.tactile_point_count as usize;

        let (tx, rx) = crossbeam_channel::unbounded::<TactilePublish>();
        let bus_root = device.bus_root.clone();
        let channel_for_thread = channel.clone();
        let stop_flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name(format!("tactile-cora-pub-{}", channel.channel_type))
            .spawn(move || run_publisher(bus_root, channel_for_thread, n, rx, stop_flag))?;
        handles.push(thread);

        // Per-channel error gates so we warn at most once for each failure mode.
        let warn_bigendian = Arc::new(AtomicBool::new(false));
        let warn_count_mismatch = Arc::new(AtomicBool::new(false));
        let warn_datatype = Arc::new(AtomicBool::new(false));
        let warn_missing_field = Arc::new(AtomicBool::new(false));
        let warn_short_payload = Arc::new(AtomicBool::new(false));
        let dropped_counter = Arc::new(AtomicU64::new(0));

        let channel_type_for_log = channel.channel_type.clone();
        let topic_for_log = channel_extra.cora_topic.clone();
        let field_map = channel_extra.pointcloud_field_map.clone();
        let expected_n = channel_extra.tactile_point_count;

        let sub = bridge.subscribe_point_cloud2(
            &channel_extra.cora_topic,
            channel_extra.cora_qos,
            move |sample| match convert(
                &sample,
                expected_n,
                &field_map,
                &warn_bigendian,
                &warn_count_mismatch,
                &warn_datatype,
                &warn_missing_field,
                &warn_short_payload,
                &channel_type_for_log,
                &topic_for_log,
            ) {
                Ok(item) => {
                    let _ = tx.send(item);
                }
                Err(_) => {
                    dropped_counter.fetch_add(1, Ordering::Relaxed);
                }
            },
        )?;
        tracing::info!(
            "tactile-cora: subscribed channel \"{}\" -> Cora topic \"{}\" (qos={:?}), iceoryx2 service \"{}\", N={}",
            channel.channel_type,
            channel_extra.cora_topic,
            channel_extra.cora_qos,
            channel_sample_service_name(
                &device.bus_root,
                &channel.channel_type,
                "tactile_point_cloud2"
            ),
            channel_extra.tactile_point_count,
        );
        subscriptions.push(sub);
    }

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
    tracing::info!("tactile-cora: stop signal received; tearing down");

    drop(subscriptions);
    drop(bridge);

    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

fn validate_channel(
    device: &BinaryDeviceConfig,
    channel: &DeviceChannelConfigV2,
) -> Result<(), Box<dyn Error>> {
    if channel.kind != DeviceType::Sensor {
        return Err(format!(
            "device \"{}\" channel \"{}\": tactile-cora requires kind=sensor (got {:?})",
            device.name, channel.channel_type, channel.kind
        )
        .into());
    }
    let kinds: Vec<SensorStateKind> = channel
        .publish_states
        .iter()
        .filter_map(|s| s.as_sensor())
        .collect();
    if !kinds.contains(&SensorStateKind::TactilePointCloud2) {
        return Err(format!(
            "device \"{}\" channel \"{}\": publish_states must contain \"tactile_point_cloud2\"",
            device.name, channel.channel_type
        )
        .into());
    }
    let sample_rate_hz = channel.sample_rate_hz.ok_or_else(|| {
        format!(
            "device \"{}\" channel \"{}\": sample_rate_hz is required for sensor channels",
            device.name, channel.channel_type
        )
    })?;
    if !sample_rate_hz.is_finite() || sample_rate_hz <= 0.0 {
        return Err(format!(
            "device \"{}\" channel \"{}\": sample_rate_hz must be > 0",
            device.name, channel.channel_type
        )
        .into());
    }
    Ok(())
}

struct TactilePublish {
    ts_us: u64,
    /// Flat [N*6 * 4] byte vector, little-endian f32 per slot.
    payload: Vec<u8>,
}

/// Project six float32 slots per point. Returns `Err(())` on any precondition
/// failure (already-logged warnings are produced via the supplied gate atomics).
#[allow(clippy::too_many_arguments)]
fn convert(
    sample: &PointCloud2Sample,
    expected_n: u32,
    field_map: &[String; 6],
    warn_bigendian: &Arc<AtomicBool>,
    warn_count_mismatch: &Arc<AtomicBool>,
    warn_datatype: &Arc<AtomicBool>,
    warn_missing_field: &Arc<AtomicBool>,
    warn_short_payload: &Arc<AtomicBool>,
    channel_type: &str,
    topic: &str,
) -> Result<TactilePublish, ()> {
    if sample.is_bigendian {
        warn_once(warn_bigendian, || {
            tracing::warn!(
                "tactile-cora[{channel_type}]: dropping is_bigendian=true PointCloud2 from {topic} \
                 (Phase 1 supports little-endian only)"
            );
        });
        return Err(());
    }
    let n_message = sample.width.saturating_mul(sample.height);
    if n_message != expected_n {
        warn_once(warn_count_mismatch, || {
            tracing::warn!(
                "tactile-cora[{channel_type}]: dropping PointCloud2 ({} x {} = {} points) \
                 mismatching configured tactile_point_count={} on {topic}",
                sample.width,
                sample.height,
                n_message,
                expected_n,
            );
        });
        return Err(());
    }

    // Build slot → field offset (None means leave 0).
    let mut slot_offsets: [Option<u32>; 6] = [None; 6];
    for (slot, name) in field_map.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        match sample.fields.iter().find(|f| f.name == *name) {
            Some(f) if f.datatype != POINTFIELD_FLOAT32 => {
                warn_once(warn_datatype, || {
                    tracing::warn!(
                        "tactile-cora[{channel_type}]: dropping PointCloud2 on {topic}: field \"{}\" \
                         has datatype={} (Phase 1 requires FLOAT32={})",
                        name,
                        f.datatype,
                        POINTFIELD_FLOAT32,
                    );
                });
                return Err(());
            }
            Some(f) => slot_offsets[slot] = Some(f.offset),
            None => {
                warn_once(warn_missing_field, || {
                    tracing::warn!(
                        "tactile-cora[{channel_type}]: dropping PointCloud2 on {topic}: field \"{}\" \
                         not present (available={:?})",
                        name,
                        sample.fields.iter().map(|f| &f.name).collect::<Vec<_>>(),
                    );
                });
                return Err(());
            }
        }
    }

    let n = expected_n as usize;
    let point_step = sample.point_step as usize;
    let required_bytes = point_step.saturating_mul(n);
    if sample.data.len() < required_bytes {
        warn_once(warn_short_payload, || {
            tracing::warn!(
                "tactile-cora[{channel_type}]: dropping PointCloud2 on {topic}: data.len={} \
                 < required={} (point_step={} * N={})",
                sample.data.len(),
                required_bytes,
                sample.point_step,
                n,
            );
        });
        return Err(());
    }

    let mut payload: Vec<u8> = vec![0u8; 6 * 4 * n];
    for i in 0..n {
        let row_off = i * point_step;
        let row = &sample.data[row_off..row_off + point_step];
        for (slot, off_opt) in slot_offsets.iter().enumerate() {
            let value: f32 = match off_opt {
                Some(off) => {
                    let s = *off as usize;
                    if s + 4 > row.len() {
                        // Defensive: declared offset extends past point_step. Skip gracefully.
                        0.0
                    } else {
                        let bytes = [row[s], row[s + 1], row[s + 2], row[s + 3]];
                        f32::from_le_bytes(bytes)
                    }
                }
                None => 0.0,
            };
            let out_off = (i * 6 + slot) * 4;
            payload[out_off..out_off + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    Ok(TactilePublish {
        ts_us: sample.ts_us,
        payload,
    })
}

fn warn_once<F: FnOnce()>(gate: &Arc<AtomicBool>, emit: F) {
    if !gate.swap(true, Ordering::Relaxed) {
        emit();
    }
}

type SamplePublisher = iceoryx2::port::publisher::Publisher<ipc::Service, [u8], SensorFrameHeader>;
type ShutdownSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;
type ChannelModePublisher =
    iceoryx2::port::publisher::Publisher<ipc::Service, DeviceChannelMode, ()>;

fn run_publisher(
    bus_root: String,
    channel: DeviceChannelConfigV2,
    n: usize,
    rx: crossbeam_channel::Receiver<TactilePublish>,
    stop: Arc<AtomicBool>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("tactile-cora: NodeBuilder failed: {e}").into()
        })?;

    let publisher = open_sample_publisher(&node, &bus_root, &channel.channel_type, n)?;
    let shutdown = open_shutdown_subscriber(&node)?;
    let mode_info = open_mode_info_publisher(&node, &bus_root, &channel.channel_type)?;
    let _ = mode_info.send_copy(DeviceChannelMode::Enabled);

    let mut sample_index: u64 = 0;
    let payload_bytes = 6 * 4 * n;
    let mut shape_arr = [0u32; SENSOR_FRAME_MAX_DIMS];
    shape_arr[0] = n as u32;
    shape_arr[1] = 6;

    loop {
        if stop.load(Ordering::Relaxed) || drain_shutdown_events(&shutdown)? {
            let _ = mode_info.send_copy(DeviceChannelMode::Disabled);
            return Ok(());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(item) => {
                if item.payload.len() != payload_bytes {
                    tracing::error!(
                        "tactile-cora: payload size mismatch (got {}, expected {}); dropping",
                        item.payload.len(),
                        payload_bytes
                    );
                    continue;
                }
                let header = SensorFrameHeader {
                    timestamp_us: item.ts_us,
                    sample_index,
                    sensor_kind: SensorStateKind::TactilePointCloud2 as u32,
                    dtype: SensorDType::F32,
                    ndim: 2,
                    _pad: [0; 2],
                    shape: shape_arr,
                };
                let sample = publisher.loan_slice_uninit(payload_bytes).map_err(
                    |e| -> Box<dyn Error + Send + Sync> {
                        format!("tactile-cora: loan_slice_uninit failed: {e}").into()
                    },
                )?;
                let mut sample = sample;
                *sample.user_header_mut() = header;
                let sample = sample.write_from_slice(&item.payload);
                sample.send().map_err(|e| -> Box<dyn Error + Send + Sync> {
                    format!("tactile-cora: publisher.send failed: {e}").into()
                })?;
                sample_index = sample_index.saturating_add(1);
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                let _ = mode_info.send_copy(DeviceChannelMode::Disabled);
                return Ok(());
            }
        }
    }
}

fn open_sample_publisher(
    node: &Node<ipc::Service>,
    bus_root: &str,
    channel_type: &str,
    n: usize,
) -> Result<SamplePublisher, Box<dyn Error + Send + Sync>> {
    let topic = channel_sample_service_name(
        bus_root,
        channel_type,
        SensorStateKind::TactilePointCloud2.topic_suffix(),
    );
    let service_name: ServiceName =
        topic
            .as_str()
            .try_into()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("bad service name \"{topic}\": {e}").into()
            })?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<[u8]>()
        .user_header::<SensorFrameHeader>()
        .subscriber_max_buffer_size(SAMPLE_BUFFER)
        .history_size(SAMPLE_BUFFER)
        .max_publishers(SAMPLE_MAX_PUBLISHERS)
        .max_subscribers(SAMPLE_MAX_SUBSCRIBERS)
        .max_nodes(SAMPLE_MAX_NODES)
        .open_or_create()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("tactile-cora: open_or_create sample service \"{topic}\": {e}").into()
        })?;
    let publisher = service
        .publisher_builder()
        .initial_max_slice_len(n * 6 * SensorDType::F32.byte_size())
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("tactile-cora: publisher_builder failed: {e}").into()
        })?;
    Ok(publisher)
}

fn open_shutdown_subscriber(
    node: &Node<ipc::Service>,
) -> Result<ShutdownSubscriber, Box<dyn Error + Send + Sync>> {
    let service_name: ServiceName =
        CONTROL_EVENTS_SERVICE
            .try_into()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("bad CONTROL_EVENTS_SERVICE name: {e}").into()
            })?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("tactile-cora: open_or_create control/events: {e}").into()
        })?;
    let subscriber =
        service
            .subscriber_builder()
            .create()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("tactile-cora: subscriber_builder failed: {e}").into()
            })?;
    Ok(subscriber)
}

fn open_mode_info_publisher(
    node: &Node<ipc::Service>,
    bus_root: &str,
    channel_type: &str,
) -> Result<ChannelModePublisher, Box<dyn Error + Send + Sync>> {
    let topic = channel_mode_info_service_name(bus_root, channel_type);
    let service_name: ServiceName =
        topic
            .as_str()
            .try_into()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("bad mode service \"{topic}\": {e}").into()
            })?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<DeviceChannelMode>()
        .max_publishers(16)
        .max_subscribers(16)
        .max_nodes(16)
        .open_or_create()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("tactile-cora: open_or_create mode/info: {e}").into()
        })?;
    let publisher =
        service
            .publisher_builder()
            .create()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("tactile-cora: mode publisher_builder failed: {e}").into()
            })?;
    Ok(publisher)
}

fn drain_shutdown_events(
    subscriber: &ShutdownSubscriber,
) -> Result<bool, Box<dyn Error + Send + Sync>> {
    loop {
        match subscriber.receive() {
            Ok(Some(sample)) => {
                if matches!(*sample.payload(), ControlEvent::Shutdown) {
                    return Ok(true);
                }
            }
            Ok(None) => return Ok(false),
            Err(e) => {
                return Err(format!("tactile-cora: shutdown receive failed: {e}").into());
            }
        }
    }
}
