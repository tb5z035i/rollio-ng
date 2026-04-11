use clap::Parser;
use iceoryx2::node::NodeWaitFailure;
use iceoryx2::prelude::*;
use rollio_bus::{
    camera_frames_service_name, robot_command_service_name, robot_state_service_name,
    CONTROL_EVENTS_SERVICE, EPISODE_STATUS_SERVICE,
};
use rollio_types::messages::{
    CameraFrameHeader, CommandMode, ControlEvent, EpisodeStatus, RobotCommand, RobotState,
};
use serde_json::json;
use signal_hook::consts::signal::{SIGINT, SIGTERM};
use std::collections::HashMap;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Parser, Debug)]
#[command(name = "rollio-bus-tap")]
#[command(about = "Lightweight manual tap for Rollio iceoryx2 topics")]
struct Args {
    #[arg(long = "camera")]
    cameras: Vec<String>,

    #[arg(long = "robot-state")]
    robot_states: Vec<String>,

    #[arg(long)]
    leader: Option<String>,

    #[arg(long)]
    follower: Option<String>,

    #[arg(long, default_value_t = 5.0)]
    duration_s: f64,
}

struct CameraTap {
    name: String,
    subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, [u8], CameraFrameHeader>,
}

struct RobotStateTap {
    name: String,
    subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotState, ()>,
}

struct RobotCommandTap {
    name: String,
    subscriber: iceoryx2::port::subscriber::Subscriber<ipc::Service, RobotCommand, ()>,
}

#[derive(Debug, Default)]
struct CameraStats {
    frames: u64,
    first_frame_index: Option<u64>,
    last_frame_index: Option<u64>,
    first_source_timestamp_ns: Option<u64>,
    last_source_timestamp_ns: Option<u64>,
    last_receive_ns: Option<u64>,
    source_intervals_ms: Vec<f64>,
    receive_intervals_ms: Vec<f64>,
}

#[derive(Debug, Default)]
struct CommandStats {
    latencies_ms: Vec<f64>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("rollio-bus-tap: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if args.cameras.is_empty()
        && args.robot_states.is_empty()
        && args.leader.is_none()
        && args.follower.is_none()
    {
        return Err("specify at least one --camera, --leader, or --follower topic group".into());
    }

    let node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()?;

    let mut camera_taps = Vec::with_capacity(args.cameras.len());
    for camera in &args.cameras {
        let service_name: ServiceName = camera_frames_service_name(camera).as_str().try_into()?;
        let service = node
            .service_builder(&service_name)
            .publish_subscribe::<[u8]>()
            .user_header::<CameraFrameHeader>()
            .open_or_create()?;
        camera_taps.push(CameraTap {
            name: camera.clone(),
            subscriber: service.subscriber_builder().create()?,
        });
    }

    let mut robot_state_names = args.robot_states.clone();
    if let Some(leader) = &args.leader {
        if !robot_state_names.iter().any(|name| name == leader) {
            robot_state_names.push(leader.clone());
        }
    }
    let mut robot_state_taps = Vec::with_capacity(robot_state_names.len());
    for robot_name in &robot_state_names {
        let service_name: ServiceName = robot_state_service_name(robot_name).as_str().try_into()?;
        let service = node
            .service_builder(&service_name)
            .publish_subscribe::<RobotState>()
            .open_or_create()?;
        robot_state_taps.push(RobotStateTap {
            name: robot_name.clone(),
            subscriber: service.subscriber_builder().create()?,
        });
    }

    let follower_command_tap = if let Some(follower) = &args.follower {
        let service_name: ServiceName = robot_command_service_name(follower).as_str().try_into()?;
        let service = node
            .service_builder(&service_name)
            .publish_subscribe::<RobotCommand>()
            .open_or_create()?;
        Some(RobotCommandTap {
            name: follower.clone(),
            subscriber: service.subscriber_builder().create()?,
        })
    } else {
        None
    };

    let control_service_name: ServiceName = CONTROL_EVENTS_SERVICE.try_into()?;
    let control_service = node
        .service_builder(&control_service_name)
        .publish_subscribe::<ControlEvent>()
        .open_or_create()?;
    let control_subscriber = control_service.subscriber_builder().create()?;

    let episode_status_service_name: ServiceName = EPISODE_STATUS_SERVICE.try_into()?;
    let episode_status_service = node
        .service_builder(&episode_status_service_name)
        .publish_subscribe::<EpisodeStatus>()
        .open_or_create()?;
    let episode_status_subscriber = episode_status_service.subscriber_builder().create()?;

    let shutdown_requested = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGINT, Arc::clone(&shutdown_requested))?;
    signal_hook::flag::register(SIGTERM, Arc::clone(&shutdown_requested))?;

    let start = Instant::now();
    let mut camera_stats: HashMap<String, CameraStats> = HashMap::new();
    let mut command_stats = CommandStats::default();

    while !shutdown_requested.load(Ordering::Relaxed)
        && start.elapsed().as_secs_f64() < args.duration_s.max(0.0)
    {
        let receive_ns = now_ns();

        for tap in &camera_taps {
            loop {
                match tap.subscriber.receive()? {
                    Some(sample) => {
                        let header = *sample.user_header();
                        update_camera_stats(
                            camera_stats.entry(tap.name.clone()).or_default(),
                            header,
                            receive_ns,
                        );
                        println!(
                            "{}",
                            json!({
                                "type": "camera_frame",
                                "name": tap.name,
                                "receive_timestamp_ns": receive_ns,
                                "source_timestamp_ns": header.timestamp_ns,
                                "frame_index": header.frame_index,
                                "width": header.width,
                                "height": header.height,
                            })
                        );
                    }
                    None => break,
                }
            }
        }

        for tap in &robot_state_taps {
            loop {
                match tap.subscriber.receive()? {
                    Some(sample) => {
                        let state = *sample.payload();
                        let joint_count = state.num_joints.min(6) as usize;
                        println!(
                            "{}",
                            json!({
                                "type": "robot_state",
                                "name": tap.name,
                                "receive_timestamp_ns": receive_ns,
                                "source_timestamp_ns": state.timestamp_ns,
                                "num_joints": state.num_joints,
                                "positions": &state.positions[..joint_count],
                            })
                        );
                    }
                    None => break,
                }
            }
        }

        if let Some(tap) = &follower_command_tap {
            loop {
                match tap.subscriber.receive()? {
                    Some(sample) => {
                        let command = *sample.payload();
                        let latency_ms =
                            (receive_ns.saturating_sub(command.timestamp_ns)) as f64 / 1_000_000.0;
                        command_stats.latencies_ms.push(latency_ms);
                        println!(
                            "{}",
                            json!({
                                "type": "robot_command",
                                "name": tap.name,
                                "receive_timestamp_ns": receive_ns,
                                "source_timestamp_ns": command.timestamp_ns,
                                "mode": command_mode_name(command.mode),
                                "num_joints": command.num_joints,
                                "latency_ms": latency_ms,
                                "joint_targets": &command.joint_targets[..(command.num_joints as usize).min(6)],
                                "cartesian_target": command.cartesian_target,
                            })
                        );
                    }
                    None => break,
                }
            }
        }

        loop {
            match control_subscriber.receive()? {
                Some(sample) => {
                    let event = *sample.payload();
                    let (event_name, episode_index) = control_event_fields(event);
                    println!(
                        "{}",
                        json!({
                            "type": "control_event",
                            "receive_timestamp_ns": receive_ns,
                            "event": event_name,
                            "episode_index": episode_index,
                        })
                    );
                }
                None => break,
            }
        }

        loop {
            match episode_status_subscriber.receive()? {
                Some(sample) => {
                    let status = *sample.payload();
                    println!(
                        "{}",
                        json!({
                            "type": "episode_status",
                            "receive_timestamp_ns": receive_ns,
                            "state": status.state.as_str(),
                            "episode_count": status.episode_count,
                            "elapsed_ms": status.elapsed_ms,
                        })
                    );
                }
                None => break,
            }
        }

        match node.wait(Duration::from_millis(1)) {
            Ok(()) => {}
            Err(NodeWaitFailure::Interrupt | NodeWaitFailure::TerminationRequest) => break,
        }
    }

    print_summary(&camera_stats, &command_stats);
    Ok(())
}

fn update_camera_stats(stats: &mut CameraStats, header: CameraFrameHeader, receive_ns: u64) {
    stats.frames = stats.frames.saturating_add(1);
    stats.first_frame_index.get_or_insert(header.frame_index);
    stats
        .first_source_timestamp_ns
        .get_or_insert(header.timestamp_ns);

    if let Some(previous_source_ns) = stats.last_source_timestamp_ns {
        stats
            .source_intervals_ms
            .push((header.timestamp_ns.saturating_sub(previous_source_ns)) as f64 / 1_000_000.0);
    }
    if let Some(previous_receive_ns) = stats.last_receive_ns {
        stats
            .receive_intervals_ms
            .push((receive_ns.saturating_sub(previous_receive_ns)) as f64 / 1_000_000.0);
    }

    stats.last_source_timestamp_ns = Some(header.timestamp_ns);
    stats.last_frame_index = Some(header.frame_index);
    stats.last_receive_ns = Some(receive_ns);
}

fn print_summary(camera_stats: &HashMap<String, CameraStats>, command_stats: &CommandStats) {
    for (camera_name, stats) in camera_stats {
        let observed_fps = match (
            stats.first_frame_index,
            stats.last_frame_index,
            stats.first_source_timestamp_ns,
            stats.last_source_timestamp_ns,
        ) {
            (Some(first_frame), Some(last_frame), Some(first_ts), Some(last_ts))
                if last_frame > first_frame && last_ts > first_ts =>
            {
                (last_frame - first_frame) as f64 / ((last_ts - first_ts) as f64 / 1_000_000_000.0)
            }
            _ => 0.0,
        };
        println!(
            "{}",
            json!({
                "type": "summary",
                "stream": camera_name,
                "kind": "camera",
                "frames": stats.frames,
                "observed_fps": observed_fps,
                "source_interval_ms_median": percentile(stats.source_intervals_ms.clone(), 0.5),
                "source_interval_ms_p99": percentile(stats.source_intervals_ms.clone(), 0.99),
                "receive_interval_ms_median": percentile(stats.receive_intervals_ms.clone(), 0.5),
                "receive_interval_ms_p99": percentile(stats.receive_intervals_ms.clone(), 0.99),
            })
        );
    }

    println!(
        "{}",
        json!({
            "type": "summary",
            "kind": "command_latency",
            "samples": command_stats.latencies_ms.len(),
            "latency_ms_median": percentile(command_stats.latencies_ms.clone(), 0.5),
            "latency_ms_p99": percentile(command_stats.latencies_ms.clone(), 0.99),
        })
    );
}

fn percentile(mut values: Vec<f64>, quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((values.len().saturating_sub(1)) as f64 * quantile).round() as usize;
    values[index]
}

fn control_event_fields(event: ControlEvent) -> (&'static str, Option<u32>) {
    match event {
        ControlEvent::RecordingStart { episode_index } => ("recording_start", Some(episode_index)),
        ControlEvent::RecordingStop { episode_index } => ("recording_stop", Some(episode_index)),
        ControlEvent::EpisodeKeep { episode_index } => ("episode_keep", Some(episode_index)),
        ControlEvent::EpisodeDiscard { episode_index } => ("episode_discard", Some(episode_index)),
        ControlEvent::Shutdown => ("shutdown", None),
        ControlEvent::ModeSwitch { .. } => ("mode_switch", None),
    }
}

fn command_mode_name(mode: CommandMode) -> &'static str {
    match mode {
        CommandMode::Joint => "joint",
        CommandMode::Cartesian => "cartesian",
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
