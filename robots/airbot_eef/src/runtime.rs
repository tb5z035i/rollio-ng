use crate::DriverProfile;
use airbot_play_rust::can::router::CanFrameRouter;
use airbot_play_rust::can::worker::{CanTxPriority, CanWorker, CanWorkerBackend, CanWorkerConfig};
use airbot_play_rust::eef::{
    EefRuntime, EefRuntimeError, EefState, SingleEefCommand, SingleEefFeedback,
    spawn_eef_runtime_task,
};
use async_trait::async_trait;
use iceoryx2::prelude::*;
use rollio_bus::{CONTROL_EVENTS_SERVICE, robot_command_service_name, robot_state_service_name};
use rollio_types::config::RobotMode;
use rollio_types::messages::{
    CommandMode, ControlEvent, EndEffectorStatus, RobotCommand, RobotState,
};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};

type StatePublisher = iceoryx2::port::publisher::Publisher<ipc::Service, RobotState, ()>;
type CommandSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotCommand, ()>;
type ControlSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;

const EEF_DOF: u32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct RollioRuntimeConfig {
    pub device_name: String,
    pub interface: String,
    pub initial_mode: RobotMode,
    pub publish_rate_hz: f64,
    pub profile: DriverProfile,
    pub can_backend: CanWorkerBackend,
    pub mit_kp: f64,
    pub mit_kd: f64,
    pub command_velocity: f64,
    pub command_effort: f64,
    pub current_threshold: f64,
}

#[derive(Debug, Error)]
pub enum RollioRuntimeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("CAN worker error: {0}")]
    CanWorker(#[from] airbot_play_rust::can::worker::CanWorkerError),
    #[error("end-effector runtime error: {0}")]
    Eef(#[from] EefRuntimeError),
    #[error("publish_rate_hz must be a positive finite number, got {0}")]
    InvalidPublishRate(f64),
    #[error("invalid command parameter {field}: {value}")]
    InvalidCommandParameter { field: &'static str, value: f64 },
    #[error("transport setup error: {0}")]
    TransportSetup(String),
    #[error("iceoryx2 error: {0}")]
    Ipc(String),
}

#[derive(Debug, Default)]
struct ControlFlowState {
    next_mode: Option<RobotMode>,
    shutdown: bool,
}

#[derive(Debug)]
struct StandaloneEefClient {
    worker: Arc<CanWorker>,
    frame_router: Arc<CanFrameRouter>,
    runtime_task: Mutex<Option<JoinHandle<()>>>,
    eef: Arc<EefRuntime>,
    status: RwLock<EndEffectorStatus>,
}

impl StandaloneEefClient {
    fn connect(config: &RollioRuntimeConfig) -> Result<Self, RollioRuntimeError> {
        let worker = CanWorker::open(CanWorkerConfig {
            interface: config.interface.clone(),
            backend: config.can_backend,
            ..CanWorkerConfig::default()
        })?;

        let eef = Arc::new(EefRuntime::new(config.profile.mounted_eef()));
        let (frame_router, routes) =
            CanFrameRouter::new(Arc::clone(&worker), std::iter::empty::<u16>(), Some(7));
        frame_router
            .start()
            .map_err(|err| RollioRuntimeError::TransportSetup(err.to_string()))?;
        let runtime_task =
            spawn_eef_runtime_task(routes.eef_rx, Arc::clone(&eef), Arc::clone(&worker));

        Ok(Self {
            worker,
            frame_router,
            runtime_task: Mutex::new(Some(runtime_task)),
            eef,
            status: RwLock::new(EndEffectorStatus::Disabled),
        })
    }
}

#[async_trait(?Send)]
trait RuntimeTransport: Send + Sync {
    fn latest_feedback(&self) -> Option<SingleEefFeedback>;
    fn current_status(&self) -> EndEffectorStatus;
    async fn set_state(&self, state: EefState) -> Result<(), RollioRuntimeError>;
    async fn submit_e2_command(&self, command: &SingleEefCommand)
    -> Result<(), RollioRuntimeError>;
    async fn submit_g2_command(&self, command: &SingleEefCommand)
    -> Result<(), RollioRuntimeError>;
    async fn shutdown_gracefully(&self) -> Result<(), RollioRuntimeError>;
}

#[async_trait(?Send)]
impl RuntimeTransport for StandaloneEefClient {
    fn latest_feedback(&self) -> Option<SingleEefFeedback> {
        self.eef.latest_feedback()
    }

    fn current_status(&self) -> EndEffectorStatus {
        *self.status.read().expect("EEF status lock poisoned")
    }

    async fn set_state(&self, state: EefState) -> Result<(), RollioRuntimeError> {
        let frames = self.eef.set_state(state)?;
        if !frames.is_empty() {
            self.worker
                .send_frames(CanTxPriority::Lifecycle, frames)
                .await?;
        }
        let status = match state {
            EefState::Disabled => EndEffectorStatus::Disabled,
            EefState::Enabled => EndEffectorStatus::Enabled,
        };
        *self.status.write().expect("EEF status lock poisoned") = status;
        Ok(())
    }

    async fn submit_e2_command(
        &self,
        command: &SingleEefCommand,
    ) -> Result<(), RollioRuntimeError> {
        let frames = self.eef.build_e2_command(command)?;
        if !frames.is_empty() {
            self.worker
                .send_frames(CanTxPriority::Control, frames)
                .await?;
        }
        Ok(())
    }

    async fn submit_g2_command(
        &self,
        command: &SingleEefCommand,
    ) -> Result<(), RollioRuntimeError> {
        self.eef.submit_g2_mit_target(command)?;
        Ok(())
    }

    async fn shutdown_gracefully(&self) -> Result<(), RollioRuntimeError> {
        self.frame_router.stop();
        if let Some(task) = self
            .runtime_task
            .lock()
            .expect("EEF runtime task lock poisoned")
            .take()
        {
            task.abort();
        }

        let frames = self.eef.shutdown_frames()?;
        if !frames.is_empty() {
            self.worker
                .send_frames(CanTxPriority::Lifecycle, frames)
                .await?;
        }
        self.worker.shutdown().await;
        Ok(())
    }
}

pub async fn run_rollio_runtime(config: RollioRuntimeConfig) -> Result<(), RollioRuntimeError> {
    let transport = Arc::new(StandaloneEefClient::connect(&config)?);
    run_rollio_runtime_with_transport(config, transport).await
}

async fn run_rollio_runtime_with_transport<T>(
    config: RollioRuntimeConfig,
    transport: Arc<T>,
) -> Result<(), RollioRuntimeError>
where
    T: RuntimeTransport + 'static,
{
    validate_config(&config)?;

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .map_err(map_iceoryx_error)?;

    let (state_publisher, command_subscriber, control_subscriber) =
        open_ports(&node, &config.device_name)?;

    let mut ticker = interval(Duration::from_secs_f64(1.0 / config.publish_rate_hz));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut current_mode = config.initial_mode;
    transport.set_state(EefState::Enabled).await?;

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);

    let run_result = async {
        loop {
            tokio::select! {
                shutdown_result = &mut shutdown => {
                    shutdown_result?;
                    break;
                }
                _ = ticker.tick() => {
                    let control_flow = drain_control_events(&control_subscriber)?;
                    if control_flow.shutdown {
                        break;
                    }

                    if let Some(next_mode) = control_flow.next_mode {
                        current_mode = next_mode;
                    }

                    if current_mode == RobotMode::CommandFollowing {
                        if let Some(command) = drain_latest_command(&command_subscriber)? {
                            let target = build_target_command(&command, &config)?;
                            match config.profile {
                                DriverProfile::E2 => transport.submit_e2_command(&target).await?,
                                DriverProfile::G2 => transport.submit_g2_command(&target).await?,
                            }
                        }
                    } else {
                        let _ = drain_latest_command(&command_subscriber)?;
                    }

                    if let Some(feedback) = transport.latest_feedback() {
                        publish_robot_state(
                            &state_publisher,
                            &feedback,
                            transport.current_status(),
                        )?;
                    }
                }
            }
        }

        Ok::<(), RollioRuntimeError>(())
    }
    .await;

    let shutdown_result = transport.shutdown_gracefully().await;
    run_result?;
    shutdown_result?;
    Ok(())
}

fn validate_config(config: &RollioRuntimeConfig) -> Result<(), RollioRuntimeError> {
    if !config.publish_rate_hz.is_finite() || config.publish_rate_hz <= 0.0 {
        return Err(RollioRuntimeError::InvalidPublishRate(
            config.publish_rate_hz,
        ));
    }

    for (field, value) in [
        ("mit_kp", config.mit_kp),
        ("mit_kd", config.mit_kd),
        ("command_velocity", config.command_velocity),
        ("command_effort", config.command_effort),
        ("current_threshold", config.current_threshold),
    ] {
        if !value.is_finite() {
            return Err(RollioRuntimeError::InvalidCommandParameter { field, value });
        }
    }

    Ok(())
}

fn open_ports(
    node: &Node<ipc::Service>,
    device_name: &str,
) -> Result<(StatePublisher, CommandSubscriber, ControlSubscriber), RollioRuntimeError> {
    let state_service_name: ServiceName = robot_state_service_name(device_name)
        .as_str()
        .try_into()
        .map_err(map_iceoryx_error)?;
    let state_service = node
        .service_builder(&state_service_name)
        .publish_subscribe::<RobotState>()
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let state_publisher = state_service
        .publisher_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let command_service_name: ServiceName = robot_command_service_name(device_name)
        .as_str()
        .try_into()
        .map_err(map_iceoryx_error)?;
    let command_service = node
        .service_builder(&command_service_name)
        .publish_subscribe::<RobotCommand>()
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let command_subscriber = command_service
        .subscriber_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE
        .try_into()
        .map_err(map_iceoryx_error)?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()
        .map_err(map_iceoryx_error)?;
    let control_subscriber = control_service
        .subscriber_builder()
        .create()
        .map_err(map_iceoryx_error)?;

    Ok((state_publisher, command_subscriber, control_subscriber))
}

fn drain_control_events(
    subscriber: &ControlSubscriber,
) -> Result<ControlFlowState, RollioRuntimeError> {
    let mut state = ControlFlowState::default();
    loop {
        match subscriber.receive().map_err(map_iceoryx_error)? {
            Some(sample) => match *sample.payload() {
                ControlEvent::ModeSwitch { target_mode } => {
                    if let Some(mode) = RobotMode::from_control_mode_value(target_mode) {
                        state.next_mode = Some(mode);
                    }
                }
                ControlEvent::Shutdown => {
                    state.shutdown = true;
                    return Ok(state);
                }
                _ => {}
            },
            None => return Ok(state),
        }
    }
}

fn drain_latest_command(
    subscriber: &CommandSubscriber,
) -> Result<Option<RobotCommand>, RollioRuntimeError> {
    let mut latest = None;
    loop {
        match subscriber.receive().map_err(map_iceoryx_error)? {
            Some(sample) => latest = Some(*sample.payload()),
            None => return Ok(latest),
        }
    }
}

fn build_target_command(
    command: &RobotCommand,
    config: &RollioRuntimeConfig,
) -> Result<SingleEefCommand, RollioRuntimeError> {
    let position = match command.mode {
        CommandMode::Joint => {
            if command.num_joints == 0 {
                0.0
            } else {
                command.joint_targets[0]
            }
        }
        CommandMode::Cartesian => command.cartesian_target[0],
    };

    if !position.is_finite() {
        return Err(RollioRuntimeError::InvalidCommandParameter {
            field: "position",
            value: position,
        });
    }

    Ok(SingleEefCommand {
        position,
        velocity: config.command_velocity,
        effort: config.command_effort,
        mit_kp: config.mit_kp,
        mit_kd: config.mit_kd,
        current_threshold: config.current_threshold,
    })
}

fn publish_robot_state(
    publisher: &StatePublisher,
    feedback: &SingleEefFeedback,
    status: EndEffectorStatus,
) -> Result<(), RollioRuntimeError> {
    let mut state = RobotState {
        timestamp_ns: unix_timestamp_ns(),
        num_joints: EEF_DOF,
        ..RobotState::default()
    };
    state.positions[0] = feedback.position;
    state.velocities[0] = feedback.velocity;
    state.efforts[0] = feedback.effort;
    state.end_effector_status = status;
    state.has_end_effector_status = true;
    state.end_effector_feedback_valid = feedback.valid;
    publisher.send_copy(state).map_err(map_iceoryx_error)?;
    Ok(())
}

fn map_iceoryx_error(error: impl std::fmt::Display) -> RollioRuntimeError {
    RollioRuntimeError::Ipc(error.to_string())
}

fn unix_timestamp_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

async fn shutdown_signal() -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::time::{sleep, timeout};

    struct TestPorts {
        _node: Node<ipc::Service>,
        state_subscriber: StatePublisherSubscriber,
        command_publisher: CommandPublisher,
        control_publisher: ControlPublisher,
    }

    type StatePublisherSubscriber =
        iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotState, ()>;
    type CommandPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, RobotCommand, ()>;
    type ControlPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>;

    #[derive(Default)]
    struct FakeTransport {
        feedback: RwLock<Option<SingleEefFeedback>>,
        status: RwLock<EndEffectorStatus>,
        e2_commands: Mutex<Vec<SingleEefCommand>>,
        g2_commands: Mutex<Vec<SingleEefCommand>>,
        shutdown_called: AtomicBool,
    }

    #[async_trait(?Send)]
    impl RuntimeTransport for FakeTransport {
        fn latest_feedback(&self) -> Option<SingleEefFeedback> {
            self.feedback
                .read()
                .expect("feedback lock poisoned")
                .clone()
        }

        fn current_status(&self) -> EndEffectorStatus {
            *self.status.read().expect("status lock poisoned")
        }

        async fn set_state(&self, state: EefState) -> Result<(), RollioRuntimeError> {
            let status = match state {
                EefState::Disabled => EndEffectorStatus::Disabled,
                EefState::Enabled => EndEffectorStatus::Enabled,
            };
            *self.status.write().expect("status lock poisoned") = status;
            Ok(())
        }

        async fn submit_e2_command(
            &self,
            command: &SingleEefCommand,
        ) -> Result<(), RollioRuntimeError> {
            self.e2_commands
                .lock()
                .expect("e2 commands lock poisoned")
                .push(command.clone());
            Ok(())
        }

        async fn submit_g2_command(
            &self,
            command: &SingleEefCommand,
        ) -> Result<(), RollioRuntimeError> {
            self.g2_commands
                .lock()
                .expect("g2 commands lock poisoned")
                .push(command.clone());
            Ok(())
        }

        async fn shutdown_gracefully(&self) -> Result<(), RollioRuntimeError> {
            self.shutdown_called.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn runtime_config(
        profile: DriverProfile,
        device_name: &str,
        mode: RobotMode,
    ) -> RollioRuntimeConfig {
        RollioRuntimeConfig {
            device_name: device_name.to_owned(),
            interface: "can0".to_owned(),
            initial_mode: mode,
            publish_rate_hz: 80.0,
            profile,
            can_backend: CanWorkerBackend::AsyncFd,
            mit_kp: profile.default_mit_kp(),
            mit_kd: profile.default_mit_kd(),
            command_velocity: 0.0,
            command_effort: 0.0,
            current_threshold: 0.0,
        }
    }

    fn fake_feedback() -> SingleEefFeedback {
        SingleEefFeedback {
            position: 0.042,
            velocity: -0.1,
            effort: 1.25,
            valid: true,
            timestamp_millis: 1,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publishes_robot_state_with_end_effector_status()
    -> Result<(), Box<dyn std::error::Error>> {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let _guard = test_guard();
                let device_name = unique_name("eef_state");
                let ports = create_test_ports(&device_name)?;
                let transport = Arc::new(FakeTransport::default());
                *transport.feedback.write().expect("feedback lock poisoned") =
                    Some(fake_feedback());

                let task = tokio::task::spawn_local(run_rollio_runtime_with_transport(
                    runtime_config(DriverProfile::G2, &device_name, RobotMode::FreeDrive),
                    Arc::clone(&transport),
                ));

                let state = receive_state(&ports.state_subscriber).await?;
                assert_eq!(state.num_joints, 1);
                assert_eq!(state.positions[0], 0.042);
                assert_eq!(state.velocities[0], -0.1);
                assert_eq!(state.efforts[0], 1.25);
                assert!(state.has_end_effector_status);
                assert_eq!(state.end_effector_status, EndEffectorStatus::Enabled);
                assert!(state.end_effector_feedback_valid);

                send_control_event(&ports.control_publisher, ControlEvent::Shutdown)?;
                task.await??;
                assert!(transport.shutdown_called.load(Ordering::SeqCst));
                Ok::<(), Box<dyn std::error::Error>>(())
            })
            .await
    }

    #[tokio::test(flavor = "current_thread")]
    async fn e2_command_following_uses_e2_submit_path() -> Result<(), Box<dyn std::error::Error>> {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let _guard = test_guard();
                let device_name = unique_name("eef_e2");
                let ports = create_test_ports(&device_name)?;
                let transport = Arc::new(FakeTransport::default());
                *transport.feedback.write().expect("feedback lock poisoned") =
                    Some(fake_feedback());

                let task = tokio::task::spawn_local(run_rollio_runtime_with_transport(
                    runtime_config(DriverProfile::E2, &device_name, RobotMode::FreeDrive),
                    Arc::clone(&transport),
                ));

                let _warmup = receive_state(&ports.state_subscriber).await?;
                send_control_event(
                    &ports.control_publisher,
                    ControlEvent::ModeSwitch {
                        target_mode: RobotMode::CommandFollowing.control_mode_value(),
                    },
                )?;
                send_joint_command(&ports.command_publisher, 0.018)?;

                timeout(Duration::from_secs(2), async {
                    loop {
                        if !transport
                            .e2_commands
                            .lock()
                            .expect("e2 commands lock poisoned")
                            .is_empty()
                        {
                            break;
                        }
                        sleep(Duration::from_millis(10)).await;
                    }
                })
                .await?;

                assert_eq!(
                    transport
                        .e2_commands
                        .lock()
                        .expect("e2 commands lock poisoned")
                        .last()
                        .expect("E2 command should be recorded")
                        .position,
                    0.018
                );
                assert!(
                    transport
                        .g2_commands
                        .lock()
                        .expect("g2 commands lock poisoned")
                        .is_empty()
                );

                send_control_event(&ports.control_publisher, ControlEvent::Shutdown)?;
                task.await??;
                Ok::<(), Box<dyn std::error::Error>>(())
            })
            .await
    }

    #[tokio::test(flavor = "current_thread")]
    async fn g2_command_following_uses_g2_submit_path() -> Result<(), Box<dyn std::error::Error>> {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let _guard = test_guard();
                let device_name = unique_name("eef_g2");
                let ports = create_test_ports(&device_name)?;
                let transport = Arc::new(FakeTransport::default());
                *transport.feedback.write().expect("feedback lock poisoned") =
                    Some(fake_feedback());

                let task = tokio::task::spawn_local(run_rollio_runtime_with_transport(
                    runtime_config(DriverProfile::G2, &device_name, RobotMode::FreeDrive),
                    Arc::clone(&transport),
                ));

                let _warmup = receive_state(&ports.state_subscriber).await?;
                send_control_event(
                    &ports.control_publisher,
                    ControlEvent::ModeSwitch {
                        target_mode: RobotMode::CommandFollowing.control_mode_value(),
                    },
                )?;
                send_joint_command(&ports.command_publisher, 0.057)?;

                timeout(Duration::from_secs(2), async {
                    loop {
                        if !transport
                            .g2_commands
                            .lock()
                            .expect("g2 commands lock poisoned")
                            .is_empty()
                        {
                            break;
                        }
                        sleep(Duration::from_millis(10)).await;
                    }
                })
                .await?;

                assert_eq!(
                    transport
                        .g2_commands
                        .lock()
                        .expect("g2 commands lock poisoned")
                        .last()
                        .expect("G2 command should be recorded")
                        .position,
                    0.057
                );
                assert!(
                    transport
                        .e2_commands
                        .lock()
                        .expect("e2 commands lock poisoned")
                        .is_empty()
                );

                send_control_event(&ports.control_publisher, ControlEvent::Shutdown)?;
                task.await??;
                Ok::<(), Box<dyn std::error::Error>>(())
            })
            .await
    }

    fn create_test_ports(device_name: &str) -> Result<TestPorts, Box<dyn std::error::Error>> {
        let node = NodeBuilder::new()
            .signal_handling_mode(SignalHandlingMode::Disabled)
            .create::<ipc::Service>()?;

        let state_service_name: ServiceName =
            robot_state_service_name(device_name).as_str().try_into()?;
        let state_service = node
            .service_builder(&state_service_name)
            .publish_subscribe::<RobotState>()
            .open_or_create()?;
        let state_subscriber = state_service.subscriber_builder().create()?;

        let command_service_name: ServiceName = robot_command_service_name(device_name)
            .as_str()
            .try_into()?;
        let command_service = node
            .service_builder(&command_service_name)
            .publish_subscribe::<RobotCommand>()
            .open_or_create()?;
        let command_publisher = command_service.publisher_builder().create()?;

        let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
        let control_service = node
            .service_builder(&control_service_name)
            .publish_subscribe::<ControlEvent>()
            .open_or_create()?;
        let control_publisher = control_service.publisher_builder().create()?;

        Ok(TestPorts {
            _node: node,
            state_subscriber,
            command_publisher,
            control_publisher,
        })
    }

    async fn receive_state(
        subscriber: &StatePublisherSubscriber,
    ) -> Result<RobotState, Box<dyn std::error::Error>> {
        let state = timeout(Duration::from_secs(2), async {
            loop {
                if let Some(sample) = subscriber.receive().map_err(map_iceoryx_error)? {
                    break Ok::<RobotState, RollioRuntimeError>(*sample.payload());
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await??;
        Ok(state)
    }

    fn send_joint_command(
        publisher: &CommandPublisher,
        position: f64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut command = RobotCommand {
            mode: CommandMode::Joint,
            num_joints: 1,
            ..RobotCommand::default()
        };
        command.joint_targets[0] = position;
        publisher.send_copy(command)?;
        Ok(())
    }

    fn send_control_event(
        publisher: &ControlPublisher,
        event: ControlEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        publisher.send_copy(event)?;
        Ok(())
    }

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn unique_name(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{prefix}_{nanos}")
    }
}
