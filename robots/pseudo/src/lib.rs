use iceoryx2::prelude::*;
use rollio_bus::{robot_command_service_name, robot_state_service_name, CONTROL_EVENTS_SERVICE};
use rollio_types::config::{DeviceConfig, DeviceType, RobotMode};
use rollio_types::messages::{CommandMode, ControlEvent, RobotCommand, RobotState, MAX_JOINTS};
use std::error::Error;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
struct RuntimeTuning {
    dof: usize,
    frequency_hz: f64,
    command_latency_s: f64,
    state_noise_stddev: f64,
}

#[derive(Debug, Default)]
struct ControlFlowState {
    next_mode: Option<RobotMode>,
    shutdown: bool,
}

pub fn validate_device(device: DeviceConfig) -> Result<DeviceConfig, Box<dyn Error>> {
    if device.device_type != DeviceType::Robot {
        return Err(format!("device \"{}\" is not a robot", device.name).into());
    }
    if device.driver != "pseudo" {
        return Err(format!(
            "device \"{}\" uses driver \"{}\", expected pseudo",
            device.name, device.driver
        )
        .into());
    }

    Ok(device)
}

pub fn run_device(device: DeviceConfig) -> Result<(), Box<dyn Error>> {
    let mode = device.mode.ok_or("pseudo robot requires mode")?;
    let dof = device.dof.ok_or("pseudo robot requires dof")? as usize;
    let tuning = RuntimeTuning {
        dof,
        frequency_hz: device.control_frequency_hz.unwrap_or(60.0),
        command_latency_s: (device.command_latency_ms.unwrap_or(50) as f64) / 1000.0,
        state_noise_stddev: device.state_noise_stddev.unwrap_or(0.0),
    };

    let period = Duration::from_secs_f64(1.0 / tuning.frequency_hz.max(1.0));
    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let state_service_name: ServiceName =
        robot_state_service_name(&device.name).as_str().try_into()?;
    let state_service = node
        .service_builder(&state_service_name)
        .publish_subscribe::<RobotState>()
        .open_or_create()?;
    let state_publisher = state_service.publisher_builder().create()?;

    let command_service_name: ServiceName = robot_command_service_name(&device.name)
        .as_str()
        .try_into()?;
    let command_service = node
        .service_builder(&command_service_name)
        .publish_subscribe::<RobotCommand>()
        .open_or_create()?;
    let command_subscriber = command_service.subscriber_builder().create()?;

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    let control_subscriber = control_service.subscriber_builder().create()?;

    eprintln!(
        "rollio-robot-pseudo: device={} mode={mode:?} dof={} rate_hz={:.1}",
        device.name, tuning.dof, tuning.frequency_hz
    );

    let start = Instant::now();
    let mut next_tick = Instant::now();
    let mut current_mode = mode;
    let mut frame_index = 0u64;
    let mut last_status = Instant::now();
    let mut current_positions = [0.0f64; MAX_JOINTS];
    let mut target_positions = [0.0f64; MAX_JOINTS];
    let mut previous_positions = [0.0f64; MAX_JOINTS];

    loop {
        let control_flow = drain_control_events(&control_subscriber)?;
        if control_flow.shutdown {
            break;
        }
        if let Some(next_mode) = control_flow.next_mode {
            current_mode = next_mode;
            eprintln!(
                "rollio-robot-pseudo: device={} switched to mode={current_mode:?}",
                device.name
            );
        }

        if current_mode == RobotMode::CommandFollowing {
            drain_commands(&command_subscriber, tuning.dof, &mut target_positions)?;
            update_command_following_state(
                &mut current_positions,
                &target_positions,
                tuning,
                period,
            );
        } else {
            update_free_drive_state(
                &mut current_positions,
                tuning,
                start.elapsed().as_secs_f64(),
            );
        }

        let timestamp_ns = unix_timestamp_ns();
        let elapsed_secs = start.elapsed().as_secs_f64();
        let mut positions = current_positions;
        let mut velocities = [0.0f64; MAX_JOINTS];
        let mut efforts = [0.0f64; MAX_JOINTS];
        for joint_idx in 0..tuning.dof {
            let noise = deterministic_noise(elapsed_secs, joint_idx, tuning.state_noise_stddev);
            positions[joint_idx] += noise;
            velocities[joint_idx] = (positions[joint_idx] - previous_positions[joint_idx])
                / period.as_secs_f64().max(1e-6);
            efforts[joint_idx] = match current_mode {
                RobotMode::FreeDrive => 0.1 * (elapsed_secs * 0.5 + joint_idx as f64).sin(),
                RobotMode::CommandFollowing => {
                    (target_positions[joint_idx] - positions[joint_idx]) * 0.25
                }
            };
        }
        previous_positions = positions;

        let state = RobotState {
            timestamp_ns,
            num_joints: tuning.dof as u32,
            positions,
            velocities,
            efforts,
            ..RobotState::default()
        };
        state_publisher.send_copy(state)?;

        frame_index += 1;
        if last_status.elapsed() >= Duration::from_secs(1) {
            eprintln!(
                "rollio-robot-pseudo: device={} mode={current_mode:?} ticks={frame_index} active=true",
                device.name
            );
            last_status = Instant::now();
        }

        next_tick += period;
        let now = Instant::now();
        if next_tick > now {
            std::thread::sleep(next_tick - now);
        } else {
            next_tick = now;
        }
    }

    eprintln!(
        "rollio-robot-pseudo: device={} shutdown complete",
        device.name
    );
    Ok(())
}

fn drain_control_events(
    subscriber: &iceoryx2::port::subscriber::Subscriber<ipc::Service, ControlEvent, ()>,
) -> Result<ControlFlowState, Box<dyn Error>> {
    let mut state = ControlFlowState::default();
    loop {
        match subscriber.receive()? {
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

fn drain_commands(
    subscriber: &iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotCommand, ()>,
    dof: usize,
    target_positions: &mut [f64; MAX_JOINTS],
) -> Result<(), Box<dyn Error>> {
    loop {
        let Some(sample) = subscriber.receive()? else {
            break;
        };
        let command = *sample.payload();
        let active_joints = dof.min(command.num_joints as usize).min(MAX_JOINTS);
        match command.mode {
            CommandMode::Joint => {
                target_positions[..active_joints]
                    .copy_from_slice(&command.joint_targets[..active_joints]);
            }
            CommandMode::Cartesian => {
                let cartesian_joints = active_joints.min(command.cartesian_target.len());
                for (joint_idx, target) in target_positions
                    .iter_mut()
                    .take(cartesian_joints)
                    .enumerate()
                {
                    *target = command.cartesian_target[joint_idx];
                }
            }
        }
    }
    Ok(())
}

fn update_free_drive_state(
    current_positions: &mut [f64; MAX_JOINTS],
    tuning: RuntimeTuning,
    elapsed_secs: f64,
) {
    for (joint_idx, position) in current_positions.iter_mut().take(tuning.dof).enumerate() {
        let frequency = 0.5 + joint_idx as f64 * 0.075;
        *position =
            (elapsed_secs * frequency * std::f64::consts::TAU + joint_idx as f64 * 0.4).sin();
    }
}

fn update_command_following_state(
    current_positions: &mut [f64; MAX_JOINTS],
    target_positions: &[f64; MAX_JOINTS],
    tuning: RuntimeTuning,
    period: Duration,
) {
    let alpha =
        (period.as_secs_f64() / tuning.command_latency_s.max(period.as_secs_f64())).clamp(0.0, 1.0);
    for joint_idx in 0..tuning.dof {
        let error = target_positions[joint_idx] - current_positions[joint_idx];
        current_positions[joint_idx] += error * alpha;
    }
}

fn deterministic_noise(elapsed_secs: f64, joint_idx: usize, noise_stddev: f64) -> f64 {
    if noise_stddev == 0.0 {
        return 0.0;
    }

    noise_stddev * (elapsed_secs * 17.0 + joint_idx as f64 * 1.37).sin()
}

fn unix_timestamp_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
