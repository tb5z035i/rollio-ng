use airbot_play_rust::arm::{ArmJointFeedback, ArmState, ARM_DOF};
use airbot_play_rust::can::worker::CanWorkerBackend;
use airbot_play_rust::client::{AirbotPlayClient, ClientError};
use airbot_play_rust::model::{ModelBackendKind, Pose};
use async_trait::async_trait;
use iceoryx2::prelude::*;
use rollio_bus::{robot_command_service_name, robot_state_service_name, CONTROL_EVENTS_SERVICE};
use rollio_types::config::RobotMode;
use rollio_types::messages::{CommandMode, ControlEvent, RobotCommand, RobotState, MAX_JOINTS};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::time::{interval, MissedTickBehavior};

type StatePublisher = iceoryx2::port::publisher::Publisher<ipc::Service, RobotState, ()>;
type CommandSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotCommand, ()>;
type ControlSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>;

#[derive(Clone, Debug, PartialEq)]
pub struct RollioRuntimeConfig {
    pub device_name: String,
    pub interface: String,
    pub dof: usize,
    pub initial_mode: RobotMode,
    pub publish_rate_hz: f64,
    pub can_backend: CanWorkerBackend,
    pub model_backend: ModelBackendKind,
}

impl Default for RollioRuntimeConfig {
    fn default() -> Self {
        Self {
            device_name: "airbot".to_owned(),
            interface: "can0".to_owned(),
            dof: ARM_DOF,
            initial_mode: RobotMode::FreeDrive,
            publish_rate_hz: 250.0,
            can_backend: CanWorkerBackend::AsyncFd,
            model_backend: ModelBackendKind::PlayAnalytical,
        }
    }
}

#[derive(Debug, Error)]
pub enum RollioRuntimeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("client error: {0}")]
    Client(#[from] ClientError),
    #[error("invalid AIRBOT dof {0}, expected 1..=6")]
    InvalidDof(usize),
    #[error("publish_rate_hz must be a positive finite number, got {0}")]
    InvalidPublishRate(f64),
    #[error("invalid cartesian target: {0}")]
    InvalidCartesianTarget(String),
    #[error("iceoryx2 error: {0}")]
    Ipc(String),
}

#[derive(Debug, Default)]
struct ControlFlowState {
    next_mode: Option<RobotMode>,
    shutdown: bool,
}

#[async_trait(?Send)]
pub trait RuntimeClient: Send + Sync {
    fn latest_feedback(&self) -> Option<ArmJointFeedback>;
    async fn set_arm_state(&self, state: ArmState) -> Result<(), ClientError>;
    fn submit_joint_target(&self, positions: [f64; ARM_DOF]) -> Result<(), ClientError>;
    fn submit_task_target(&self, pose: &Pose) -> Result<(), ClientError>;
    fn query_current_pose(&self) -> Result<Pose, ClientError>;
    async fn shutdown_gracefully(&self) -> Result<(), ClientError>;
}

#[async_trait(?Send)]
impl RuntimeClient for AirbotPlayClient {
    fn latest_feedback(&self) -> Option<ArmJointFeedback> {
        self.arm().latest_feedback()
    }

    async fn set_arm_state(&self, state: ArmState) -> Result<(), ClientError> {
        AirbotPlayClient::set_arm_state(self, state).await
    }

    fn submit_joint_target(&self, positions: [f64; ARM_DOF]) -> Result<(), ClientError> {
        AirbotPlayClient::submit_joint_target(self, positions).map(|_| ())
    }

    fn submit_task_target(&self, pose: &Pose) -> Result<(), ClientError> {
        AirbotPlayClient::submit_task_target(self, pose).map(|_| ())
    }

    fn query_current_pose(&self) -> Result<Pose, ClientError> {
        AirbotPlayClient::query_current_pose(self)
    }

    async fn shutdown_gracefully(&self) -> Result<(), ClientError> {
        AirbotPlayClient::shutdown_gracefully(self).await
    }
}

pub async fn run_rollio_runtime(config: RollioRuntimeConfig) -> Result<(), RollioRuntimeError> {
    let client = Arc::new(
        AirbotPlayClient::connect_control_with_backends(
            config.interface.clone(),
            config.can_backend,
            config.model_backend,
        )
        .await?,
    );

    run_rollio_runtime_with_client(config, client).await
}

pub async fn run_rollio_runtime_with_client<C>(
    config: RollioRuntimeConfig,
    client: Arc<C>,
) -> Result<(), RollioRuntimeError>
where
    C: RuntimeClient + 'static,
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
    client
        .set_arm_state(arm_state_for_mode(current_mode))
        .await?;

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
                        if next_mode != current_mode {
                            client.set_arm_state(arm_state_for_mode(next_mode)).await?;
                            current_mode = next_mode;
                        }
                    }

                    if current_mode == RobotMode::CommandFollowing {
                        if let Some(command) = drain_latest_command(&command_subscriber)? {
                            if matches!(command.mode, CommandMode::Cartesian) {
                                let pose = Pose::from_slice(&command.cartesian_target).map_err(|error| {
                                    RollioRuntimeError::InvalidCartesianTarget(error.to_string())
                                })?;
                                client.submit_task_target(&pose)?;
                            } else {
                                let targets = command_targets(&command, config.dof);
                                client.submit_joint_target(targets)?;
                            }
                        }
                    } else {
                        let _ = drain_latest_command(&command_subscriber)?;
                    }

                    if let Some(feedback) = client.latest_feedback() {
                        publish_robot_state(
                            &state_publisher,
                            config.dof,
                            &feedback,
                            client.query_current_pose().ok(),
                        )?;
                    }
                }
            }
        }

        Ok::<(), RollioRuntimeError>(())
    }
    .await;

    let shutdown_result = client.shutdown_gracefully().await;
    run_result?;
    shutdown_result?;
    Ok(())
}

fn validate_config(config: &RollioRuntimeConfig) -> Result<(), RollioRuntimeError> {
    if config.dof == 0 || config.dof > ARM_DOF {
        return Err(RollioRuntimeError::InvalidDof(config.dof));
    }
    if !config.publish_rate_hz.is_finite() || config.publish_rate_hz <= 0.0 {
        return Err(RollioRuntimeError::InvalidPublishRate(
            config.publish_rate_hz,
        ));
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

fn publish_robot_state(
    publisher: &StatePublisher,
    dof: usize,
    feedback: &ArmJointFeedback,
    pose: Option<Pose>,
) -> Result<(), RollioRuntimeError> {
    if !feedback.valid {
        return Ok(());
    }

    let mut state = RobotState {
        timestamp_ns: unix_timestamp_ns(),
        num_joints: dof as u32,
        ..RobotState::default()
    };

    for joint_idx in 0..dof {
        state.positions[joint_idx] = feedback.positions[joint_idx];
        state.velocities[joint_idx] = feedback.velocities[joint_idx];
        state.efforts[joint_idx] = feedback.torques[joint_idx];
    }

    if let Some(pose) = pose {
        let pose_values = pose.as_vec();
        state.ee_pose.copy_from_slice(&pose_values);
        state.has_ee_pose = true;
    }

    publisher.send_copy(state).map_err(map_iceoryx_error)?;
    Ok(())
}

fn command_targets(command: &RobotCommand, dof: usize) -> [f64; ARM_DOF] {
    let active_joints = (command.num_joints as usize)
        .min(dof)
        .min(ARM_DOF)
        .min(MAX_JOINTS);
    let mut targets = [0.0; ARM_DOF];

    match command.mode {
        CommandMode::Joint => {
            targets[..active_joints].copy_from_slice(&command.joint_targets[..active_joints]);
        }
        CommandMode::Cartesian => {
            let cartesian_joints = active_joints.min(command.cartesian_target.len());
            targets[..cartesian_joints]
                .copy_from_slice(&command.cartesian_target[..cartesian_joints]);
        }
    }

    targets
}

fn arm_state_for_mode(mode: RobotMode) -> ArmState {
    match mode {
        RobotMode::FreeDrive => ArmState::FreeDrive,
        RobotMode::CommandFollowing => ArmState::CommandFollowing,
    }
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
    use airbot_play_rust::arm::PlayArmError;
    use std::error::Error;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock, RwLock};
    use tokio::time::{sleep, timeout};

    struct TestPorts {
        _node: Node<ipc::Service>,
        state_subscriber: StateSubscriber,
        command_publisher: CommandPublisher,
        control_publisher: ControlPublisher,
    }

    type StateSubscriber = iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotState, ()>;
    type CommandPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, RobotCommand, ()>;
    type ControlPublisher = iceoryx2::port::publisher::Publisher<ipc::Service, ControlEvent, ()>;

    #[derive(Default)]
    struct FakeClient {
        latest_feedback: RwLock<Option<ArmJointFeedback>>,
        pose: RwLock<Option<Pose>>,
        state_calls: Mutex<Vec<ArmState>>,
        targets: Mutex<Vec<[f64; ARM_DOF]>>,
        task_targets: Mutex<Vec<[f64; 7]>>,
        shutdown_called: AtomicBool,
    }

    impl FakeClient {
        fn with_feedback(feedback: ArmJointFeedback) -> Self {
            Self {
                latest_feedback: RwLock::new(Some(feedback)),
                pose: RwLock::new(None),
                state_calls: Mutex::new(Vec::new()),
                targets: Mutex::new(Vec::new()),
                task_targets: Mutex::new(Vec::new()),
                shutdown_called: AtomicBool::new(false),
            }
        }

        fn set_pose(&self, pose: Pose) {
            *self.pose.write().expect("pose lock poisoned") = Some(pose);
        }
    }

    #[async_trait(?Send)]
    impl RuntimeClient for FakeClient {
        fn latest_feedback(&self) -> Option<ArmJointFeedback> {
            self.latest_feedback
                .read()
                .expect("feedback lock poisoned")
                .clone()
        }

        async fn set_arm_state(&self, state: ArmState) -> Result<(), ClientError> {
            self.state_calls
                .lock()
                .expect("state calls lock poisoned")
                .push(state);
            Ok(())
        }

        fn submit_joint_target(&self, positions: [f64; ARM_DOF]) -> Result<(), ClientError> {
            self.targets
                .lock()
                .expect("targets lock poisoned")
                .push(positions);
            Ok(())
        }

        fn submit_task_target(&self, pose: &Pose) -> Result<(), ClientError> {
            let pose_values: [f64; 7] = pose
                .as_vec()
                .try_into()
                .expect("pose should serialize to 7 values");
            self.task_targets
                .lock()
                .expect("task targets lock poisoned")
                .push(pose_values);
            Ok(())
        }

        fn query_current_pose(&self) -> Result<Pose, ClientError> {
            self.pose
                .read()
                .expect("pose lock poisoned")
                .clone()
                .ok_or(ClientError::Arm(PlayArmError::MissingFeedback))
        }

        async fn shutdown_gracefully(&self) -> Result<(), ClientError> {
            self.shutdown_called.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publishes_states_and_shuts_down_cleanly() -> Result<(), Box<dyn Error>> {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let _guard = test_guard();
                let device_name = unique_name("airbot_state");
                let ports = create_test_ports(&device_name)?;
                let feedback = ArmJointFeedback {
                    positions: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
                    velocities: [0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
                    torques: [0.6, 0.5, 0.4, 0.3, 0.2, 0.1],
                    valid: true,
                    timestamp_millis: 1,
                };
                let client = Arc::new(FakeClient::with_feedback(feedback));
                client.set_pose(Pose::from_slice(&[0.1, 0.2, 0.3, 0.0, 0.0, 0.0, 1.0])?);

                let task = tokio::task::spawn_local(run_rollio_runtime_with_client(
                    RollioRuntimeConfig {
                        device_name: device_name.clone(),
                        interface: "can0".to_owned(),
                        dof: 6,
                        initial_mode: RobotMode::FreeDrive,
                        publish_rate_hz: 50.0,
                        can_backend: CanWorkerBackend::AsyncFd,
                        model_backend: ModelBackendKind::PlayAnalytical,
                    },
                    Arc::clone(&client),
                ));

                let state = receive_state(&ports.state_subscriber).await?;
                assert_eq!(state.num_joints, 6);
                assert_eq!(state.positions[0], 1.0);
                assert_eq!(state.velocities[5], 0.6);
                assert_eq!(state.efforts[0], 0.6);
                assert!(state.has_ee_pose);
                assert_eq!(state.ee_pose[0], 0.1);
                assert_eq!(state.ee_pose[6], 1.0);

                send_control_event(&ports.control_publisher, ControlEvent::Shutdown)?;
                task.await??;
                assert!(client.shutdown_called.load(Ordering::SeqCst));
                Ok::<(), Box<dyn Error>>(())
            })
            .await
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mode_switch_and_joint_commands_submit_targets() -> Result<(), Box<dyn Error>> {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let _guard = test_guard();
                let device_name = unique_name("airbot_joint");
                let ports = create_test_ports(&device_name)?;
                let feedback = ArmJointFeedback {
                    positions: [0.0; ARM_DOF],
                    velocities: [0.0; ARM_DOF],
                    torques: [0.0; ARM_DOF],
                    valid: true,
                    timestamp_millis: 1,
                };
                let client = Arc::new(FakeClient::with_feedback(feedback));

                let task = tokio::task::spawn_local(run_rollio_runtime_with_client(
                    RollioRuntimeConfig {
                        device_name: device_name.clone(),
                        interface: "can0".to_owned(),
                        dof: 6,
                        initial_mode: RobotMode::FreeDrive,
                        publish_rate_hz: 80.0,
                        can_backend: CanWorkerBackend::AsyncFd,
                        model_backend: ModelBackendKind::PlayAnalytical,
                    },
                    Arc::clone(&client),
                ));

                let _warmup = receive_state(&ports.state_subscriber).await?;
                send_control_event(
                    &ports.control_publisher,
                    ControlEvent::ModeSwitch {
                        target_mode: RobotMode::CommandFollowing.control_mode_value(),
                    },
                )?;
                send_joint_command(&ports.command_publisher, [1.0, 1.1, 1.2, 1.3, 1.4, 1.5])?;

                timeout(Duration::from_secs(2), async {
                    loop {
                        if !client
                            .targets
                            .lock()
                            .expect("targets lock poisoned")
                            .is_empty()
                        {
                            break;
                        }
                        sleep(Duration::from_millis(10)).await;
                    }
                })
                .await?;

                let state_calls = client
                    .state_calls
                    .lock()
                    .expect("state calls lock poisoned");
                assert_eq!(state_calls[0], ArmState::FreeDrive);
                assert!(state_calls.contains(&ArmState::CommandFollowing));
                drop(state_calls);

                let targets = client.targets.lock().expect("targets lock poisoned");
                assert_eq!(
                    targets.last().copied(),
                    Some([1.0, 1.1, 1.2, 1.3, 1.4, 1.5])
                );
                drop(targets);

                send_control_event(&ports.control_publisher, ControlEvent::Shutdown)?;
                task.await??;
                Ok::<(), Box<dyn Error>>(())
            })
            .await
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cartesian_commands_use_task_target_api() -> Result<(), Box<dyn Error>> {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let _guard = test_guard();
                let device_name = unique_name("airbot_cartesian");
                let ports = create_test_ports(&device_name)?;
                let feedback = ArmJointFeedback {
                    positions: [0.0; ARM_DOF],
                    velocities: [0.0; ARM_DOF],
                    torques: [0.0; ARM_DOF],
                    valid: true,
                    timestamp_millis: 1,
                };
                let client = Arc::new(FakeClient::with_feedback(feedback));

                let task = tokio::task::spawn_local(run_rollio_runtime_with_client(
                    RollioRuntimeConfig {
                        device_name: device_name.clone(),
                        interface: "can0".to_owned(),
                        dof: 6,
                        initial_mode: RobotMode::CommandFollowing,
                        publish_rate_hz: 80.0,
                        can_backend: CanWorkerBackend::AsyncFd,
                        model_backend: ModelBackendKind::PlayAnalytical,
                    },
                    Arc::clone(&client),
                ));

                let _warmup = receive_state(&ports.state_subscriber).await?;
                send_cartesian_command(
                    &ports.command_publisher,
                    [0.5, 0.4, 0.3, 0.0, 0.0, 0.0, 1.0],
                    0,
                )?;

                timeout(Duration::from_secs(2), async {
                    loop {
                        if !client
                            .task_targets
                            .lock()
                            .expect("task targets lock poisoned")
                            .is_empty()
                        {
                            break;
                        }
                        sleep(Duration::from_millis(10)).await;
                    }
                })
                .await?;

                let targets = client.targets.lock().expect("targets lock poisoned");
                assert!(targets.is_empty(), "cartesian commands should not use joint API");
                drop(targets);

                let task_targets = client
                    .task_targets
                    .lock()
                    .expect("task targets lock poisoned");
                assert_eq!(
                    task_targets.last().copied(),
                    Some([0.5, 0.4, 0.3, 0.0, 0.0, 0.0, 1.0])
                );
                drop(task_targets);

                send_control_event(&ports.control_publisher, ControlEvent::Shutdown)?;
                task.await??;
                Ok::<(), Box<dyn Error>>(())
            })
            .await
    }

    fn create_test_ports(device_name: &str) -> Result<TestPorts, Box<dyn Error>> {
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

    async fn receive_state(subscriber: &StateSubscriber) -> Result<RobotState, Box<dyn Error>> {
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
        targets: [f64; ARM_DOF],
    ) -> Result<(), Box<dyn Error>> {
        let mut command = RobotCommand {
            mode: CommandMode::Joint,
            num_joints: ARM_DOF as u32,
            ..RobotCommand::default()
        };
        command.joint_targets[..ARM_DOF].copy_from_slice(&targets);
        publisher.send_copy(command)?;
        Ok(())
    }

    fn send_cartesian_command(
        publisher: &CommandPublisher,
        targets: [f64; 7],
        num_joints: u32,
    ) -> Result<(), Box<dyn Error>> {
        let mut command = RobotCommand {
            mode: CommandMode::Cartesian,
            num_joints,
            ..RobotCommand::default()
        };
        command.cartesian_target.copy_from_slice(&targets);
        publisher.send_copy(command)?;
        Ok(())
    }

    fn send_control_event(
        publisher: &ControlPublisher,
        event: ControlEvent,
    ) -> Result<(), Box<dyn Error>> {
        publisher.send_copy(event)?;
        Ok(())
    }

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn unique_name(prefix: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{prefix}_{nanos}")
    }
}
