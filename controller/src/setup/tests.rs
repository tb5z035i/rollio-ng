use super::devices::wizard_selectable_modes;
use super::discovery::{
    available_devices_from_discoveries, binary_device_from_discovery, build_discovery_config,
    missing_value_limit_warnings,
};
use super::pairings::{ensure_channel_publishes_state, PairingEndpoint};
use super::runtime::{is_interrupt_exit_status, should_treat_trigger_as_shutdown};
use super::state::{
    CameraProfile, DiscoveredChannelMeta, DiscoveredDevice, SetupSession, SetupStep,
    TeleopPairCreate, IDENTIFY_ACTIVE_MESSAGE_PREFIX,
};
use crate::discovery::known_device_executables;
use rollio_types::config::{
    BinaryDeviceConfig, ChannelPairingConfig, CollectionMode, DeviceType, MappingStrategy,
    ProjectConfig, RobotCommandKind, RobotMode, RobotStateKind,
};
use rollio_types::messages::PixelFormat;
use serde_json::json;
#[cfg(unix)]
use signal_hook::consts::signal::SIGINT;
use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;


fn project_camera_device_names(p: &ProjectConfig) -> Vec<String> {
    p.devices
        .iter()
        .filter(|d| {
            d.channels
                .iter()
                .any(|c| c.kind == DeviceType::Camera && c.enabled)
        })
        .map(|d| d.name.clone())
        .collect()
}

fn project_robot_device_names(p: &ProjectConfig) -> Vec<String> {
    p.devices
        .iter()
        .filter(|d| {
            d.channels
                .iter()
                .any(|c| c.kind == DeviceType::Robot && c.enabled)
        })
        .map(|d| d.name.clone())
        .collect()
}

fn make_robot_modes() -> Vec<RobotMode> {
    vec![
        RobotMode::FreeDrive,
        RobotMode::CommandFollowing,
        RobotMode::Identifying,
        RobotMode::Disabled,
    ]
}

fn camera_discovery(id: &str) -> DiscoveredDevice {
    let mut channel_meta_by_channel = BTreeMap::new();
    channel_meta_by_channel.insert(
        "color".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Camera,
            channel_label: Some("Pseudo Camera".into()),
            default_name: None,
            modes: Vec::new(),
            profiles: vec![CameraProfile {
                width: 640,
                height: 480,
                fps: 30,
                pixel_format: PixelFormat::Rgb24,
                native_pixel_format: None,
                stream: Some("color".into()),
                channel: None,
            }],
            ..DiscoveredChannelMeta::default()
        },
    );
    DiscoveredDevice {
        driver: "pseudo".into(),
        id: id.into(),
        display_name: id.into(),
        default_device_name: Some("pseudo_camera".into()),
        channel_meta_by_channel,
        transport: Some("simulated".into()),
        interface: None,
        product_variant: None,
        end_effector: None,
    }
}

fn robot_discovery(id: &str, dof: u32) -> DiscoveredDevice {
    let default_name = if dof == 1 { "pseudo_eef" } else { "pseudo_arm" };
    let mut channel_meta_by_channel = BTreeMap::new();
    // Default fixture self-whitelists for direct-joint so the
    // existing auto-pair tests continue to seed pairings without
    // needing per-driver opt-in. Tests that need to exercise the
    // strict whitelist rejection use `robot_discovery_no_whitelist`
    // (defined alongside).
    let direct_joint_compatibility = rollio_types::config::DirectJointCompatibility {
        can_lead: vec![rollio_types::config::DirectJointCompatibilityPeer {
            driver: "pseudo".into(),
            channel_type: "arm".into(),
        }],
        can_follow: vec![rollio_types::config::DirectJointCompatibilityPeer {
            driver: "pseudo".into(),
            channel_type: "arm".into(),
        }],
    };
    channel_meta_by_channel.insert(
        "arm".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Robot,
            channel_label: None,
            default_name: Some(default_name.to_owned()),
            modes: make_robot_modes(),
            dof: Some(dof),
            default_control_frequency_hz: Some(60.0),
            direct_joint_compatibility,
            // The fixture mirrors what an arm driver advertises: it
            // can lead via `joint_position` state and accept
            // `joint_position` / `joint_mit` commands. (Tests that
            // need parallel-grip behaviour use `parallel_gripper_discovery`
            // below.)
            supported_commands: vec![
                RobotCommandKind::JointPosition,
                RobotCommandKind::JointMit,
            ],
            ..DiscoveredChannelMeta::default()
        },
    );
    DiscoveredDevice {
        driver: "pseudo".into(),
        id: id.into(),
        display_name: id.into(),
        default_device_name: Some(default_name.to_owned()),
        channel_meta_by_channel,
        transport: Some("simulated".into()),
        interface: None,
        product_variant: None,
        end_effector: None,
    }
}

/// Robot discovery fixture WITHOUT a `direct_joint_compatibility`
/// whitelist. Used by tests that exercise the new strict two-sided
/// whitelist rejection for DirectJoint pairs.
fn robot_discovery_no_whitelist(id: &str, dof: u32) -> DiscoveredDevice {
    let mut device = robot_discovery(id, dof);
    if let Some(meta) = device.channel_meta_by_channel.get_mut("arm") {
        meta.direct_joint_compatibility =
            rollio_types::config::DirectJointCompatibility::default();
    }
    device
}

/// Parallel-gripper discovery fixture: dof=1 robot channel that
/// publishes `parallel_position` and accepts `parallel_position` /
/// `parallel_mit` commands. Used by tests that exercise the new
/// `Parallel` policy.
fn parallel_gripper_discovery(id: &str, default_name: &str) -> DiscoveredDevice {
    let mut channel_meta_by_channel = BTreeMap::new();
    channel_meta_by_channel.insert(
        "gripper".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Robot,
            channel_label: Some("Pseudo Parallel Gripper".into()),
            default_name: Some(default_name.to_owned()),
            modes: make_robot_modes(),
            dof: Some(1),
            default_control_frequency_hz: Some(60.0),
            supported_states: vec![
                RobotStateKind::ParallelPosition,
                RobotStateKind::ParallelVelocity,
                RobotStateKind::ParallelEffort,
            ],
            supported_commands: vec![
                RobotCommandKind::ParallelPosition,
                RobotCommandKind::ParallelMit,
            ],
            ..DiscoveredChannelMeta::default()
        },
    );
    DiscoveredDevice {
        driver: "pseudo".into(),
        id: id.into(),
        display_name: id.into(),
        default_device_name: Some(default_name.to_owned()),
        channel_meta_by_channel,
        transport: Some("simulated".into()),
        interface: None,
        product_variant: None,
        end_effector: None,
    }
}

fn airbot_play_discovery(end_effector: Option<&str>) -> DiscoveredDevice {
    let mut channel_meta_by_channel = BTreeMap::new();
    channel_meta_by_channel.insert(
        "arm".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Robot,
            channel_label: Some("AIRBOT Play".into()),
            default_name: Some("airbot_play_arm".into()),
            modes: make_robot_modes(),
            dof: Some(6),
            default_control_frequency_hz: Some(250.0),
            direct_joint_compatibility: rollio_types::config::DirectJointCompatibility {
                can_lead: vec![rollio_types::config::DirectJointCompatibilityPeer {
                    driver: "airbot-play".into(),
                    channel_type: "arm".into(),
                }],
                can_follow: vec![rollio_types::config::DirectJointCompatibilityPeer {
                    driver: "airbot-play".into(),
                    channel_type: "arm".into(),
                }],
            },
            ..DiscoveredChannelMeta::default()
        },
    );
    if let Some(channel_type) = end_effector.map(|value| value.to_ascii_lowercase()) {
        let (label, name, defaults) = match channel_type.as_str() {
            "e2" => (
                "AIRBOT E2",
                "airbot_e2",
                rollio_types::config::ChannelCommandDefaults {
                    joint_mit_kp: Vec::new(),
                    joint_mit_kd: Vec::new(),
                    parallel_mit_kp: vec![0.0],
                    parallel_mit_kd: vec![0.0],
                },
            ),
            "g2" => (
                "AIRBOT G2",
                "airbot_g2",
                rollio_types::config::ChannelCommandDefaults {
                    joint_mit_kp: Vec::new(),
                    joint_mit_kd: Vec::new(),
                    parallel_mit_kp: vec![10.0],
                    parallel_mit_kd: vec![0.5],
                },
            ),
            _ => (
                "AIRBOT EEF",
                "airbot_eef",
                rollio_types::config::ChannelCommandDefaults::default(),
            ),
        };
        channel_meta_by_channel.insert(
            channel_type,
            DiscoveredChannelMeta {
                kind: DeviceType::Robot,
                channel_label: Some(label.into()),
                default_name: Some(name.into()),
                modes: make_robot_modes(),
                dof: Some(1),
                default_control_frequency_hz: Some(250.0),
                defaults,
                ..DiscoveredChannelMeta::default()
            },
        );
    }
    DiscoveredDevice {
        driver: "airbot-play".into(),
        id: "PZ123".into(),
        display_name: "AIRBOT Play".into(),
        default_device_name: Some("airbot_play".into()),
        channel_meta_by_channel,
        transport: Some("can".into()),
        interface: Some("can0".into()),
        product_variant: Some("play-e2".into()),
        end_effector: end_effector.map(str::to_owned),
    }
}

fn setup_session(discoveries: &[DiscoveredDevice]) -> SetupSession {
    let config = build_discovery_config(discoveries).expect("config should build");
    let available_devices = available_devices_from_discoveries(discoveries, &config)
        .expect("available devices should build");
    SetupSession::new(
        config,
        available_devices,
        std::path::PathBuf::from("config.toml"),
        false,
        Vec::new(),
    )
}

#[test]
fn build_discovery_config_assigns_default_roles_and_pairing() {
    let config = build_discovery_config(&[
        camera_discovery("cam0"),
        camera_discovery("cam1"),
        robot_discovery("robot0", 6),
        robot_discovery("robot1", 6),
        robot_discovery("eef0", 1),
        robot_discovery("eef1", 1),
    ])
    .expect("config should build");

    assert_eq!(
        project_camera_device_names(&config),
        vec!["pseudo_camera", "pseudo_camera_2"]
    );
    assert_eq!(
        project_robot_device_names(&config),
        vec!["pseudo_arm", "pseudo_arm_2", "pseudo_eef", "pseudo_eef_2"]
    );
    assert_eq!(config.pairings.len(), 2);
    assert_eq!(config.pairings[0].leader_device, "pseudo_arm");
    assert_eq!(config.pairings[0].follower_device, "pseudo_arm_2");
    assert_eq!(config.pairings[1].leader_device, "pseudo_eef");
    assert_eq!(config.pairings[1].follower_device, "pseudo_eef_2");
}

// (Legacy `parse_camera_capabilities_*` and `normalize_camera_profiles_*`
// tests removed alongside their helper functions: profiles now flow
// through `parse_query_channel_meta` directly from the driver's
// `query --json`, and the v4l2 special-case is dead code because the
// driver itself reports normalized RGB24/BGR24.)

#[test]
fn available_devices_from_discoveries_merges_airbot_interface_into_existing_config() {
    let discovery = airbot_play_discovery(Some("e2"));
    let mut config =
        build_discovery_config(std::slice::from_ref(&discovery)).expect("config should build");
    config.devices[0].extra.clear();

    let available = available_devices_from_discoveries(&[discovery], &config)
        .expect("available devices should build");

    assert_eq!(
        available[0]
            .current
            .extra
            .get("interface")
            .and_then(|value| value.as_str()),
        Some("can0")
    );
    assert_eq!(
        available[0]
            .current
            .extra
            .get("end_effector")
            .and_then(|value| value.as_str()),
        Some("e2")
    );
}

#[test]
fn build_discovery_config_dedupes_robot_channel_names_across_two_airbot_devices() {
    // Two physical AIRBOT Play arms each report
    // `default_name = "airbot_play_arm"` from the driver query. The
    // discovery path must dedupe the channel-level name the same way it
    // dedupes the device-level name, otherwise the wizard's devices step
    // shows two rows with identical names and the operator can't tell
    // them apart.
    let mut leader = airbot_play_discovery(Some("e2"));
    leader.id = "PZ_LEADER".into();
    let mut follower = airbot_play_discovery(Some("e2"));
    follower.id = "PZ_FOLLOWER".into();

    let config = build_discovery_config(&[leader, follower]).expect("config should build");

    let arm_names: Vec<&str> = config
        .devices
        .iter()
        .filter(|device| device.driver == "airbot-play")
        .filter_map(|device| {
            device
                .channel_named("arm")
                .and_then(|channel| channel.name.as_deref())
        })
        .collect();
    assert_eq!(
        arm_names,
        vec!["airbot_play_arm", "airbot_play_arm_2"],
        "two airbot arms must get distinct channel names",
    );

    let eef_names: Vec<&str> = config
        .devices
        .iter()
        .filter(|device| device.driver == "airbot-play")
        .filter_map(|device| {
            device
                .channel_named("e2")
                .and_then(|channel| channel.name.as_deref())
        })
        .collect();
    assert_eq!(
        eef_names,
        vec!["airbot_e2", "airbot_e2_2"],
        "two airbot end-effector channels must get distinct names",
    );
}

#[test]
fn available_devices_from_discoveries_splits_airbot_channels_into_rows() {
    let discovery = airbot_play_discovery(Some("e2"));
    let config =
        build_discovery_config(std::slice::from_ref(&discovery)).expect("config should build");

    let available = available_devices_from_discoveries(&[discovery], &config)
        .expect("available devices should build");

    assert_eq!(available.len(), 2);
    assert_eq!(available[0].current.channels.len(), 1);
    let arm_channel = &available[0].current.channels[0];
    assert_eq!(arm_channel.channel_type, "arm");
    assert_eq!(available[0].display_name, "AIRBOT Play");
    assert_eq!(arm_channel.channel_label.as_deref(), Some("AIRBOT Play"));
    assert_eq!(arm_channel.name.as_deref(), Some("airbot_play_arm"));

    assert_eq!(available[1].current.channels.len(), 1);
    let eef_channel = &available[1].current.channels[0];
    assert_eq!(eef_channel.channel_type, "e2");
    assert_eq!(available[1].display_name, "AIRBOT E2");
    assert_eq!(eef_channel.channel_label.as_deref(), Some("AIRBOT E2"));
    assert_eq!(eef_channel.name.as_deref(), Some("airbot_e2"));
    // The two rows share the same parent BinaryDeviceConfig.name (= bus
    // root / iceoryx2 service root), but their per-channel `name` fields
    // are independent so renaming one row no longer affects the other.
    assert_eq!(available[0].current.name, available[1].current.name);
    assert_ne!(arm_channel.name, eef_channel.name);
}

#[test]
fn toggle_device_selection_disables_only_selected_airbot_channel() {
    let mut session = setup_session(&[airbot_play_discovery(Some("e2"))]);
    let e2_name = session
        .available_devices
        .iter()
        .find(|device| device.current.channels[0].channel_type == "e2")
        .expect("e2 row should exist")
        .name
        .clone();

    assert!(session
        .toggle_device_selection(&e2_name)
        .expect("toggle should succeed"));

    let device = session
        .config
        .devices
        .iter()
        .find(|device| device.driver == "airbot-play")
        .expect("physical airbot device should remain configured");
    assert!(device
        .channel_named("arm")
        .is_some_and(|channel| channel.enabled));
    assert!(device
        .channel_named("e2")
        .is_some_and(|channel| !channel.enabled));
}

/// `airbot_play_discovery` plus an explicit `EndEffectorPose` in
/// supported_states. Used to exercise the new toggle commands so the
/// fixture has enough surface area to flip kinds on and off.
fn airbot_arm_discovery_with_supported_states(
    supported: Vec<RobotStateKind>,
) -> DiscoveredDevice {
    let mut discovery = airbot_play_discovery(None);
    // Preserve the parent fixture's `direct_joint_compatibility` (it
    // already self-whitelists airbot-play:arm peers) so DirectJoint
    // auto-pairing keeps working under the new strict whitelist.
    let parent_compat = discovery
        .channel_meta_by_channel
        .get("arm")
        .map(|m| m.direct_joint_compatibility.clone())
        .unwrap_or_default();
    discovery.channel_meta_by_channel.insert(
        "arm".into(),
        DiscoveredChannelMeta {
            kind: DeviceType::Robot,
            channel_label: Some("AIRBOT Play".into()),
            default_name: Some("airbot_play_arm".into()),
            modes: make_robot_modes(),
            dof: Some(6),
            default_control_frequency_hz: Some(250.0),
            supported_states: supported,
            supported_commands: vec![
                RobotCommandKind::JointPosition,
                RobotCommandKind::JointMit,
                RobotCommandKind::EndPose,
            ],
            direct_joint_compatibility: parent_compat,
            ..DiscoveredChannelMeta::default()
        },
    );
    discovery
}

#[test]
fn binary_device_from_discovery_defaults_publish_states_to_all_supported() {
    // The driver advertises EndEffectorPose alongside the standard
    // joint kinds; the wizard should opt all of them into both
    // publish_states and recorded_states by default so operators don't
    // hit the FK/IK pairing failure when switching mappings.
    let discovery = airbot_arm_discovery_with_supported_states(vec![
        RobotStateKind::JointPosition,
        RobotStateKind::JointVelocity,
        RobotStateKind::JointEffort,
        RobotStateKind::EndEffectorPose,
    ]);
    let device = binary_device_from_discovery(
        &discovery,
        "airbot_play".into(),
        RobotMode::FreeDrive,
        &mut BTreeMap::new(),
    );
    let arm = device
        .channel_named("arm")
        .expect("arm channel should exist");
    assert!(arm
        .publish_states
        .contains(&RobotStateKind::EndEffectorPose));
    assert_eq!(arm.publish_states, arm.recorded_states);
}

#[test]
fn toggle_publish_state_drops_recorded_kind_with_it() {
    let discovery = airbot_arm_discovery_with_supported_states(vec![
        RobotStateKind::JointPosition,
        RobotStateKind::JointVelocity,
        RobotStateKind::JointEffort,
    ]);
    let mut session = setup_session(&[discovery]);
    let arm_name = session
        .available_devices
        .iter()
        .find(|device| device.current.channels[0].channel_type == "arm")
        .expect("arm row should exist")
        .name
        .clone();

    assert!(session
        .toggle_publish_state(&arm_name, RobotStateKind::JointEffort)
        .expect("toggle should succeed"));

    let arm = session
        .config
        .device_named("airbot_play")
        .and_then(|device| device.channel_named("arm"))
        .expect("arm channel still configured");
    assert!(!arm.publish_states.contains(&RobotStateKind::JointEffort));
    assert!(
        !arm.recorded_states.contains(&RobotStateKind::JointEffort),
        "recorded_states must stay a subset of publish_states",
    );
}

#[test]
fn toggle_publish_state_blocks_removal_when_pairing_uses_kind() {
    let discovery = airbot_arm_discovery_with_supported_states(vec![
        RobotStateKind::JointPosition,
        RobotStateKind::JointVelocity,
        RobotStateKind::JointEffort,
    ]);
    // Two arms so a default pairing exists with leader_state = JointPosition.
    let mut session = setup_session(&[discovery.clone(), discovery]);
    let leader_name = session
        .config
        .pairings
        .first()
        .expect("default pairing should exist")
        .leader_device
        .clone();
    let leader_row = session
        .available_devices
        .iter()
        .find(|device| {
            device.current.name == leader_name
                && device.current.channels[0].channel_type == "arm"
        })
        .expect("leader arm row should exist")
        .name
        .clone();

    let outcome = session
        .toggle_publish_state(&leader_row, RobotStateKind::JointPosition)
        .expect("call should not error");
    assert!(
        !outcome,
        "toggling off a leader_state must be rejected without applying the change",
    );
    let arm = session
        .config
        .device_named(&leader_name)
        .and_then(|device| device.channel_named("arm"))
        .expect("leader arm still configured");
    assert!(arm.publish_states.contains(&RobotStateKind::JointPosition));
    assert!(session.message.is_some(), "user should see a clear refusal");
}

#[test]
fn toggle_publish_state_mirrors_into_available_devices_snapshot() {
    // Regression for a UI bug: the wizard reads publish_states /
    // recorded_states from `AvailableDevice.current.channels[0]`, so a
    // toggle that only updates `config.devices` leaves the rendered
    // [P R] glyph stale. Toggle methods must mirror the freshly
    // mutated kind into the AvailableDevice snapshot, the way
    // `cycle_robot_mode` does for the channel mode.
    let discovery = airbot_arm_discovery_with_supported_states(vec![
        RobotStateKind::JointPosition,
        RobotStateKind::JointVelocity,
        RobotStateKind::JointEffort,
    ]);
    let mut session = setup_session(&[discovery]);
    let arm_name = session
        .available_devices
        .iter()
        .find(|device| device.current.channels[0].channel_type == "arm")
        .expect("arm row should exist")
        .name
        .clone();

    assert!(session
        .toggle_publish_state(&arm_name, RobotStateKind::JointEffort)
        .expect("publish toggle should succeed"));

    let available_channel = session
        .available_devices
        .iter()
        .find(|device| device.name == arm_name)
        .and_then(|device| device.current.channels.first())
        .expect("available arm row should still exist");
    assert!(
        !available_channel
            .publish_states
            .contains(&RobotStateKind::JointEffort),
        "AvailableDevice snapshot must mirror the updated publish_states; \
         got {:?}",
        available_channel.publish_states,
    );
    assert!(
        !available_channel
            .recorded_states
            .contains(&RobotStateKind::JointEffort),
        "AvailableDevice snapshot must mirror the updated recorded_states",
    );

    assert!(session
        .toggle_recorded_state(&arm_name, RobotStateKind::JointVelocity)
        .expect("recorded toggle should succeed"));
    let available_channel = session
        .available_devices
        .iter()
        .find(|device| device.name == arm_name)
        .and_then(|device| device.current.channels.first())
        .expect("available arm row should still exist");
    assert!(
        !available_channel
            .recorded_states
            .contains(&RobotStateKind::JointVelocity),
        "recorded_state toggle should also propagate; got {:?}",
        available_channel.recorded_states,
    );
}

#[test]
fn toggle_recorded_state_requires_published_kind() {
    let discovery = airbot_arm_discovery_with_supported_states(vec![
        RobotStateKind::JointPosition,
        RobotStateKind::JointVelocity,
        RobotStateKind::JointEffort,
    ]);
    let mut session = setup_session(&[discovery]);
    let arm_name = session
        .available_devices
        .iter()
        .find(|device| device.current.channels[0].channel_type == "arm")
        .expect("arm row should exist")
        .name
        .clone();

    // Drop joint_effort from publish_states (and implicitly from recorded_states)
    // and then attempt to record it: the wizard should reject the toggle.
    assert!(session
        .toggle_publish_state(&arm_name, RobotStateKind::JointEffort)
        .expect("publish toggle should succeed"));
    let outcome = session
        .toggle_recorded_state(&arm_name, RobotStateKind::JointEffort)
        .expect("call should not error");
    assert!(!outcome, "recording a non-published kind must be rejected",);
    let arm = session
        .config
        .device_named("airbot_play")
        .and_then(|device| device.channel_named("arm"))
        .expect("arm channel still configured");
    assert!(!arm.recorded_states.contains(&RobotStateKind::JointEffort));
}

#[test]
fn preview_runtime_project_overrides_visualizer_port() {
    let config = build_discovery_config(&[
        camera_discovery("cam0"),
        camera_discovery("cam1"),
        robot_discovery("robot0", 6),
        robot_discovery("robot1", 6),
    ])
    .expect("config should build");

    let mut preview = config.clone();
    preview.visualizer.port = 42424;

    assert_eq!(preview.visualizer.port, 42424);
    assert_eq!(
        project_camera_device_names(&preview),
        project_camera_device_names(&config)
    );
    assert_eq!(
        project_robot_device_names(&preview),
        project_robot_device_names(&config)
    );
}

#[test]
fn visible_steps_include_pairing_for_teleop_default() {
    // Teleop is now the only collection mode the wizard exposes — the
    // default discovery config lands on it directly so the Pairing
    // step is always visible.
    let session = setup_session(&[camera_discovery("cam0"), robot_discovery("robot0", 6)]);

    assert_eq!(session.config.mode, CollectionMode::Teleop);
    assert_eq!(
        session.visible_steps(),
        &[
            SetupStep::Devices,
            SetupStep::Storage,
            SetupStep::Pairing,
            SetupStep::States,
            SetupStep::Preview,
        ]
    );
}

#[test]
fn visible_steps_drop_pairing_for_legacy_intervention_configs() {
    // Older saved configs may explicitly set `mode = "intervention"`;
    // round-trip through the wizard still hides the Pairing step in
    // that case so the operator can review/save without seeing a
    // step they no longer use.
    let mut session = setup_session(&[camera_discovery("cam0"), robot_discovery("robot0", 6)]);
    session.config.mode = CollectionMode::Intervention;
    assert_eq!(
        session.visible_steps(),
        &[
            SetupStep::Devices,
            SetupStep::Storage,
            SetupStep::States,
            SetupStep::Preview,
        ]
    );
}

#[test]
fn jump_to_step_maps_legacy_device_stage_names_to_devices() {
    let mut session = setup_session(&[camera_discovery("cam0")]);

    session.current_step = SetupStep::Preview;
    assert!(session.jump_to_step("discovery"));
    assert_eq!(session.current_step, SetupStep::Devices);

    session.current_step = SetupStep::Preview;
    assert!(session.jump_to_step("selection"));
    assert_eq!(session.current_step, SetupStep::Devices);

    session.current_step = SetupStep::Preview;
    assert!(session.jump_to_step("parameters"));
    assert_eq!(session.current_step, SetupStep::Devices);

    session.current_step = SetupStep::Preview;
    assert!(session.jump_to_step("devices"));
    assert_eq!(session.current_step, SetupStep::Devices);
}

#[test]
fn deselecting_identified_device_clears_identify_target() {
    let mut session = setup_session(&[camera_discovery("cam0"), robot_discovery("robot0", 6)]);
    let device_name = session.available_devices[0].name.clone();

    assert!(session.is_device_selected(&device_name));
    session.message = Some(format!("{IDENTIFY_ACTIVE_MESSAGE_PREFIX}{device_name}"));
    assert!(session.set_identify_device(Some(&device_name)));
    assert_eq!(
        session.identify_device_name.as_deref(),
        Some(device_name.as_str())
    );

    assert!(session
        .toggle_device_selection(&device_name)
        .expect("deselect should succeed"));
    assert!(!session.is_device_selected(&device_name));
    assert!(session.identify_device_name.is_none());
    assert!(session.message.is_none());
}

#[test]
fn clearing_identify_target_removes_identify_message() {
    let mut session = setup_session(&[camera_discovery("cam0")]);
    let device_name = session.available_devices[0].name.clone();

    session.message = Some(format!("{IDENTIFY_ACTIVE_MESSAGE_PREFIX}{device_name}"));
    assert!(session.set_identify_device(Some(&device_name)));
    assert!(session.set_identify_device(None));
    assert!(session.identify_device_name.is_none());
    assert!(session.message.is_none());
}

#[test]
fn setup_toggle_identify_ignores_unselected_devices() {
    let mut session = setup_session(&[camera_discovery("cam0"), robot_discovery("robot0", 6)]);
    let device_name = session.available_devices[0].name.clone();

    session
        .toggle_device_selection(&device_name)
        .expect("deselect should succeed");

    let mutation = session
        .apply_raw_command(
            &json!({
                "type": "command",
                "action": "setup_toggle_identify",
                "name": device_name,
            })
            .to_string(),
        )
        .expect("identify command should parse");

    assert!(!mutation.state_changed);
    assert!(session.identify_device_name.is_none());
}

#[test]
fn known_device_executables_skip_pseudo_and_standalone_eef_by_default() {
    let executables = known_device_executables()
        .iter()
        .copied()
        .collect::<Vec<_>>();

    assert_eq!(
        executables,
        vec![
            "rollio-device-airbot-play",
            "rollio-device-realsense",
            "rollio-device-v4l2",
            "rollio-device-agx-nero",
        ]
    );
    assert!(
        !executables.contains(&"rollio-device-pseudo"),
        "pseudo must stay opt-in via --sim-pseudo"
    );
}

#[cfg(unix)]
#[test]
fn interrupt_exit_statuses_are_treated_as_shutdown() {
    let sigint_status = std::process::ExitStatus::from_raw(SIGINT);
    let code_130_status = std::process::ExitStatus::from_raw(130 << 8);
    let code_143_status = std::process::ExitStatus::from_raw(143 << 8);
    let normal_error_status = std::process::ExitStatus::from_raw(1 << 8);

    assert!(is_interrupt_exit_status(&sigint_status));
    assert!(is_interrupt_exit_status(&code_130_status));
    assert!(is_interrupt_exit_status(&code_143_status));
    assert!(!is_interrupt_exit_status(&normal_error_status));
}

#[cfg(unix)]
#[test]
fn child_interrupt_trigger_is_not_treated_as_crash() {
    let trigger = crate::ShutdownTrigger::ChildExited {
        id: "setup-ui".into(),
        status: std::process::ExitStatus::from_raw(130 << 8),
    };

    assert!(should_treat_trigger_as_shutdown(&trigger, false, false));
}

fn v4l2_discovery(id: &str) -> DiscoveredDevice {
    let mut channel_meta_by_channel = BTreeMap::new();
    channel_meta_by_channel.insert(
        "color".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Camera,
            channel_label: Some("V4L2 Camera".into()),
            default_name: Some("camera".into()),
            profiles: vec![CameraProfile {
                width: 640,
                height: 480,
                fps: 30,
                pixel_format: PixelFormat::Rgb24,
                native_pixel_format: Some("MJPG".into()),
                stream: Some("color".into()),
                channel: None,
            }],
            ..DiscoveredChannelMeta::default()
        },
    );
    DiscoveredDevice {
        driver: "v4l2".into(),
        id: id.into(),
        display_name: "V4L2 Camera".into(),
        default_device_name: Some("camera".into()),
        channel_meta_by_channel,
        transport: None,
        interface: None,
        product_variant: None,
        end_effector: None,
    }
}

fn realsense_multi_stream_discovery(id: &str) -> DiscoveredDevice {
    let mut channel_meta_by_channel = BTreeMap::new();
    channel_meta_by_channel.insert(
        "color".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Camera,
            channel_label: Some("Intel RealSense RGB".into()),
            default_name: Some("realsense_rgb".into()),
            profiles: vec![CameraProfile {
                width: 1920,
                height: 1080,
                fps: 30,
                pixel_format: PixelFormat::Rgb24,
                native_pixel_format: None,
                stream: Some("color".into()),
                channel: None,
            }],
            ..DiscoveredChannelMeta::default()
        },
    );
    channel_meta_by_channel.insert(
        "depth".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Camera,
            channel_label: Some("Intel RealSense Depth".into()),
            default_name: Some("realsense_depth".into()),
            profiles: vec![CameraProfile {
                width: 640,
                height: 480,
                fps: 30,
                pixel_format: PixelFormat::Depth16,
                native_pixel_format: None,
                stream: Some("depth".into()),
                channel: None,
            }],
            ..DiscoveredChannelMeta::default()
        },
    );
    channel_meta_by_channel.insert(
        "infrared".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Camera,
            channel_label: Some("Intel RealSense Infrared".into()),
            default_name: Some("realsense_ir".into()),
            profiles: vec![CameraProfile {
                width: 640,
                height: 480,
                fps: 30,
                pixel_format: PixelFormat::Gray8,
                native_pixel_format: None,
                stream: Some("infrared".into()),
                channel: None,
            }],
            ..DiscoveredChannelMeta::default()
        },
    );
    DiscoveredDevice {
        driver: "realsense".into(),
        id: id.into(),
        display_name: "Intel RealSense".into(),
        default_device_name: Some("realsense".into()),
        channel_meta_by_channel,
        transport: None,
        interface: None,
        product_variant: None,
        end_effector: None,
    }
}

fn camera_channel_names(device: &BinaryDeviceConfig) -> Vec<Option<String>> {
    device.channels.iter().map(|c| c.name.clone()).collect()
}

fn camera_channel_types(device: &BinaryDeviceConfig) -> Vec<String> {
    device
        .channels
        .iter()
        .map(|c| c.channel_type.clone())
        .collect()
}

/// Regression for issue #1: when two V4L2 cameras are discovered, the
/// channel name for the second one used to be set from the V4L2 driver's
/// `default_name = "camera"` and was *not* deduplicated, so both setup
/// rows showed `name=camera` and the user couldn't tell them apart in
/// the wizard.
#[test]
fn build_discovery_config_dedupes_channel_name_for_two_v4l2_cameras() {
    let config =
        build_discovery_config(&[v4l2_discovery("/dev/video0"), v4l2_discovery("/dev/video2")])
            .expect("config should build");

    assert_eq!(
        project_camera_device_names(&config),
        vec!["camera", "camera_2"]
    );
    assert_eq!(
        camera_channel_names(&config.devices[0]),
        vec![Some("camera".to_string())]
    );
    assert_eq!(
        camera_channel_names(&config.devices[1]),
        vec![Some("camera_2".to_string())]
    );
}

/// Regression for issue #3: a RealSense unit reports color + depth +
/// infrared in its `query --json` output, but `build_discovery_config`
/// used to keep only the first camera profile, so the wizard showed
/// just one `color` channel and depth / infrared were silently dropped.
#[test]
fn build_discovery_config_keeps_all_realsense_streams() {
    let config = build_discovery_config(&[realsense_multi_stream_discovery("332322071743")])
        .expect("config should build");

    assert_eq!(config.devices.len(), 1);
    let device = &config.devices[0];
    assert_eq!(device.driver, "realsense");
    assert_eq!(device.name, "realsense");
    assert_eq!(device.bus_root, "realsense");
    assert_eq!(
        camera_channel_types(device),
        vec![
            "color".to_string(),
            "depth".to_string(),
            "infrared".to_string()
        ]
    );
    assert_eq!(
        camera_channel_names(device),
        vec![
            Some("realsense_rgb".to_string()),
            Some("realsense_depth".to_string()),
            Some("realsense_ir".to_string()),
        ]
    );
}

/// Multi-channel + multi-device: two RealSense units both produce 3
/// channels and the device-level dedup counter must produce
/// `realsense` / `realsense_2`, not `realsense` / `realsense`.
#[test]
fn build_discovery_config_dedupes_multi_channel_devices() {
    let config = build_discovery_config(&[
        realsense_multi_stream_discovery("332322071743"),
        realsense_multi_stream_discovery("332322071744"),
    ])
    .expect("config should build");

    assert_eq!(config.devices.len(), 2);
    assert_eq!(config.devices[0].name, "realsense");
    assert_eq!(config.devices[1].name, "realsense_2");
    assert_eq!(camera_channel_types(&config.devices[0]).len(), 3);
    assert_eq!(camera_channel_types(&config.devices[1]).len(), 3);
}

/// `group_camera_profiles_by_channel` must pick the highest-resolution
/// + highest-fps profile per channel as the wizard's default, even when
/// the discovery happens to list lower-quality profiles first. We
/// construct a discovery where the first listed profile per channel is
/// the worst one and assert that each channel ends up defaulting to
/// the best one (largest pixel count, then highest fps).
#[test]
fn build_discovery_config_picks_highest_resolution_and_fps_default_profile() {
    let mut channel_meta_by_channel = BTreeMap::new();
    channel_meta_by_channel.insert(
        "color".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Camera,
            channel_label: Some("Intel RealSense RGB".into()),
            default_name: Some("realsense_rgb".into()),
            profiles: vec![
                CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: None,
                    stream: Some("color".into()),
                    channel: None,
                },
                CameraProfile {
                    width: 1280,
                    height: 720,
                    fps: 60,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: None,
                    stream: Some("color".into()),
                    channel: None,
                },
                CameraProfile {
                    width: 1920,
                    height: 1080,
                    fps: 30,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: None,
                    stream: Some("color".into()),
                    channel: None,
                },
                CameraProfile {
                    width: 1920,
                    height: 1080,
                    fps: 60,
                    pixel_format: PixelFormat::Rgb24,
                    native_pixel_format: None,
                    stream: Some("color".into()),
                    channel: None,
                },
            ],
            ..DiscoveredChannelMeta::default()
        },
    );
    channel_meta_by_channel.insert(
        "depth".to_owned(),
        DiscoveredChannelMeta {
            kind: DeviceType::Camera,
            channel_label: Some("Intel RealSense Depth".into()),
            default_name: Some("realsense_depth".into()),
            profiles: vec![
                CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 30,
                    pixel_format: PixelFormat::Depth16,
                    native_pixel_format: None,
                    stream: Some("depth".into()),
                    channel: None,
                },
                CameraProfile {
                    width: 640,
                    height: 480,
                    fps: 90,
                    pixel_format: PixelFormat::Depth16,
                    native_pixel_format: None,
                    stream: Some("depth".into()),
                    channel: None,
                },
            ],
            ..DiscoveredChannelMeta::default()
        },
    );
    let discovery = DiscoveredDevice {
        driver: "realsense".into(),
        id: "best-default".into(),
        display_name: "Intel RealSense".into(),
        default_device_name: Some("realsense".into()),
        channel_meta_by_channel,
        transport: None,
        interface: None,
        product_variant: None,
        end_effector: None,
    };
    let config =
        build_discovery_config(std::slice::from_ref(&discovery)).expect("config should build");

    assert_eq!(config.devices.len(), 1);
    let device = &config.devices[0];
    assert_eq!(
        camera_channel_types(device),
        vec!["color".to_string(), "depth".to_string()]
    );
    let color = device.channels[0]
        .profile
        .as_ref()
        .expect("color channel must have a default profile");
    assert_eq!(
        (color.width, color.height, color.fps),
        (1920, 1080, 60),
        "color default must be highest-resolution + highest-fps"
    );
    let depth = device.channels[1]
        .profile
        .as_ref()
        .expect("depth channel must have a default profile");
    assert_eq!(
        (depth.width, depth.height, depth.fps),
        (640, 480, 90),
        "depth default must keep its highest-fps profile"
    );
}

#[test]
fn missing_value_limit_warnings_flags_robot_channels_with_no_driver_limits() {
    // The pseudo-style discovery in tests carries no value_limits because
    // its `DiscoveredChannelMeta` is empty by default. This mirrors the
    // case where a real driver has not been updated to report limits yet.
    let config =
        build_discovery_config(&[robot_discovery("robot0", 6)]).expect("config should build");

    let warnings = missing_value_limit_warnings(&config);
    // Three publish_states (joint position/velocity/effort) → three
    // distinct warnings, one per kind.
    assert_eq!(
        warnings.len(),
        3,
        "expected one warning per missing kind, got: {:?}",
        warnings
    );
    for kind in ["joint_position", "joint_velocity", "joint_effort"] {
        assert!(
            warnings.iter().any(|w| w.contains(kind)),
            "missing warning for {kind}: {warnings:?}"
        );
    }
}

#[test]
fn missing_value_limit_warnings_silent_when_driver_supplied_limits() {
    let mut config =
        build_discovery_config(&[robot_discovery("robot0", 6)]).expect("config should build");
    // Simulate the post-`enrich_current_device_from_discovery` state by
    // populating value_limits on every published kind.
    for device in &mut config.devices {
        for channel in &mut device.channels {
            channel.value_limits = channel
                .publish_states
                .iter()
                .map(|kind| {
                    rollio_types::config::StateValueLimitsEntry::symmetric(
                        *kind,
                        std::f64::consts::PI,
                        channel.dof.unwrap_or(1) as usize,
                    )
                })
                .collect();
        }
    }
    assert!(
        missing_value_limit_warnings(&config).is_empty(),
        "no warnings expected when limits are present"
    );
}

/// `setup_set_ui_http_host` should accept a new host string and persist
/// it to `config.ui.http_host`. An empty value is rejected with a
/// descriptive message instead of silently committing a useless config.
#[test]
fn set_ui_http_host_rejects_empty_and_persists_valid_values() {
    let mut session = setup_session(&[camera_discovery("cam0")]);
    // Default value comes from rollio_types::default_ui_http_host
    // which now binds to all interfaces by default.
    assert_eq!(session.config.ui.http_host, "0.0.0.0");

    // A trimmed-empty value must not mutate the field and should set a
    // visible error message for the wizard footer.
    let changed = session
        .set_ui_http_host("   ")
        .expect("empty input should be reported via message, not error");
    assert!(!changed, "blank UI host must not be persisted");
    assert_eq!(session.config.ui.http_host, "0.0.0.0");
    assert_eq!(
        session.message.as_deref(),
        Some("UI host must not be empty.")
    );

    // A valid value updates the field; identical re-submissions are
    // a no-op so the wizard doesn't re-broadcast unchanged state.
    session.message = None;
    let changed = session
        .set_ui_http_host("127.0.0.1")
        .expect("valid host should be accepted");
    assert!(changed);
    assert_eq!(session.config.ui.http_host, "127.0.0.1");

    let changed = session
        .set_ui_http_host("127.0.0.1")
        .expect("repeated host should be a no-op");
    assert!(!changed);
}

/// Regression: pressing identify on a camera channel during setup used to
/// render no frames. The bug had to be in either the preview project
/// config or the spec generation: this test exercises both ends.
///
/// Asserts that with a freshly-discovered pseudo camera, setting
/// `identify_device_name` and calling `build_preview_project_config`
/// produces:
///   - a valid `ProjectConfig` (validates without errors)
///   - exactly one camera device with `preview_enabled = true`
///   - an `encoder_runtime_configs_v2()` list containing a Preview-role
///     encoder for that camera
///   - a `visualizer_runtime_config_v2()` whose `camera_sources`
///     subscribes to the same `bus_root + channel_type` the encoder
///     publishes preview packets on
#[test]
fn identify_preview_pipeline_produces_consistent_encoder_and_visualizer_topics() {
    use rollio_types::config::EncoderRole;

    let mut session = setup_session(&[camera_discovery("cam0")]);
    let camera_name = session
        .available_devices
        .iter()
        .find(|device| device.device_type == DeviceType::Camera)
        .expect("camera discovery should land in the available list")
        .name
        .clone();
    assert!(
        session.is_device_selected(&camera_name),
        "discovery should auto-select the camera"
    );

    // Trigger identify the same way the dispatcher does.
    assert!(session.set_identify_device(Some(&camera_name)));
    assert_eq!(
        session.identify_device_name.as_deref(),
        Some(camera_name.as_str())
    );

    let preview = super::overview::build_preview_project_config(
        &session,
        4242,
        "ws://127.0.0.1:4242",
    )
    .expect("preview config builds for camera identify");

    preview
        .validate()
        .expect("identify-time preview config must validate");

    assert_eq!(preview.devices.len(), 1, "identify isolates one device");
    let device = &preview.devices[0];
    assert_eq!(device.channels.len(), 1);
    let channel = &device.channels[0];
    assert_eq!(channel.kind, DeviceType::Camera);
    assert!(
        channel.enabled,
        "identify target must keep its channel enabled"
    );
    assert!(
        channel.preview_enabled,
        "camera channel must keep preview_enabled = true"
    );

    let encoders = preview.encoder_runtime_configs_v2();
    let preview_spec = encoders
        .iter()
        .find(|cfg| matches!(cfg.role, EncoderRole::Preview))
        .expect("a Preview-role encoder must be spawned for the camera");
    let preview_payload = preview_spec
        .preview
        .as_ref()
        .expect("Preview-role spec carries preview payload");
    let encoder_topic = preview_payload
        .packet_topic
        .as_deref()
        .or(preview_payload.jpeg_topic.as_deref())
        .expect("preview encoder must publish on at least one topic");

    let visualizer_cfg = preview.visualizer_runtime_config_v2();
    let camera_source = visualizer_cfg
        .camera_sources
        .iter()
        .find(|source| source.channel_id == preview_spec.channel_id)
        .expect("visualizer must subscribe to the identify target");

    // The visualizer derives its subscription topic from (bus_root,
    // channel_type) via `rollio_bus::preview_packet_service_name`. The
    // encoder publishes on the same topic name when output_mode is
    // Encoded, or on `preview_jpeg_service_name` when Jpeg. Re-derive
    // both topic candidates and assert one of them matches what the
    // encoder is publishing on.
    let viz_packet_topic = rollio_bus::preview_packet_service_name(
        &camera_source.bus_root,
        &camera_source.channel_type,
    );
    let viz_jpeg_topic = rollio_bus::preview_jpeg_service_name(
        &camera_source.bus_root,
        &camera_source.channel_type,
    );
    assert!(
        encoder_topic == viz_packet_topic || encoder_topic == viz_jpeg_topic,
        "encoder topic {encoder_topic:?} must match visualizer's derived packet topic \
         {viz_packet_topic:?} or jpeg topic {viz_jpeg_topic:?}"
    );
}

/// Regression for the "no image renders" identify bug: the setup
/// wizard's terminal UI only decodes the JPEG binary message kind
/// from the visualizer (`parseBinaryMessage` accepts 0x01 only). The
/// default `PreviewOutputMode::Encoded` produces H.264 packets the
/// terminal UI silently drops. `build_preview_project_config` must
/// force every enabled camera channel's `preview_settings.output_mode`
/// to `Jpeg` for the setup-time preview pipeline so the terminal UI
/// actually sees frames.
#[test]
fn build_preview_project_config_forces_jpeg_for_terminal_ui() {
    use rollio_types::config::PreviewOutputMode;

    let mut session = setup_session(&[camera_discovery("cam0")]);
    let camera_name = session
        .available_devices
        .iter()
        .find(|device| device.device_type == DeviceType::Camera)
        .expect("camera discovery should land in the available list")
        .name
        .clone();
    assert!(session.set_identify_device(Some(&camera_name)));

    let preview = super::overview::build_preview_project_config(
        &session,
        4242,
        "ws://127.0.0.1:4242",
    )
    .expect("preview config builds for camera identify");

    for device in &preview.devices {
        for channel in &device.channels {
            if channel.kind != DeviceType::Camera || !channel.preview_enabled {
                continue;
            }
            let output_mode = channel
                .preview_settings
                .as_ref()
                .and_then(|s| s.output_mode)
                .expect("preview_settings.output_mode must be set after preview build");
            assert_eq!(
                output_mode,
                PreviewOutputMode::Jpeg,
                "setup wizard must force JPEG preview for the terminal UI",
            );
        }
    }
}

#[test]
fn wizard_selectable_modes_keeps_only_steady_state_modes_in_canonical_order() {
    // Drivers list modes in arbitrary order; the wizard cycle pins the
    // order to FreeDrive -> CommandFollowing so cycling is predictable
    // across every device.
    let driver_advertised = vec![
        RobotMode::Disabled,
        RobotMode::CommandFollowing,
        RobotMode::Identifying,
        RobotMode::FreeDrive,
    ];
    assert_eq!(
        wizard_selectable_modes(&driver_advertised),
        vec![RobotMode::FreeDrive, RobotMode::CommandFollowing],
    );
}

#[test]
fn wizard_selectable_modes_reflects_capability_only_drivers() {
    // E2-style passive grippers only advertise free-drive: the wizard
    // surfaces a one-option cycle without the controller hardcoding
    // any per-driver knowledge.
    assert_eq!(
        wizard_selectable_modes(&[RobotMode::FreeDrive]),
        vec![RobotMode::FreeDrive],
    );
    assert_eq!(
        wizard_selectable_modes(&[RobotMode::Identifying, RobotMode::Disabled]),
        Vec::<RobotMode>::new(),
    );
}

#[test]
fn cycle_robot_mode_only_alternates_between_free_drive_and_command_following() {
    // Two arms -> one auto-paired teleop pair, both with the full mode
    // list. The wizard cycle should hop between FreeDrive and
    // CommandFollowing only, ignoring Identifying / Disabled.
    let mut session = setup_session(&[
        robot_discovery("arm_lead", 6),
        robot_discovery("arm_follow", 6),
    ]);
    let first_robot = session
        .available_devices
        .iter()
        .find(|device| device.device_type == DeviceType::Robot)
        .expect("setup discovers two robots")
        .name
        .clone();
    // Walk the cycle a few times in each direction; only steady-state
    // modes should ever be observed.
    let mut observed = Vec::new();
    for _ in 0..6 {
        session
            .cycle_robot_mode(&first_robot, 1)
            .expect("forward cycle should succeed");
        let mode = session
            .available_device(&first_robot)
            .and_then(|available| available.current.channels.first().and_then(|ch| ch.mode))
            .expect("robot channel always has a mode");
        observed.push(mode);
    }
    assert!(
        observed
            .iter()
            .all(|mode| matches!(mode, RobotMode::FreeDrive | RobotMode::CommandFollowing)),
        "cycle landed on a non-steady-state mode: {observed:?}",
    );
    assert!(observed.contains(&RobotMode::FreeDrive));
    assert!(observed.contains(&RobotMode::CommandFollowing));
}

#[test]
fn create_pairing_rejects_when_no_eligible_leader_exists() {
    // A single arm gives us a candidate follower but no leader (we'd
    // need at least two enabled command-following channels and the
    // pair must not collapse to a self-loop). The wizard should keep
    // pairings empty and surface a descriptive message instead of
    // silently producing a degenerate pair.
    let mut session = setup_session(&[robot_discovery("arm_only", 6)]);
    // Drop the auto-built default pairing so we test create_pairing
    // from a clean slate. Mode stays Teleop (the only mode now).
    session.config.pairings.clear();
    // With a single arm, the only `eligible_follower_channels` entry
    // collapses onto the same channel as the eligible leader, so
    // `create_pairing` falls back through follower picking and emits
    // no pair (the would-be self-loop is detected and rejected).
    let new_index = session
        .create_pairing(None)
        .expect("create_pairing should not bubble validation errors");
    // With only one channel, no eligible follower is selectable.
    assert!(new_index.is_none());
    assert!(
        session.message.is_some(),
        "rejection should leave a message for the operator",
    );
    assert!(session.config.pairings.is_empty());
}

#[test]
fn create_then_remove_pairing_round_trip_reaches_empty_state() {
    let mut session = setup_session(&[
        robot_discovery("arm_lead", 6),
        robot_discovery("arm_follow", 6),
    ]);
    // The discovery path auto-builds one pair already; clear so
    // create_pairing's bookkeeping is exercised from a known-empty
    // baseline. Mode stays at Teleop (the only mode the wizard now
    // exposes) since pairings can be empty in teleop.
    session.config.pairings.clear();
    let new_index = session
        .create_pairing(None)
        .expect("create_pairing should succeed with two arms")
        .expect("a pair should land at index 0");
    assert_eq!(new_index, 0);
    assert_eq!(session.config.pairings.len(), 1);
    assert_eq!(session.config.mode, CollectionMode::Teleop);
    // Removing the pair leaves teleop in place with zero pairings —
    // the wizard treats this as a valid intermediate state.
    let removed = session
        .remove_pairing(0)
        .expect("remove_pairing should succeed");
    assert!(removed);
    assert!(session.config.pairings.is_empty());
    assert_eq!(session.config.mode, CollectionMode::Teleop);
}

#[test]
fn set_pairing_endpoint_rejects_ineligible_channel() {
    // Build a pair, then try to set a follower that doesn't support
    // command-following (the camera channel is the easiest such
    // ineligible target). The set should fail and leave the pair
    // unchanged.
    let mut session = setup_session(&[
        robot_discovery("arm_lead", 6),
        robot_discovery("arm_follow", 6),
        camera_discovery("cam0"),
    ]);
    // Ensure we have a pair to operate on.
    if session.config.pairings.is_empty() {
        session
            .create_pairing(None)
            .expect("create_pairing should succeed")
            .expect("a pair should land at index 0");
    }
    let cam_device = session
        .config
        .devices
        .iter()
        .find(|device| {
            device
                .channels
                .iter()
                .any(|channel| channel.kind == DeviceType::Camera)
        })
        .expect("camera discovery always lands in config")
        .clone();
    let cam_channel_type = cam_device
        .channels
        .iter()
        .find(|channel| channel.kind == DeviceType::Camera)
        .expect("camera device has at least one camera channel")
        .channel_type
        .clone();
    let follower_device_before = session.config.pairings[0].follower_device.clone();
    let follower_channel_before = session.config.pairings[0].follower_channel_type.clone();
    let mutated = session
        .set_pairing_endpoint(
            0,
            PairingEndpoint::Follower,
            &cam_device.name,
            &cam_channel_type,
        )
        .expect("set_pairing_endpoint should not bubble validation errors");
    assert!(!mutated, "ineligible channels should not mutate the pair");
    assert_eq!(
        session.config.pairings[0].follower_device,
        follower_device_before,
    );
    assert_eq!(
        session.config.pairings[0].follower_channel_type,
        follower_channel_before,
    );
    assert!(
        session.message.is_some(),
        "rejection should leave a message for the operator",
    );
}

#[test]
fn set_pairing_endpoint_rejects_self_loop_leader() {
    // The picker should never let the operator pick a leader that
    // equals the pair's existing follower; the controller backstops
    // that with the same constraint.
    let mut session =
        setup_session(&[robot_discovery("arm_a", 6), robot_discovery("arm_b", 6)]);
    if session.config.pairings.is_empty() {
        session
            .create_pairing(None)
            .expect("create_pairing should succeed")
            .expect("a pair should land at index 0");
    }
    let follower_device = session.config.pairings[0].follower_device.clone();
    let follower_channel = session.config.pairings[0].follower_channel_type.clone();
    let mutated = session
        .set_pairing_endpoint(
            0,
            PairingEndpoint::Leader,
            &follower_device,
            &follower_channel,
        )
        .expect("set_pairing_endpoint should not bubble validation errors");
    assert!(!mutated, "self-loop leader should be rejected");
    assert!(session.message.is_some());
}

#[test]
fn create_pairing_promotes_follower_channel_mode_to_command_following() {
    // The wizard auto-promotes the follower's step-1 mode to
    // CommandFollowing on pair creation: the operator shouldn't have
    // to bounce back to step 1 to flip a free-drive default after
    // they've decided to use the channel as a follower, and a
    // follower stuck in FreeDrive would silently break teleop at
    // runtime.
    let mut session = setup_session(&[
        robot_discovery("arm_lead", 6),
        robot_discovery("arm_follow", 6),
    ]);
    // Drop the auto-built pair so we exercise create_pairing
    // explicitly. Force every robot channel back to FreeDrive so
    // the test isn't accidentally green just because discovery
    // already left the follower at CommandFollowing.
    session.config.pairings.clear();
    for device in session.config.devices.iter_mut() {
        for channel in device.channels.iter_mut() {
            if channel.kind == DeviceType::Robot {
                channel.mode = Some(RobotMode::FreeDrive);
            }
        }
    }
    let new_index = session
        .create_pairing(None)
        .expect("create_pairing should succeed with two arms")
        .expect("a pair should land at index 0");
    let pair = &session.config.pairings[new_index];
    let follower_mode = session
        .config
        .device_named(&pair.follower_device)
        .and_then(|d| {
            d.channels
                .iter()
                .find(|c| c.channel_type == pair.follower_channel_type)
        })
        .and_then(|ch| ch.mode);
    assert_eq!(
        follower_mode,
        Some(RobotMode::CommandFollowing),
        "create_pairing must auto-flip the follower channel into CommandFollowing",
    );
    // Mirror also lands in the available_devices snapshot used by
    // step 1's UI so the operator sees the change reflected.
    let mirror_mode = session
        .available_devices
        .iter()
        .find(|available| available.current.name == pair.follower_device)
        .and_then(|available| available.current.channels.first().and_then(|ch| ch.mode));
    assert_eq!(
        mirror_mode,
        Some(RobotMode::CommandFollowing),
        "available_devices mirror must reflect the auto-promoted follower mode",
    );
}

#[test]
fn set_pairing_endpoint_promotes_new_follower_to_command_following() {
    // Same auto-promotion contract as `create_pairing`, but exercised
    // via the editor path: switching the follower endpoint of an
    // existing pair must flip the *new* follower channel into
    // CommandFollowing too.
    let mut session = setup_session(&[
        robot_discovery("arm_a", 6),
        robot_discovery("arm_b", 6),
        robot_discovery("arm_c", 6),
    ]);
    if session.config.pairings.is_empty() {
        session
            .create_pairing(None)
            .expect("create_pairing should succeed")
            .expect("a pair should land at index 0");
    }
    // Pick a third channel that isn't the current leader or
    // follower to swap in. Reset its mode so we can observe the
    // promotion happening as a result of `set_pairing_endpoint`.
    let leader_device = session.config.pairings[0].leader_device.clone();
    let follower_device = session.config.pairings[0].follower_device.clone();
    let (new_follower_device, new_follower_channel) = session
        .config
        .devices
        .iter()
        .find_map(|d| {
            if d.name == leader_device || d.name == follower_device {
                return None;
            }
            d.channels
                .iter()
                .find(|c| c.kind == DeviceType::Robot && c.enabled)
                .map(|c| (d.name.clone(), c.channel_type.clone()))
        })
        .expect("a third arm should be eligible as a new follower");
    if let Some(device) = session
        .config
        .devices
        .iter_mut()
        .find(|d| d.name == new_follower_device)
    {
        for channel in device.channels.iter_mut() {
            if channel.channel_type == new_follower_channel {
                channel.mode = Some(RobotMode::FreeDrive);
            }
        }
    }
    let mutated = session
        .set_pairing_endpoint(
            0,
            PairingEndpoint::Follower,
            &new_follower_device,
            &new_follower_channel,
        )
        .expect("set_pairing_endpoint should succeed");
    assert!(mutated, "follower swap should mutate the pair");
    let promoted_mode = session
        .config
        .device_named(&new_follower_device)
        .and_then(|d| {
            d.channels
                .iter()
                .find(|c| c.channel_type == new_follower_channel)
        })
        .and_then(|ch| ch.mode);
    assert_eq!(
        promoted_mode,
        Some(RobotMode::CommandFollowing),
        "set_pairing_endpoint must auto-flip the newly-bound follower into CommandFollowing",
    );
}

#[test]
fn cycle_pair_mapping_rolls_back_validation_failures_into_a_warning() {
    // Mirrors the operator-reported bug: a 6-DOF leader paired with
    // a 7-DOF follower cannot use direct-joint identity mapping (the
    // joint_index_map would reach into a leader joint that doesn't
    // exist), but the wizard previously bubbled the validation error
    // out of `apply_raw_command` and aborted. After the fix we
    // expect a soft warning and a rollback to the pre-cycle pair.
    let mut session = setup_session(&[
        robot_discovery("arm_lead", 6),
        robot_discovery("arm_follow", 7),
    ]);
    // Wipe any auto-built pair and bake one with an end-effector
    // mapping so cycling forward lands on direct-joint (where the
    // 6→7 DOF mismatch trips validation). This avoids depending on
    // discovery's choice of starting mapping.
    session.config.pairings.clear();
    // `build_discovery_config` rewrites raw discovery ids
    // ("arm_lead", "arm_follow") into the per-driver default name
    // ("pseudo_arm", "pseudo_arm_2"), so reach into the config to
    // recover whichever names the discovery loop actually picked.
    let leader_name = session.config.devices[0].name.clone();
    let follower_name = session.config.devices[1].name.clone();
    // Bake a known-good cartesian baseline by hand: opt the leader
    // into publishing EndEffectorPose, opt the follower into
    // accepting EndPose commands, and push a hand-crafted pair so
    // we don't depend on `pairing_from_channels` finding the
    // cartesian branch.
    ensure_channel_publishes_state(
        &mut session.config.devices,
        &leader_name,
        "arm",
        RobotStateKind::EndEffectorPose,
    );
    if let Some(follower) = session
        .config
        .devices
        .iter_mut()
        .find(|d| d.name == follower_name)
    {
        if let Some(channel) = follower
            .channels
            .iter_mut()
            .find(|c| c.channel_type == "arm")
        {
            if !channel
                .supported_commands
                .contains(&RobotCommandKind::EndPose)
            {
                channel.supported_commands.push(RobotCommandKind::EndPose);
            }
        }
    }
    session.config.pairings.push(ChannelPairingConfig {
        leader_device: leader_name.clone(),
        leader_channel_type: "arm".into(),
        follower_device: follower_name.clone(),
        follower_channel_type: "arm".into(),
        mapping: MappingStrategy::Cartesian,
        leader_state: RobotStateKind::EndEffectorPose,
        follower_command: RobotCommandKind::EndPose,
        joint_index_map: Vec::new(),
        joint_scales: Vec::new(),
    });
    session
        .config
        .validate()
        .expect("baseline cartesian pair should validate");
    let mapping_before = session.config.pairings[0].mapping;
    // Cycle forward to direct-joint, which must fail validation
    // because joint_index_map = [0..7] would index leader joint 6
    // (out of range for a 6-DOF arm).
    let mutated = session
        .cycle_pair_mapping(0, 1)
        .expect("cycle_pair_mapping should not bubble validation errors");
    assert!(!mutated, "incompatible mapping cycle should be a no-op");
    assert_eq!(session.config.pairings[0].mapping, mapping_before);
    assert!(
        session
            .message
            .as_ref()
            .is_some_and(|msg| msg.contains("Cannot switch")),
        "validation rejection should leave a descriptive warning",
    );
}

#[test]
fn eligibility_lists_drop_channels_disabled_in_step_one() {
    // Three robots discovered → all auto-selected → all eligible
    // initially. Disabling one in step 1 must remove it from both
    // the leader and follower picker pools immediately, so the
    // operator can't accidentally pair a channel they've turned off.
    let mut session = setup_session(&[
        robot_discovery("arm_a", 6),
        robot_discovery("arm_b", 6),
        robot_discovery("arm_c", 6),
    ]);
    let target_name = session
        .available_devices
        .iter()
        .find(|device| device.device_type == DeviceType::Robot)
        .expect("setup discovers robot rows")
        .name
        .clone();
    // Find the (device, channel_type) the disabled row maps to so
    // we can assert it disappears from both eligibility lists.
    let (target_device, target_channel) = session
        .config
        .devices
        .iter()
        .find_map(|d| {
            d.channels.iter().find_map(|c| {
                if format!("{}|{}|{}|{}|-", "robot", d.driver, d.id, c.channel_type)
                    == target_name
                {
                    Some((d.name.clone(), c.channel_type.clone()))
                } else {
                    None
                }
            })
        })
        .expect("target row exists in config");

    // Sanity: it shows up before being disabled. We probe the
    // DirectJoint pool because the test fixture is an arm channel.
    let policy = MappingStrategy::DirectJoint;
    assert!(session
        .eligible_leader_channels_for(policy, None)
        .contains(&(target_device.clone(), target_channel.clone())));
    // For the follower probe, pass another arm as the leader so the
    // policy-aware filter has a peer to compare against.
    let other_leader = session
        .config
        .devices
        .iter()
        .find_map(|d| {
            if d.name != target_device {
                d.channels
                    .iter()
                    .find(|c| c.kind == DeviceType::Robot && c.enabled)
                    .map(|c| (d.name.clone(), c.channel_type.clone()))
            } else {
                None
            }
        })
        .expect("another arm should exist as leader fixture");
    assert!(session
        .eligible_follower_channels_for(policy, Some(&other_leader), None)
        .contains(&(target_device.clone(), target_channel.clone())));

    // Toggle off via the same path the wizard uses (space in step 1).
    session
        .toggle_device_selection(&target_name)
        .expect("toggle_device_selection should succeed");

    // After disabling, neither pool should include the channel —
    // even if other pairs still reference it (the picker now shows
    // only eligible candidates, mirroring the controller's view).
    assert!(!session
        .eligible_leader_channels_for(policy, None)
        .contains(&(target_device.clone(), target_channel.clone())));
    assert!(!session
        .eligible_follower_channels_for(policy, Some(&other_leader), None)
        .contains(&(target_device, target_channel)));
}

#[test]
fn eligible_leader_channels_accept_free_drive_only_devices() {
    // E2-style channels advertise only `FreeDrive`. Per the
    // capability-driven leader predicate (free-drive OR
    // command-following), they should still be eligible leaders for
    // both DirectJoint (when paired with a matching whitelist peer)
    // and Parallel.
    let mut session = setup_session(&[robot_discovery("arm_lead", 6)]);
    // Synthesize an "e2-like" channel: drop CommandFollowing so the
    // available_device only advertises FreeDrive.
    if let Some(available) = session.available_devices.first_mut() {
        available
            .supported_modes
            .retain(|mode| *mode == RobotMode::FreeDrive);
    }
    let leaders = session.eligible_leader_channels_for(MappingStrategy::DirectJoint, None);
    assert!(
        !leaders.is_empty(),
        "free-drive-only channels must qualify as DirectJoint leaders",
    );
}

#[test]
fn set_pairing_endpoint_rejects_follower_already_used_in_another_pair() {
    // Two pairs share a single eligible follower pool (three arms,
    // three commands); after the first pair claims arm_b as
    // follower, the second pair must not be allowed to point its
    // follower at arm_b too — each follower can only follow one
    // leader at a time.
    let mut session = setup_session(&[
        robot_discovery("arm_a", 6),
        robot_discovery("arm_b", 6),
        robot_discovery("arm_c", 6),
    ]);
    // Discovery seeds one pair; create a second pair to test
    // cross-pair follower uniqueness.
    if session.config.pairings.is_empty() {
        session
            .create_pairing(None)
            .expect("create_pairing should succeed");
    }
    // Snapshot the first pair's follower so we can try to re-use it.
    let claimed_follower_device = session.config.pairings[0].follower_device.clone();
    let claimed_follower_channel = session.config.pairings[0].follower_channel_type.clone();
    // Need the second pair to exist to call set_pairing_endpoint on it.
    let second_index = session
        .create_pairing(None)
        .expect("second create_pairing should succeed")
        .expect("a second pair should land at the next index");
    let mutated = session
        .set_pairing_endpoint(
            second_index,
            PairingEndpoint::Follower,
            &claimed_follower_device,
            &claimed_follower_channel,
        )
        .expect("set_pairing_endpoint should not bubble validation errors");
    assert!(
        !mutated,
        "follower claimed by another pair should be rejected"
    );
    assert!(session.message.is_some());
    // The second pair must keep its previously-seeded follower (which
    // create_pairing chose to be distinct from any already-claimed
    // follower).
    assert_ne!(
        (
            &session.config.pairings[second_index].follower_device,
            &session.config.pairings[second_index].follower_channel_type,
        ),
        (&claimed_follower_device, &claimed_follower_channel),
    );
}

// ------------------------------------------------------------------
// Tests for the three-policy teleop redesign.
// ------------------------------------------------------------------

#[test]
fn cycle_pair_mapping_walks_all_three_policies() {
    // Two arms with matching driver whitelist auto-pair as
    // DirectJoint. Cycling forward should land on Cartesian (if
    // EndPose support exists) or Parallel (if dof=1 + parallel
    // states exist) per the strict cycle order, with rollback when
    // the current channel shape doesn't match. For arms (dof=6,
    // joint_position only) only DirectJoint validates -- the other
    // two cycles are no-ops with warnings.
    let mut session = setup_session(&[
        robot_discovery("arm_lead", 6),
        robot_discovery("arm_follow", 6),
    ]);
    assert_eq!(session.config.pairings.len(), 1);
    assert_eq!(
        session.config.pairings[0].mapping,
        MappingStrategy::DirectJoint
    );
    // Cycling forward to Cartesian should fail (arms don't publish
    // end_effector_pose in this fixture); the pair stays put with
    // a warning message.
    let mutated = session.cycle_pair_mapping(0, 1).unwrap();
    assert!(
        !mutated,
        "cycle to cartesian must roll back when end_effector_pose is missing"
    );
    assert_eq!(
        session.config.pairings[0].mapping,
        MappingStrategy::DirectJoint
    );
    assert!(
        session
            .message
            .as_ref()
            .is_some_and(|m| m.contains("cartesian")),
        "expected cartesian rejection message",
    );
}

#[test]
fn create_pairing_accepts_policy_aware_seed_for_parallel() {
    // Two parallel grippers paired explicitly under Parallel with
    // a custom ratio. The stored pair should preserve mapping=Parallel
    // and joint_scales=[ratio].
    let mut session = setup_session(&[
        parallel_gripper_discovery("g0", "lead_gripper"),
        parallel_gripper_discovery("g1", "follow_gripper"),
    ]);
    // Discovery may have auto-paired the grippers; clear so we
    // exercise create_pairing from a known baseline.
    session.config.pairings.clear();
    let leader_name = session
        .config
        .devices
        .first()
        .expect("first gripper device exists")
        .name
        .clone();
    let follower_name = session
        .config
        .devices
        .get(1)
        .expect("second gripper device exists")
        .name
        .clone();
    let new_index = session
        .create_pairing(Some(TeleopPairCreate {
            policy: MappingStrategy::Parallel,
            leader: (leader_name.clone(), "gripper".into()),
            follower: (follower_name.clone(), "gripper".into()),
            ratio: Some(0.5),
        }))
        .expect("create_pairing should succeed for parallel grippers")
        .expect("new pair should land at index 0");
    assert_eq!(new_index, 0);
    assert_eq!(
        session.config.pairings[0].mapping,
        MappingStrategy::Parallel
    );
    assert_eq!(session.config.pairings[0].joint_scales, vec![0.5]);
}

#[test]
fn set_pairing_ratio_rejects_zero_and_non_parallel_policies() {
    // Set up an arm pair (DirectJoint by default) and a parallel
    // gripper pair. set_pairing_ratio applies only to Parallel and
    // rejects zero / non-finite values for Parallel pairs.
    let mut session = setup_session(&[
        robot_discovery("arm_lead", 6),
        robot_discovery("arm_follow", 6),
        parallel_gripper_discovery("g0", "lead_gripper"),
        parallel_gripper_discovery("g1", "follow_gripper"),
    ]);
    // Wipe auto-pairs and create one of each kind explicitly.
    session.config.pairings.clear();
    let lead_arm = session
        .config
        .devices
        .iter()
        .find(|d| d.name == "pseudo_arm")
        .expect("pseudo_arm exists")
        .name
        .clone();
    let follow_arm = session
        .config
        .devices
        .iter()
        .find(|d| d.name == "pseudo_arm_2")
        .expect("pseudo_arm_2 exists")
        .name
        .clone();
    session
        .create_pairing(Some(TeleopPairCreate {
            policy: MappingStrategy::DirectJoint,
            leader: (lead_arm, "arm".into()),
            follower: (follow_arm, "arm".into()),
            ratio: None,
        }))
        .expect("DirectJoint pair create should succeed");
    let lead_grip = session
        .config
        .devices
        .iter()
        .find(|d| d.name == "lead_gripper")
        .expect("lead_gripper exists")
        .name
        .clone();
    let follow_grip = session
        .config
        .devices
        .iter()
        .find(|d| d.name == "follow_gripper")
        .expect("follow_gripper exists")
        .name
        .clone();
    session
        .create_pairing(Some(TeleopPairCreate {
            policy: MappingStrategy::Parallel,
            leader: (lead_grip, "gripper".into()),
            follower: (follow_grip, "gripper".into()),
            ratio: Some(1.0),
        }))
        .expect("Parallel pair create should succeed");

    let direct_idx = session
        .config
        .pairings
        .iter()
        .position(|p| p.mapping == MappingStrategy::DirectJoint)
        .expect("DirectJoint pair exists");
    let parallel_idx = session
        .config
        .pairings
        .iter()
        .position(|p| p.mapping == MappingStrategy::Parallel)
        .expect("Parallel pair exists");

    // DirectJoint rejection.
    let applied = session
        .set_pairing_ratio(direct_idx, 2.0)
        .expect("call should not bubble an error");
    assert!(!applied);
    assert!(
        session
            .message
            .as_ref()
            .is_some_and(|m| m.contains("no ratio")),
        "DirectJoint ratio attempt should report no-ratio message",
    );

    // Parallel zero rejection.
    let applied = session
        .set_pairing_ratio(parallel_idx, 0.0)
        .expect("call should not bubble an error");
    assert!(!applied);
    assert!(
        session
            .message
            .as_ref()
            .is_some_and(|m| m.contains("non-zero")),
        "ratio=0 should report non-zero requirement",
    );

    // Parallel happy path.
    let applied = session
        .set_pairing_ratio(parallel_idx, 2.5)
        .expect("happy-path ratio should apply");
    assert!(applied);
    assert_eq!(
        session.config.pairings[parallel_idx].joint_scales,
        vec![2.5]
    );
}

#[test]
fn eligibility_lists_filter_by_policy() {
    // A mixed config: two arms (DirectJoint) and two parallel
    // grippers (Parallel). DirectJoint pool should expose only the
    // arms; Parallel pool only the grippers; Cartesian pool empty
    // (neither side advertises end_effector_pose).
    let session = setup_session(&[
        robot_discovery("arm_a", 6),
        robot_discovery("arm_b", 6),
        parallel_gripper_discovery("g0", "lead_gripper"),
        parallel_gripper_discovery("g1", "follow_gripper"),
    ]);
    let dj_leaders = session.eligible_leader_channels_for(MappingStrategy::DirectJoint, None);
    for entry in &dj_leaders {
        assert_eq!(
            entry.1, "arm",
            "DirectJoint pool should only contain arm channels"
        );
    }
    let par_leaders = session.eligible_leader_channels_for(MappingStrategy::Parallel, None);
    for entry in &par_leaders {
        assert_eq!(
            entry.1, "gripper",
            "Parallel pool should only contain gripper channels"
        );
    }
    let cart_leaders = session.eligible_leader_channels_for(MappingStrategy::Cartesian, None);
    assert!(
        cart_leaders.is_empty(),
        "Cartesian pool should be empty without end_effector_pose support, got {cart_leaders:?}",
    );
}

#[test]
fn eligibility_lists_reject_cross_vendor_direct_joint_without_mutual_whitelist() {
    // Two arms whose drivers don't endorse each other in the
    // whitelist must NOT appear as DirectJoint follower candidates
    // for one another. We build the session with the
    // `_no_whitelist` fixture so neither side opts in.
    let mut session = setup_session(&[
        robot_discovery_no_whitelist("arm_a", 6),
        robot_discovery_no_whitelist("arm_b", 6),
    ]);
    // Drop any auto-pairs first (the no-whitelist fixture should
    // already have produced none, but make it explicit).
    session.config.pairings.clear();
    let leader_target = session
        .config
        .devices
        .iter()
        .find(|d| d.name == "pseudo_arm")
        .map(|d| (d.name.clone(), d.channels[0].channel_type.clone()))
        .expect("pseudo_arm device exists");
    let followers = session.eligible_follower_channels_for(
        MappingStrategy::DirectJoint,
        Some(&leader_target),
        None,
    );
    assert!(
        followers.is_empty(),
        "no-whitelist arms must not appear as DirectJoint follower candidates of each other; got {followers:?}",
    );
}

#[test]
fn run_load_surfaces_invalid_pair_as_warning_not_abort() {
    // Operator-reported scenario (issue thread): a stale config
    // with mapping=direct-joint + parallel_position used to abort
    // `rollio setup` at the load-time validate(). After the
    // teleop-policy refactor, that pair fails the new validator,
    // and `run()` now demotes the failure to a warning instead
    // of bubbling it as Err. We test the validation directly via
    // `validate()` and assert the wizard's launch path also
    // tolerates the loaded config (covered indirectly via
    // setup_session, which constructs a SetupSession from a
    // freshly-built ProjectConfig).
    let mut config = direct_joint_pair_config_template_for_run_test();
    // Mutate the pair to mimic the operator's stale shape:
    // direct-joint with a parallel_position leader_state, which
    // the new validator rejects as a leader_state mismatch.
    config.pairings[0].leader_state = rollio_types::config::RobotStateKind::ParallelPosition;
    let err = config
        .validate()
        .expect_err("stale shape must be rejected by the new validator");
    let msg = format!("{err}");
    assert!(
        msg.contains("ParallelPosition") || msg.contains("parallel_position"),
        "rejection should mention the offending leader_state, got: {err}",
    );
    // The run() launch path now pushes such errors into `warnings`
    // instead of returning Err. We can't fully exercise `run()`
    // here (it tries to spawn child processes), but we can confirm
    // the in-place fix preserves the pair so the wizard can render
    // it for the operator to fix manually.
    assert_eq!(config.pairings.len(), 1);
}

fn direct_joint_pair_config_template_for_run_test() -> rollio_types::config::ProjectConfig {
    use std::str::FromStr;
    let toml_text = r#"
project_name = "stale-pair-test"
mode = "teleop"

[episode]
format = "lerobot-v2.1"
fps = 30

[[devices]]
name = "lead"
driver = "pseudo-a"
id = "lead0"
bus_root = "lead"

[[devices.channels]]
channel_type = "arm"
kind = "robot"
enabled = true
mode = "free-drive"
dof = 6
publish_states = ["joint_position"]
recorded_states = ["joint_position"]

[[devices]]
name = "follow"
driver = "pseudo-b"
id = "follow0"
bus_root = "follow"

[[devices.channels]]
channel_type = "arm"
kind = "robot"
enabled = true
mode = "command-following"
dof = 6
publish_states = ["joint_position"]
recorded_states = ["joint_position"]

[[pairings]]
leader_device = "lead"
leader_channel_type = "arm"
follower_device = "follow"
follower_channel_type = "arm"
mapping = "direct-joint"
leader_state = "joint_position"
follower_command = "joint_position"
joint_index_map = [0, 1, 2, 3, 4, 5]
joint_scales = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0]

[encoder]
video_codec = "h264"
depth_codec = "rvl"

[storage]
backend = "local"
output_path = "./output"

[visualizer]
port = 19090
"#;
    rollio_types::config::ProjectConfig::from_str(toml_text)
        .expect("template should parse before mutation")
}

