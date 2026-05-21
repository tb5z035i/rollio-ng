//! Runtime: starts the Cora bridge, opens one iceoryx2 `JointVector15` publisher
//! per requested `publish_states` kind, subscribes to the configured JointState
//! topic, and forwards each accepted message to the publisher thread which
//! packs `JointVector15 { len: 1, values[0] = value, ... }` and sends.
//!
//! Phase 1 strictness: the named joint must be present on the first message
//! (channel fails fast otherwise). For subsequent messages where the joint
//! arrays are empty, that state's publish is skipped + warn-once.

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use iceoryx2::prelude::*;
use rollio_bus::{
    channel_mode_info_service_name, channel_state_service_name, CONTROL_EVENTS_MAX_NODES,
    CONTROL_EVENTS_MAX_PUBLISHERS, CONTROL_EVENTS_MAX_SUBSCRIBERS, CONTROL_EVENTS_SERVICE,
    STATE_BUFFER, STATE_MAX_NODES, STATE_MAX_PUBLISHERS, STATE_MAX_SUBSCRIBERS,
};
use rollio_types::config::{BinaryDeviceConfig, DeviceChannelConfigV2, DeviceType, RobotStateKind};
use rollio_types::messages::{ControlEvent, DeviceChannelMode, JointVector15};

use crate::config::{DeviceExtra, GripperChannelExtra};
use crate::cora::{Bridge, JointStateSample};
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
        "gripper-cora: bridge started (domain_id={}, participant={})",
        device_extra.cora_domain_id,
        device_extra.cora_participant_name
    );

    let mut handles = Vec::new();
    let mut subscriptions = Vec::new();

    for channel in device.channels.iter().filter(|c| c.enabled).cloned() {
        validate_channel(&device, &channel)?;
        let channel_extra = GripperChannelExtra::parse(&device, &channel)?;
        let publish_kinds: Vec<RobotStateKind> = channel
            .publish_states
            .iter()
            .copied()
            .filter(|k| {
                matches!(
                    k,
                    RobotStateKind::JointPosition
                        | RobotStateKind::JointVelocity
                        | RobotStateKind::JointEffort
                )
            })
            .collect();

        let (tx, rx) = crossbeam_channel::unbounded::<GripperPublish>();
        let bus_root = device.bus_root.clone();
        let channel_for_thread = channel.clone();
        let publish_kinds_for_thread = publish_kinds.clone();
        let stop_flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name(format!("gripper-cora-pub-{}", channel.channel_type))
            .spawn(move || {
                run_publisher(
                    bus_root,
                    channel_for_thread,
                    publish_kinds_for_thread,
                    rx,
                    stop_flag,
                )
            })?;
        handles.push(thread);

        let joint_name = channel_extra.joint_name.clone();
        let publish_kinds_for_cb = publish_kinds.clone();
        let cached_index: parking_lot::Mutex<Option<usize>> = parking_lot::Mutex::new(None);
        let warn_missing_joint = Arc::new(AtomicBool::new(false));
        let warn_no_position = Arc::new(AtomicBool::new(false));
        let warn_no_velocity = Arc::new(AtomicBool::new(false));
        let warn_no_effort = Arc::new(AtomicBool::new(false));
        let channel_type_log = channel.channel_type.clone();

        let sub = bridge.subscribe_joint_state(
            &channel_extra.cora_topic,
            channel_extra.cora_qos,
            move |sample| {
                let idx_opt = resolve_index(
                    &sample,
                    &joint_name,
                    &cached_index,
                    &warn_missing_joint,
                    &channel_type_log,
                );
                let Some(idx) = idx_opt else { return };

                let mut item = GripperPublish {
                    ts_us: sample.ts_us,
                    position: None,
                    velocity: None,
                    effort: None,
                };
                for kind in &publish_kinds_for_cb {
                    match kind {
                        RobotStateKind::JointPosition => {
                            if let Some(v) = sample.positions.get(idx).copied() {
                                item.position = Some(v);
                            } else {
                                warn_once(&warn_no_position, || {
                                    tracing::warn!(
                                        "gripper-cora[{channel_type_log}]: positions[{idx}] missing; skipping joint_position"
                                    );
                                });
                            }
                        }
                        RobotStateKind::JointVelocity => {
                            if let Some(v) = sample.velocities.get(idx).copied() {
                                item.velocity = Some(v);
                            } else {
                                warn_once(&warn_no_velocity, || {
                                    tracing::warn!(
                                        "gripper-cora[{channel_type_log}]: velocities[{idx}] missing; skipping joint_velocity"
                                    );
                                });
                            }
                        }
                        RobotStateKind::JointEffort => {
                            if let Some(v) = sample.efforts.get(idx).copied() {
                                item.effort = Some(v);
                            } else {
                                warn_once(&warn_no_effort, || {
                                    tracing::warn!(
                                        "gripper-cora[{channel_type_log}]: efforts[{idx}] missing; skipping joint_effort"
                                    );
                                });
                            }
                        }
                        _ => {}
                    }
                }
                let _ = tx.send(item);
            },
        )?;
        tracing::info!(
            "gripper-cora: subscribed channel \"{}\" -> Cora topic \"{}\" (qos={:?}) joint_name=\"{}\" publish_kinds={:?}",
            channel.channel_type,
            channel_extra.cora_topic,
            channel_extra.cora_qos,
            channel_extra.joint_name,
            publish_kinds,
        );
        subscriptions.push(sub);
    }

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
    tracing::info!("gripper-cora: stop signal received; tearing down");

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
    if channel.kind != DeviceType::Robot {
        return Err(format!(
            "device \"{}\" channel \"{}\": gripper-cora requires kind=robot (got {:?})",
            device.name, channel.channel_type, channel.kind
        )
        .into());
    }
    let dof = channel.dof.ok_or_else(|| {
        format!(
            "device \"{}\" channel \"{}\": dof is required (must be 1)",
            device.name, channel.channel_type
        )
    })?;
    if dof != 1 {
        return Err(format!(
            "device \"{}\" channel \"{}\": gripper-cora only supports dof=1 (got dof={})",
            device.name, channel.channel_type, dof
        )
        .into());
    }
    let kinds: Vec<RobotStateKind> = channel.publish_states.clone();
    if kinds.is_empty() {
        return Err(format!(
            "device \"{}\" channel \"{}\": publish_states must contain at least one of \
             joint_position / joint_velocity / joint_effort",
            device.name, channel.channel_type
        )
        .into());
    }
    for k in &kinds {
        if !matches!(
            k,
            RobotStateKind::JointPosition
                | RobotStateKind::JointVelocity
                | RobotStateKind::JointEffort
        ) {
            return Err(format!(
                "device \"{}\" channel \"{}\": gripper-cora does not publish state kind \"{:?}\" \
                 (allowed: joint_position, joint_velocity, joint_effort)",
                device.name, channel.channel_type, k
            )
            .into());
        }
    }
    Ok(())
}

/// Find (and cache) the position of `joint_name` in `sample.names`. Returns
/// `None` (with a one-shot warning) if the joint is not present.
fn resolve_index(
    sample: &JointStateSample,
    joint_name: &str,
    cached: &parking_lot::Mutex<Option<usize>>,
    warn: &Arc<AtomicBool>,
    channel_type: &str,
) -> Option<usize> {
    let mut slot = cached.lock();
    if let Some(idx) = *slot {
        if sample.names.get(idx).map(String::as_str) == Some(joint_name) {
            return Some(idx);
        }
        // name[] reshuffled; rebuild.
        *slot = None;
    }
    let found = sample.names.iter().position(|s| s == joint_name);
    if let Some(idx) = found {
        *slot = Some(idx);
        Some(idx)
    } else {
        warn_once(warn, || {
            tracing::warn!(
                "gripper-cora[{channel_type}]: joint_name \"{joint_name}\" not found in names={:?}; \
                 dropping message",
                sample.names
            );
        });
        None
    }
}

fn warn_once<F: FnOnce()>(gate: &Arc<AtomicBool>, emit: F) {
    if !gate.swap(true, Ordering::Relaxed) {
        emit();
    }
}

struct GripperPublish {
    ts_us: u64,
    position: Option<f64>,
    velocity: Option<f64>,
    effort: Option<f64>,
}

type JointVectorPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, JointVector15, ()>;
type ShutdownSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;
type ChannelModePublisher =
    iceoryx2::port::publisher::Publisher<ipc::Service, DeviceChannelMode, ()>;

fn run_publisher(
    bus_root: String,
    channel: DeviceChannelConfigV2,
    publish_kinds: Vec<RobotStateKind>,
    rx: crossbeam_channel::Receiver<GripperPublish>,
    stop: Arc<AtomicBool>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("gripper-cora: NodeBuilder failed: {e}").into()
        })?;

    let mut position_pub: Option<JointVectorPublisher> = None;
    let mut velocity_pub: Option<JointVectorPublisher> = None;
    let mut effort_pub: Option<JointVectorPublisher> = None;
    for kind in &publish_kinds {
        let pub_ = open_state_publisher(&node, &bus_root, &channel.channel_type, *kind)?;
        match kind {
            RobotStateKind::JointPosition => position_pub = Some(pub_),
            RobotStateKind::JointVelocity => velocity_pub = Some(pub_),
            RobotStateKind::JointEffort => effort_pub = Some(pub_),
            _ => {}
        }
    }
    let shutdown = open_shutdown_subscriber(&node)?;
    let mode_info = open_mode_info_publisher(&node, &bus_root, &channel.channel_type)?;
    let _ = mode_info.send_copy(DeviceChannelMode::Enabled);

    loop {
        if stop.load(Ordering::Relaxed) || drain_shutdown_events(&shutdown)? {
            stop.store(true, Ordering::Relaxed);
            let _ = mode_info.send_copy(DeviceChannelMode::Disabled);
            return Ok(());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(item) => {
                if let (Some(v), Some(pub_)) = (item.position, position_pub.as_ref()) {
                    let payload = JointVector15::from_slice(item.ts_us, &[v]);
                    pub_.send_copy(payload)
                        .map_err(|e| -> Box<dyn Error + Send + Sync> {
                            format!("gripper-cora: send joint_position failed: {e}").into()
                        })?;
                }
                if let (Some(v), Some(pub_)) = (item.velocity, velocity_pub.as_ref()) {
                    let payload = JointVector15::from_slice(item.ts_us, &[v]);
                    pub_.send_copy(payload)
                        .map_err(|e| -> Box<dyn Error + Send + Sync> {
                            format!("gripper-cora: send joint_velocity failed: {e}").into()
                        })?;
                }
                if let (Some(v), Some(pub_)) = (item.effort, effort_pub.as_ref()) {
                    let payload = JointVector15::from_slice(item.ts_us, &[v]);
                    pub_.send_copy(payload)
                        .map_err(|e| -> Box<dyn Error + Send + Sync> {
                            format!("gripper-cora: send joint_effort failed: {e}").into()
                        })?;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                stop.store(true, Ordering::Relaxed);
                let _ = mode_info.send_copy(DeviceChannelMode::Disabled);
                return Ok(());
            }
        }
    }
}

fn open_state_publisher(
    node: &Node<ipc::Service>,
    bus_root: &str,
    channel_type: &str,
    kind: RobotStateKind,
) -> Result<JointVectorPublisher, Box<dyn Error + Send + Sync>> {
    let topic = channel_state_service_name(bus_root, channel_type, kind.topic_suffix());
    let service_name: ServiceName =
        topic
            .as_str()
            .try_into()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("bad state service name \"{topic}\": {e}").into()
            })?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<JointVector15>()
        .subscriber_max_buffer_size(STATE_BUFFER)
        .history_size(STATE_BUFFER)
        .max_publishers(STATE_MAX_PUBLISHERS)
        .max_subscribers(STATE_MAX_SUBSCRIBERS)
        .max_nodes(STATE_MAX_NODES)
        .open_or_create()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("gripper-cora: open_or_create state service \"{topic}\": {e}").into()
        })?;
    let publisher =
        service
            .publisher_builder()
            .create()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("gripper-cora: publisher_builder for \"{topic}\" failed: {e}").into()
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
        .max_publishers(CONTROL_EVENTS_MAX_PUBLISHERS)
        .max_subscribers(CONTROL_EVENTS_MAX_SUBSCRIBERS)
        .max_nodes(CONTROL_EVENTS_MAX_NODES)
        .open_or_create()
        .map_err(|e| -> Box<dyn Error + Send + Sync> {
            format!("gripper-cora: open_or_create control/events: {e}").into()
        })?;
    let subscriber =
        service
            .subscriber_builder()
            .create()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("gripper-cora: subscriber_builder failed: {e}").into()
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
            format!("gripper-cora: open_or_create mode/info: {e}").into()
        })?;
    let publisher =
        service
            .publisher_builder()
            .create()
            .map_err(|e| -> Box<dyn Error + Send + Sync> {
                format!("gripper-cora: mode publisher_builder failed: {e}").into()
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
                return Err(format!("gripper-cora: shutdown receive failed: {e}").into());
            }
        }
    }
}
