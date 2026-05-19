//! Runtime: starts the Cora bridge, opens one iceoryx2 publisher per enabled
//! IMU channel (service `samples/imu_accel_gyro` with `SensorFrameHeader`
//! user header), and forwards every `ImuSample` from the Cora callback into a
//! per-channel `crossbeam_channel`; a dedicated publisher thread drains the
//! channel, loans an iceoryx2 sample, writes the header + 6×f32 payload, and
//! sends. The publisher thread also subscribes to `control/events` so it can
//! exit when the controller broadcasts `ControlEvent::Shutdown`, and to
//! `bus_root/<channel>/info/mode` so we publish the `Enabled` mode info on
//! startup (matching pseudo's sensor channel behavior).

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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

use crate::config::{DeviceExtra, ImuChannelExtra};
use crate::cora::{Bridge, ImuSample};
use crate::driver_name;

pub struct RunArgs {
    pub config: Option<PathBuf>,
    pub config_inline: Option<String>,
    pub dry_run: bool,
}

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
        "imu-cora: bridge started (domain_id={}, participant={})",
        device_extra.cora_domain_id,
        device_extra.cora_participant_name
    );

    let mut handles = Vec::new();
    // Keep subscription handles alive for the duration of the run; dropping the
    // Bridge later tears all of them down.
    let mut subscriptions = Vec::new();

    for channel in device.channels.iter().filter(|c| c.enabled).cloned() {
        validate_channel(&device, &channel)?;
        let channel_extra = ImuChannelExtra::parse(&device, &channel)?;
        let (tx, rx) = crossbeam_channel::unbounded::<ImuPublish>();

        let bus_root = device.bus_root.clone();
        let channel_for_thread = channel.clone();
        let stop_flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name(format!("imu-cora-pub-{}", channel.channel_type))
            .spawn(move || run_publisher(bus_root, channel_for_thread, rx, stop_flag))?;
        handles.push(thread);

        let sub = bridge.subscribe_imu(
            &channel_extra.cora_topic,
            channel_extra.cora_qos,
            move |sample| {
                let _ = tx.send(ImuPublish::from(sample));
            },
        )?;
        tracing::info!(
            "imu-cora: subscribed channel \"{}\" -> Cora topic \"{}\" (qos={:?}), iceoryx2 service \"{}\"",
            channel.channel_type,
            channel_extra.cora_topic,
            channel_extra.cora_qos,
            channel_sample_service_name(&device.bus_root, &channel.channel_type, "imu_accel_gyro"),
        );
        subscriptions.push(sub);
    }

    // Main thread: wait until SIGINT/SIGTERM. Cora callbacks run on bridge
    // threads; iceoryx2 publishes run on per-channel publisher threads.
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
    tracing::info!("imu-cora: stop signal received; tearing down");

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
            "device \"{}\" channel \"{}\": imu-cora requires kind=sensor (got {:?})",
            device.name, channel.channel_type, channel.kind
        )
        .into());
    }
    let kinds: Vec<SensorStateKind> = channel
        .publish_states
        .iter()
        .filter_map(|s| s.as_sensor())
        .collect();
    if !kinds.iter().any(|k| *k == SensorStateKind::ImuAccelGyro) {
        return Err(format!(
            "device \"{}\" channel \"{}\": publish_states must contain \"imu_accel_gyro\"",
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

/// Per-message work item routed from the Cora callback to the publisher thread.
struct ImuPublish {
    ts_us: u64,
    payload: [f32; 6],
}

impl From<ImuSample> for ImuPublish {
    fn from(s: ImuSample) -> Self {
        Self {
            ts_us: s.ts_us,
            payload: [
                s.accel[0] as f32,
                s.accel[1] as f32,
                s.accel[2] as f32,
                s.gyro[0] as f32,
                s.gyro[1] as f32,
                s.gyro[2] as f32,
            ],
        }
    }
}

type SamplePublisher = iceoryx2::port::publisher::Publisher<ipc::Service, [u8], SensorFrameHeader>;
type ShutdownSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;
type ChannelModePublisher =
    iceoryx2::port::publisher::Publisher<ipc::Service, DeviceChannelMode, ()>;

fn run_publisher(
    bus_root: String,
    channel: DeviceChannelConfigV2,
    rx: crossbeam_channel::Receiver<ImuPublish>,
    stop: Arc<AtomicBool>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("imu-cora: NodeBuilder failed: {e}").into()
        })?;

    let publisher = open_sample_publisher(&node, &bus_root, &channel.channel_type)?;
    let shutdown = open_shutdown_subscriber(&node)?;
    let mode_info = open_mode_info_publisher(&node, &bus_root, &channel.channel_type)?;
    let _ = mode_info.send_copy(DeviceChannelMode::Enabled);

    let mut sample_index: u64 = 0;
    const PAYLOAD_BYTES: usize = 6 * 4;
    let mut shape_arr = [0u32; SENSOR_FRAME_MAX_DIMS];
    shape_arr[0] = 6;

    loop {
        if stop.load(Ordering::Relaxed) || drain_shutdown_events(&shutdown)? {
            let _ = mode_info.send_copy(DeviceChannelMode::Disabled);
            return Ok(());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(item) => {
                let header = SensorFrameHeader {
                    timestamp_us: item.ts_us,
                    sample_index,
                    sensor_kind: SensorStateKind::ImuAccelGyro as u32,
                    dtype: SensorDType::F32,
                    ndim: 1,
                    _pad: [0; 2],
                    shape: shape_arr,
                };
                let mut payload_bytes = [0u8; PAYLOAD_BYTES];
                for (i, f) in item.payload.iter().enumerate() {
                    payload_bytes[i * 4..(i + 1) * 4].copy_from_slice(&f.to_le_bytes());
                }
                let sample = publisher.loan_slice_uninit(PAYLOAD_BYTES).map_err(
                    |e| -> Box<dyn Error + Send + Sync> {
                        format!("imu-cora: loan_slice_uninit failed: {e}").into()
                    },
                )?;
                let mut sample = sample;
                *sample.user_header_mut() = header;
                let sample = sample.write_from_slice(&payload_bytes);
                sample.send().map_err(|e| -> Box<dyn Error + Send + Sync> {
                    format!("imu-cora: publisher.send failed: {e}").into()
                })?;
                sample_index = sample_index.saturating_add(1);
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                // No new samples within the timeout — just loop back to check stop.
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                // Bridge has been dropped; sender side is closed.
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
) -> Result<SamplePublisher, Box<dyn Error + Send + Sync>> {
    let topic = channel_sample_service_name(
        bus_root,
        channel_type,
        SensorStateKind::ImuAccelGyro.topic_suffix(),
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
            format!("imu-cora: open_or_create sample service \"{topic}\": {e}").into()
        })?;
    let publisher = service
        .publisher_builder()
        .initial_max_slice_len(6 * SensorDType::F32.byte_size())
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("imu-cora: publisher_builder failed: {e}").into()
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
            format!("imu-cora: open_or_create control/events: {e}").into()
        })?;
    let subscriber =
        service
            .subscriber_builder()
            .create()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("imu-cora: subscriber_builder failed: {e}").into()
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
            format!("imu-cora: open_or_create mode/info: {e}").into()
        })?;
    let publisher =
        service
            .publisher_builder()
            .create()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("imu-cora: mode publisher_builder failed: {e}").into()
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
                return Err(format!("imu-cora: shutdown receive failed: {e}").into());
            }
        }
    }
}
