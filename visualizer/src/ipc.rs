//! iceoryx2 subscriber bookkeeping for the visualizer.
//!
//! Three modes of operation depending on the project's
//! `[encoder.preview] output_mode`:
//!
//! * **Jpeg**  — subscribe per camera to `…/preview-jpeg`. Forwards
//!   the JPEG bytes verbatim as binary message kind `0x01`.
//! * **Encoded** — subscribe per camera to `…/preview-config`
//!   (history=1) and `…/preview-packets`. Caches the latest config
//!   per camera (so new WS clients can be re-bootstrapped) and
//!   forwards each packet as binary message kind `0x03`. Config
//!   messages go out as kind `0x02`.
//!
//! In both modes, a per-camera publisher on `…/preview-control` is
//! opened so the WS handler can forward `set_preview_size` requests
//! upstream to the preview encoder.

use iceoryx2::prelude::*;
use rollio_bus::{
    preview_config_service_name, preview_control_service_name, preview_jpeg_service_name,
    preview_packet_service_name, CONTROL_EVENTS_SERVICE, PREVIEW_PACKET_BUFFER, STATE_BUFFER,
    STATE_MAX_NODES, STATE_MAX_PUBLISHERS, STATE_MAX_SUBSCRIBERS, STREAM_CONFIG_HISTORY_SIZE,
};
use rollio_types::config::{
    PreviewOutputMode, RobotStateKind, VisualizerCameraSourceConfig, VisualizerRobotSourceConfig,
};
use rollio_types::messages::{
    CameraFrameHeader, ControlEvent, EncodedPacketHeader, PreviewControl, SampleHeader,
};

pub enum IpcMessage {
    JpegFrame {
        name: String,
        header: CameraFrameHeader,
        payload: Vec<u8>,
    },
    EncodedConfig {
        name: String,
        header: EncodedPacketHeader,
        extradata: Vec<u8>,
    },
    EncodedPacket {
        name: String,
        header: EncodedPacketHeader,
        payload: Vec<u8>,
    },
    RobotStateMsg {
        name: String,
        state_kind: RobotStateKind,
        timestamp_us: u64,
        values: Vec<f64>,
        value_min: Vec<f64>,
        value_max: Vec<f64>,
    },
}

pub struct IpcPoller {
    node: Node<ipc::Service>,
    cameras: Vec<CameraSubscribers>,
    robot_subs: Vec<RobotSubscriber>,
    control_subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>,
    /// Per-camera `PreviewControl` publishers used by the WS handler
    /// to forward `set_preview_size` requests to the preview encoder.
    pub preview_control: Vec<PreviewControlPublisher>,
}

pub struct PreviewControlPublisher {
    pub channel_id: String,
    pub resizable: bool,
    pub publisher: iceoryx2::port::publisher::Publisher<ipc::Service, PreviewControl, ()>,
}

struct CameraSubscribers {
    name: String,
    kind: CameraKind,
}

enum CameraKind {
    Jpeg(iceoryx2::port::subscriber::Subscriber<ipc::Service, [u8], CameraFrameHeader>),
    Encoded {
        config: iceoryx2::port::subscriber::Subscriber<ipc::Service, [u8], EncodedPacketHeader>,
        packets: iceoryx2::port::subscriber::Subscriber<ipc::Service, [u8], EncodedPacketHeader>,
    },
}

struct RobotSubscriber {
    name: String,
    state_kind: RobotStateKind,
    value_len: usize,
    value_min: Vec<f64>,
    value_max: Vec<f64>,
    subscriber: RobotStateSubscriber,
}

enum RobotStateSubscriber {
    F64Vector(iceoryx2::port::subscriber::Subscriber<ipc::Service, [f64], SampleHeader>),
}

impl IpcPoller {
    pub fn new(
        camera_sources: &[VisualizerCameraSourceConfig],
        robot_sources: &[VisualizerRobotSourceConfig],
        output_mode: PreviewOutputMode,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let node = NodeBuilder::new()
            .signal_handling_mode(SignalHandlingMode::Disabled)
            .create::<ipc::Service>()?;

        let mut cameras = Vec::with_capacity(camera_sources.len());
        let mut preview_control = Vec::with_capacity(camera_sources.len());
        for source in camera_sources {
            let kind = match output_mode {
                PreviewOutputMode::Jpeg => {
                    let topic = preview_jpeg_service_name(&source.bus_root, &source.channel_type);
                    let service_name: ServiceName = topic.as_str().try_into()?;
                    let service = node
                        .service_builder(&service_name)
                        .publish_subscribe::<[u8]>()
                        .user_header::<CameraFrameHeader>()
                        .subscriber_max_buffer_size(PREVIEW_PACKET_BUFFER)
                        .max_publishers(16)
                        .max_subscribers(16)
                        .max_nodes(16)
                        .open_or_create()?;
                    CameraKind::Jpeg(service.subscriber_builder().create()?)
                }
                PreviewOutputMode::Encoded => {
                    let cfg_topic =
                        preview_config_service_name(&source.bus_root, &source.channel_type);
                    let pkt_topic =
                        preview_packet_service_name(&source.bus_root, &source.channel_type);
                    let cfg_service: ServiceName = cfg_topic.as_str().try_into()?;
                    let cfg = node
                        .service_builder(&cfg_service)
                        .publish_subscribe::<[u8]>()
                        .user_header::<EncodedPacketHeader>()
                        .history_size(STREAM_CONFIG_HISTORY_SIZE)
                        .subscriber_max_buffer_size(STREAM_CONFIG_HISTORY_SIZE.max(2))
                        .max_publishers(16)
                        .max_subscribers(16)
                        .max_nodes(16)
                        .open_or_create()?;
                    let pkt_service: ServiceName = pkt_topic.as_str().try_into()?;
                    let pkt = node
                        .service_builder(&pkt_service)
                        .publish_subscribe::<[u8]>()
                        .user_header::<EncodedPacketHeader>()
                        .subscriber_max_buffer_size(PREVIEW_PACKET_BUFFER)
                        .max_publishers(16)
                        .max_subscribers(16)
                        .max_nodes(16)
                        .open_or_create()?;
                    CameraKind::Encoded {
                        config: cfg.subscriber_builder().create()?,
                        packets: pkt.subscriber_builder().create()?,
                    }
                }
            };
            cameras.push(CameraSubscribers {
                name: source.channel_id.clone(),
                kind,
            });

            let control_topic =
                preview_control_service_name(&source.bus_root, &source.channel_type);
            let control_service: ServiceName = control_topic.as_str().try_into()?;
            let control_svc = node
                .service_builder(&control_service)
                .publish_subscribe::<PreviewControl>()
                .open_or_create()?;
            preview_control.push(PreviewControlPublisher {
                channel_id: source.channel_id.clone(),
                resizable: source.preview_resize_policy.is_resizable(),
                publisher: control_svc.publisher_builder().create()?,
            });
        }

        let mut robot_subs = Vec::with_capacity(robot_sources.len());
        for source in robot_sources {
            let service_name: ServiceName = source.state_topic.as_str().try_into()?;
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<[f64]>()
                .user_header::<SampleHeader>()
                .subscriber_max_buffer_size(STATE_BUFFER)
                .history_size(STATE_BUFFER)
                .max_publishers(STATE_MAX_PUBLISHERS)
                .max_subscribers(STATE_MAX_SUBSCRIBERS)
                .max_nodes(STATE_MAX_NODES)
                .open_or_create()?;
            let subscriber =
                RobotStateSubscriber::F64Vector(service.subscriber_builder().create()?);
            robot_subs.push(RobotSubscriber {
                name: source.channel_id.clone(),
                state_kind: source.state_kind,
                value_len: source.value_len as usize,
                value_min: source.value_min.clone(),
                value_max: source.value_max.clone(),
                subscriber,
            });
        }

        let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
        let control_service = node
            .service_builder(&control_service_name)
            .publish_subscribe::<ControlEvent>()
            .open_or_create()?;
        let control_subscriber = control_service.subscriber_builder().create()?;

        Ok(Self {
            node,
            cameras,
            robot_subs,
            control_subscriber,
            preview_control,
        })
    }

    /// Move the per-camera `PreviewControl` publishers out of the
    /// poller so the WS layer can own them. Called once at startup.
    pub fn take_preview_publishers(&mut self) -> Vec<PreviewControlPublisher> {
        std::mem::take(&mut self.preview_control)
    }

    pub fn poll(&self) -> Vec<IpcMessage> {
        let mut messages = Vec::new();
        for cam in &self.cameras {
            match &cam.kind {
                CameraKind::Jpeg(sub) => {
                    let mut latest: Option<IpcMessage> = None;
                    while let Ok(Some(sample)) = sub.receive() {
                        latest = Some(IpcMessage::JpegFrame {
                            name: cam.name.clone(),
                            header: *sample.user_header(),
                            payload: sample.payload().to_vec(),
                        });
                    }
                    if let Some(msg) = latest {
                        messages.push(msg);
                    }
                }
                CameraKind::Encoded { config, packets } => {
                    while let Ok(Some(sample)) = config.receive() {
                        messages.push(IpcMessage::EncodedConfig {
                            name: cam.name.clone(),
                            header: *sample.user_header(),
                            extradata: sample.payload().to_vec(),
                        });
                    }
                    while let Ok(Some(sample)) = packets.receive() {
                        messages.push(IpcMessage::EncodedPacket {
                            name: cam.name.clone(),
                            header: *sample.user_header(),
                            payload: sample.payload().to_vec(),
                        });
                    }
                }
            }
        }

        for robot in &self.robot_subs {
            let mut latest: Option<IpcMessage> = None;
            match &robot.subscriber {
                RobotStateSubscriber::F64Vector(subscriber) => {
                    while let Ok(Some(sample)) = subscriber.receive() {
                        let payload = sample.payload();
                        if robot.value_len != 0 && payload.len() != robot.value_len {
                            log::warn!(
                                "rollio-visualizer: skipping state sample for {}/{}: payload length {} != expected {}",
                                robot.name,
                                robot.state_kind.topic_suffix(),
                                payload.len(),
                                robot.value_len
                            );
                            continue;
                        }
                        latest = Some(IpcMessage::RobotStateMsg {
                            name: robot.name.clone(),
                            state_kind: robot.state_kind,
                            timestamp_us: sample.user_header().timestamp_us,
                            values: payload.to_vec(),
                            value_min: robot.value_min.clone(),
                            value_max: robot.value_max.clone(),
                        });
                    }
                }
            }
            if let Some(msg) = latest {
                messages.push(msg);
            }
        }

        messages
    }

    pub fn poll_shutdown(&self) -> bool {
        let mut shutdown = false;
        while let Ok(Some(sample)) = self.control_subscriber.receive() {
            if matches!(*sample.payload(), ControlEvent::Shutdown) {
                shutdown = true;
            }
        }
        shutdown
    }

    pub fn node(&self) -> &Node<ipc::Service> {
        &self.node
    }
}
