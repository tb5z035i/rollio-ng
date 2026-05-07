/// iceoryx2 subscriber management for the Visualizer.
///
/// `IpcPoller` creates subscribers for camera frame and robot state topics,
/// then polls them in a non-blocking loop. Camera frame data is copied out
/// of shared memory once (unavoidable since we release the sample), while
/// robot state is a small fixed-size Copy type.
use iceoryx2::prelude::*;
use rollio_bus::{
    CONTROL_EVENTS_SERVICE, STATE_BUFFER, STATE_MAX_NODES, STATE_MAX_PUBLISHERS,
    STATE_MAX_SUBSCRIBERS,
};
use rollio_types::config::{
    RobotStateKind, VisualizerCameraSourceConfig, VisualizerImuSourceConfig,
    VisualizerRobotSourceConfig,
};
use rollio_types::messages::{
    CameraFrameHeader, ControlEvent, Imu, JointVector15, ParallelVector2, Pose7,
};

/// A message received from iceoryx2.
pub enum IpcMessage {
    CameraFrame {
        name: String,
        header: CameraFrameHeader,
        data: Vec<u8>,
    },
    RobotStateMsg {
        name: String,
        state_kind: RobotStateKind,
        timestamp_us: u64,
        values: Vec<f64>,
        value_min: Vec<f64>,
        value_max: Vec<f64>,
    },
    /// Inertial sample. Covariances are dropped before forwarding to the UI;
    /// downstream consumers that need them can subscribe to the underlying
    /// iceoryx2 service directly via the channel id. Most fields are
    /// `#[allow(dead_code)]` for now because the visualizer's IMU rendering
    /// is a follow-up; the data is plumbed through so bus-tap and external
    /// consumers see it without further plumbing.
    #[allow(dead_code)]
    ImuSample {
        name: String,
        timestamp_us: u64,
        orientation: [f64; 4],
        angular_velocity: [f64; 3],
        linear_acceleration: [f64; 3],
    },
}

/// Manages iceoryx2 subscribers for camera, robot, and IMU topics.
pub struct IpcPoller {
    node: Node<ipc::Service>,
    camera_subs: Vec<CameraSubscriber>,
    robot_subs: Vec<RobotSubscriber>,
    imu_subs: Vec<ImuSubscriber>,
    control_subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>,
}

struct CameraSubscriber {
    name: String,
    subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, [u8], CameraFrameHeader>,
}

struct RobotSubscriber {
    name: String,
    state_kind: RobotStateKind,
    value_min: Vec<f64>,
    value_max: Vec<f64>,
    subscriber: RobotStateSubscriber,
}

enum RobotStateSubscriber {
    JointVector15(iceoryx2::port::subscriber::Subscriber<ipc::Service, JointVector15, ()>),
    ParallelVector2(iceoryx2::port::subscriber::Subscriber<ipc::Service, ParallelVector2, ()>),
    Pose7(iceoryx2::port::subscriber::Subscriber<ipc::Service, Pose7, ()>),
}

struct ImuSubscriber {
    name: String,
    subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, Imu, ()>,
}

impl IpcPoller {
    /// Create a new IpcPoller that subscribes to the given camera and robot topics.
    ///
    /// Uses `open_or_create` so the visualizer starts even if publishers don't exist yet.
    pub fn new(
        camera_sources: &[VisualizerCameraSourceConfig],
        robot_sources: &[VisualizerRobotSourceConfig],
        imu_sources: &[VisualizerImuSourceConfig],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let node = NodeBuilder::new()
            .signal_handling_mode(SignalHandlingMode::Disabled)
            .create::<ipc::Service>()?;

        let mut camera_subs = Vec::with_capacity(camera_sources.len());
        for source in camera_sources {
            // The visualizer subscribes to the encoder's RGB24 preview
            // tap (always-on, throttled to `preview_fps`), not to the
            // raw camera frames topic. The preview is already downsized
            // and converted from YUYV/MJPG by the encoder, so the
            // visualizer never has to decode anything camera-side.
            let service_name_str = source.preview_topic.clone();
            let service_name: ServiceName = service_name_str.as_str().try_into()?;

            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<[u8]>()
                .user_header::<CameraFrameHeader>()
                .open_or_create()?;

            let subscriber = service.subscriber_builder().create()?;

            log::info!("subscribed to camera preview: {service_name_str}");
            camera_subs.push(CameraSubscriber {
                name: source.channel_id.clone(),
                subscriber,
            });
        }

        let mut robot_subs = Vec::with_capacity(robot_sources.len());
        for source in robot_sources {
            let service_name_str = source.state_topic.clone();
            let service_name: ServiceName = service_name_str.as_str().try_into()?;

            // The visualizer is a state subscriber. Match the producer-side
            // caps (see `rollio_bus::STATE_BUFFER`) — without them
            // `open_or_create` rejects the subscription with mismatched
            // `subscriber_max_buffer_size`.
            let subscriber = if source.state_kind.uses_pose_payload() {
                let service = node
                    .service_builder(&service_name)
                    .publish_subscribe::<Pose7>()
                    .subscriber_max_buffer_size(STATE_BUFFER)
                    .history_size(STATE_BUFFER)
                    .max_publishers(STATE_MAX_PUBLISHERS)
                    .max_subscribers(STATE_MAX_SUBSCRIBERS)
                    .max_nodes(STATE_MAX_NODES)
                    .open_or_create()?;
                RobotStateSubscriber::Pose7(service.subscriber_builder().create()?)
            } else if matches!(
                source.state_kind,
                RobotStateKind::ParallelPosition
                    | RobotStateKind::ParallelVelocity
                    | RobotStateKind::ParallelEffort
            ) {
                let service = node
                    .service_builder(&service_name)
                    .publish_subscribe::<ParallelVector2>()
                    .subscriber_max_buffer_size(STATE_BUFFER)
                    .history_size(STATE_BUFFER)
                    .max_publishers(STATE_MAX_PUBLISHERS)
                    .max_subscribers(STATE_MAX_SUBSCRIBERS)
                    .max_nodes(STATE_MAX_NODES)
                    .open_or_create()?;
                RobotStateSubscriber::ParallelVector2(service.subscriber_builder().create()?)
            } else {
                let service = node
                    .service_builder(&service_name)
                    .publish_subscribe::<JointVector15>()
                    .subscriber_max_buffer_size(STATE_BUFFER)
                    .history_size(STATE_BUFFER)
                    .max_publishers(STATE_MAX_PUBLISHERS)
                    .max_subscribers(STATE_MAX_SUBSCRIBERS)
                    .max_nodes(STATE_MAX_NODES)
                    .open_or_create()?;
                RobotStateSubscriber::JointVector15(service.subscriber_builder().create()?)
            };

            log::info!("subscribed to robot: {service_name_str}");
            robot_subs.push(RobotSubscriber {
                // Always key on the channel id so the UI can group every
                // state-kind belonging to one channel into a single panel.
                // The previous "channel_id/<state_kind>" naming made grouping
                // ambiguous and forced consumers to special-case
                // joint_position vs the rest.
                name: source.channel_id.clone(),
                state_kind: source.state_kind,
                value_min: source.value_min.clone(),
                value_max: source.value_max.clone(),
                subscriber,
            });
        }

        let mut imu_subs = Vec::with_capacity(imu_sources.len());
        for source in imu_sources {
            let service_name_str = source.state_topic.clone();
            let service_name: ServiceName = service_name_str.as_str().try_into()?;
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<Imu>()
                .subscriber_max_buffer_size(STATE_BUFFER)
                .history_size(STATE_BUFFER)
                .max_publishers(STATE_MAX_PUBLISHERS)
                .max_subscribers(STATE_MAX_SUBSCRIBERS)
                .max_nodes(STATE_MAX_NODES)
                .open_or_create()?;
            let subscriber = service.subscriber_builder().create()?;
            log::info!("subscribed to imu: {service_name_str}");
            imu_subs.push(ImuSubscriber {
                name: source.channel_id.clone(),
                subscriber,
            });
        }

        // Listen for ControlEvent::Shutdown so the controller can stop us
        // promptly during a preview-runtime swap. Without this, identify
        // would block on `terminate_children` for the full 30 s timeout.
        let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
        let control_service = node
            .service_builder(&control_service_name)
            .publish_subscribe::<ControlEvent>()
            .open_or_create()?;
        let control_subscriber = control_service.subscriber_builder().create()?;

        Ok(Self {
            node,
            camera_subs,
            robot_subs,
            imu_subs,
            control_subscriber,
        })
    }

    /// Non-blocking poll of all subscribers. Returns all available messages.
    ///
    /// Drains each subscriber's queue completely before moving to the next.
    /// For camera frames, only the latest frame per camera is kept (skip older ones).
    pub fn poll(&self) -> Vec<IpcMessage> {
        let mut messages = Vec::new();

        // For cameras, we only want the latest frame (skip older ones to reduce latency)
        for cam in &self.camera_subs {
            let mut latest: Option<IpcMessage> = None;
            loop {
                match cam.subscriber.receive() {
                    Ok(Some(sample)) => {
                        let header = *sample.user_header();
                        let data = sample.payload().to_vec();
                        latest = Some(IpcMessage::CameraFrame {
                            name: cam.name.clone(),
                            header,
                            data,
                        });
                    }
                    Ok(None) => break,
                    Err(e) => {
                        log::warn!("camera {} receive error: {e}", cam.name);
                        break;
                    }
                }
            }
            if let Some(msg) = latest {
                messages.push(msg);
            }
        }

        // For robots, drain all messages (they're small and we want every state update)
        for robot in &self.robot_subs {
            let mut latest: Option<IpcMessage> = None;
            loop {
                match &robot.subscriber {
                    RobotStateSubscriber::JointVector15(subscriber) => match subscriber.receive() {
                        Ok(Some(sample)) => {
                            let payload = *sample.payload();
                            latest = Some(IpcMessage::RobotStateMsg {
                                name: robot.name.clone(),
                                state_kind: robot.state_kind,
                                timestamp_us: payload.timestamp_us,
                                values: payload.values[..payload.len as usize].to_vec(),
                                value_min: robot.value_min.clone(),
                                value_max: robot.value_max.clone(),
                            });
                        }
                        Ok(None) => break,
                        Err(e) => {
                            log::warn!("robot {} receive error: {e}", robot.name);
                            break;
                        }
                    },
                    RobotStateSubscriber::ParallelVector2(subscriber) => match subscriber.receive()
                    {
                        Ok(Some(sample)) => {
                            let payload = *sample.payload();
                            latest = Some(IpcMessage::RobotStateMsg {
                                name: robot.name.clone(),
                                state_kind: robot.state_kind,
                                timestamp_us: payload.timestamp_us,
                                values: payload.values[..payload.len as usize].to_vec(),
                                value_min: robot.value_min.clone(),
                                value_max: robot.value_max.clone(),
                            });
                        }
                        Ok(None) => break,
                        Err(e) => {
                            log::warn!("robot {} receive error: {e}", robot.name);
                            break;
                        }
                    },
                    RobotStateSubscriber::Pose7(subscriber) => match subscriber.receive() {
                        Ok(Some(sample)) => {
                            let payload = *sample.payload();
                            latest = Some(IpcMessage::RobotStateMsg {
                                name: robot.name.clone(),
                                state_kind: robot.state_kind,
                                timestamp_us: payload.timestamp_us,
                                values: payload.values.to_vec(),
                                value_min: robot.value_min.clone(),
                                value_max: robot.value_max.clone(),
                            });
                        }
                        Ok(None) => break,
                        Err(e) => {
                            log::warn!("robot {} receive error: {e}", robot.name);
                            break;
                        }
                    },
                }
            }
            if let Some(msg) = latest {
                messages.push(msg);
            }
        }

        // For IMU, drain every buffered sample (high-rate but small POD) so
        // downstream consumers get the full stream.
        for imu in &self.imu_subs {
            loop {
                match imu.subscriber.receive() {
                    Ok(Some(sample)) => {
                        let payload = *sample.payload();
                        messages.push(IpcMessage::ImuSample {
                            name: imu.name.clone(),
                            timestamp_us: payload.timestamp_us,
                            orientation: payload.orientation,
                            angular_velocity: payload.angular_velocity,
                            linear_acceleration: payload.linear_acceleration,
                        });
                    }
                    Ok(None) => break,
                    Err(e) => {
                        log::warn!("imu {} receive error: {e}", imu.name);
                        break;
                    }
                }
            }
        }

        messages
    }

    /// Drain pending control events. Returns `true` if a `Shutdown` event
    /// was observed.
    pub fn poll_shutdown(&self) -> bool {
        let mut shutdown = false;
        loop {
            match self.control_subscriber.receive() {
                Ok(Some(sample)) => {
                    if matches!(*sample.payload(), ControlEvent::Shutdown) {
                        shutdown = true;
                    }
                }
                Ok(None) => return shutdown,
                Err(e) => {
                    log::warn!("control events receive error: {e}");
                    return shutdown;
                }
            }
        }
    }

    /// Access the iceoryx2 node (for `node.wait()` in the poll loop).
    pub fn node(&self) -> &Node<ipc::Service> {
        &self.node
    }
}
