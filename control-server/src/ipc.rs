//! iceoryx2 transport for the control server.
//!
//! Owns one iceoryx2 [`Node`] plus per-role publishers/subscribers. The poll
//! loop drains each subscriber and pushes JSON snapshots into the broadcast
//! channel; outgoing UI commands are pushed onto the matching publisher.

use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::{
    BACKPRESSURE_SERVICE, EPISODE_COMMAND_SERVICE, EPISODE_STATUS_SERVICE, SETUP_COMMAND_SERVICE,
    SETUP_STATE_SERVICE,
};
use rollio_types::messages::{
    BackpressureEvent, EpisodeCommand, EpisodeStatus, SetupCommandMessage, SetupStateMessage,
};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use crate::protocol;
use crate::{ControlServerRole, OutboundMessage, UiCommand};

pub fn run_poll_loop(
    role: ControlServerRole,
    command_rx: mpsc::Receiver<UiCommand>,
    outbound_tx: tokio::sync::broadcast::Sender<OutboundMessage>,
    latest_snapshot: Arc<std::sync::Mutex<Option<String>>>,
    shutdown: Arc<AtomicBool>,
) -> Result<(), Box<dyn Error>> {
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    // The control server intentionally does NOT subscribe to
    // CONTROL_EVENTS_SERVICE: that bus is used by the controller to stop the
    // *preview runtime* (visualizer + identify-target device) on every
    // identify swap. Listening here would tear the long-lived control plane
    // down with each swap (root cause of the 8d351b debug session).
    // Lifecycle is owned by SIGTERM/SIGINT in `lib.rs`.
    let mut role_io = RoleIo::new(&node, role)?;

    log::info!("control server iceoryx2 poll loop started ({role:?})");

    while !shutdown.load(Ordering::Relaxed) {
        for cmd in command_rx.try_iter() {
            role_io.publish_command(cmd)?;
        }

        role_io.poll_once(&outbound_tx, &latest_snapshot)?;

        match node.wait(Duration::from_millis(2)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => {
                log::info!("control server poll loop interrupted by shutdown signal");
                break;
            }
        }
    }

    log::info!("control server iceoryx2 poll loop stopped");
    Ok(())
}

enum RoleIo {
    Setup {
        state_subscriber:
            iceoryx2::port::subscriber::Subscriber<ipc::Service, SetupStateMessage, ()>,
        command_publisher:
            iceoryx2::port::publisher::Publisher<ipc::Service, SetupCommandMessage, ()>,
    },
    Collect {
        status_subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, EpisodeStatus, ()>,
        backpressure_subscriber:
            iceoryx2::port::subscriber::Subscriber<ipc::Service, BackpressureEvent, ()>,
        command_publisher: iceoryx2::port::publisher::Publisher<ipc::Service, EpisodeCommand, ()>,
    },
}

impl RoleIo {
    fn new(node: &Node<ipc::Service>, role: ControlServerRole) -> Result<Self, Box<dyn Error>> {
        match role {
            ControlServerRole::Setup => {
                let state_service_name: ServiceName = SETUP_STATE_SERVICE.try_into()?;
                let state_service = node
                    .service_builder(&state_service_name)
                    .publish_subscribe::<SetupStateMessage>()
                    .open_or_create()?;
                let state_subscriber = state_service.subscriber_builder().create()?;

                let command_service_name: ServiceName = SETUP_COMMAND_SERVICE.try_into()?;
                let command_service = node
                    .service_builder(&command_service_name)
                    .publish_subscribe::<SetupCommandMessage>()
                    .open_or_create()?;
                let command_publisher = command_service.publisher_builder().create()?;

                Ok(Self::Setup {
                    state_subscriber,
                    command_publisher,
                })
            }
            ControlServerRole::Collect => {
                let status_service_name: ServiceName = EPISODE_STATUS_SERVICE.try_into()?;
                let status_service = node
                    .service_builder(&status_service_name)
                    .publish_subscribe::<EpisodeStatus>()
                    .open_or_create()?;
                let status_subscriber = status_service.subscriber_builder().create()?;

                let backpressure_service_name: ServiceName = BACKPRESSURE_SERVICE.try_into()?;
                let backpressure_service = node
                    .service_builder(&backpressure_service_name)
                    .publish_subscribe::<BackpressureEvent>()
                    .open_or_create()?;
                let backpressure_subscriber = backpressure_service.subscriber_builder().create()?;

                let command_service_name: ServiceName = EPISODE_COMMAND_SERVICE.try_into()?;
                let command_service = node
                    .service_builder(&command_service_name)
                    .publish_subscribe::<EpisodeCommand>()
                    .open_or_create()?;
                let command_publisher = command_service.publisher_builder().create()?;

                Ok(Self::Collect {
                    status_subscriber,
                    backpressure_subscriber,
                    command_publisher,
                })
            }
        }
    }

    fn publish_command(&self, command: UiCommand) -> Result<(), Box<dyn Error>> {
        match (self, command) {
            (
                Self::Setup {
                    command_publisher, ..
                },
                UiCommand::Setup(payload),
            ) => {
                command_publisher.send_copy(SetupCommandMessage::new(&payload))?;
                Ok(())
            }
            (
                Self::Collect {
                    command_publisher, ..
                },
                UiCommand::Episode(cmd),
            ) => {
                command_publisher.send_copy(cmd)?;
                Ok(())
            }
            (Self::Setup { .. }, UiCommand::Episode(_)) => {
                log::debug!("ignoring episode command in setup-mode control server");
                Ok(())
            }
            (Self::Collect { .. }, UiCommand::Setup(_)) => {
                log::debug!("ignoring setup command in collect-mode control server");
                Ok(())
            }
        }
    }

    fn poll_once(
        &mut self,
        outbound_tx: &tokio::sync::broadcast::Sender<OutboundMessage>,
        latest_snapshot: &Arc<std::sync::Mutex<Option<String>>>,
    ) -> Result<(), Box<dyn Error>> {
        match self {
            Self::Setup {
                state_subscriber, ..
            } => {
                let mut latest: Option<String> = None;
                while let Some(sample) = state_subscriber.receive()? {
                    latest = Some(sample.payload().as_str().to_owned());
                }
                if let Some(json) = latest {
                    if let Ok(mut slot) = latest_snapshot.lock() {
                        *slot = Some(json.clone());
                    }
                    let _ = outbound_tx.send(OutboundMessage::new(json));
                }
            }
            Self::Collect {
                status_subscriber,
                backpressure_subscriber,
                ..
            } => {
                let mut latest_status: Option<EpisodeStatus> = None;
                while let Some(sample) = status_subscriber.receive()? {
                    latest_status = Some(*sample.payload());
                }
                if let Some(status) = latest_status {
                    let json = protocol::encode_episode_status(&status);
                    if let Ok(mut slot) = latest_snapshot.lock() {
                        *slot = Some(json.clone());
                    }
                    let _ = outbound_tx.send(OutboundMessage::new(json));
                }

                while let Some(sample) = backpressure_subscriber.receive()? {
                    let json = protocol::encode_backpressure(sample.payload());
                    let _ = outbound_tx.send(OutboundMessage::new(json));
                }
            }
        }
        Ok(())
    }
}
