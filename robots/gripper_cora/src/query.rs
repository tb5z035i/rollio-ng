//! `query --json <id>` reports the gripper-cora capability: kind=Robot,
//! dof=1, supported_states=["joint_position","joint_velocity","joint_effort"].
//! The actual `publish_states` per-channel is chosen by the user in
//! `config.toml`; this query response just advertises what the driver can do.

use rollio_types::config::{
    ChannelCommandDefaults, DeviceQueryChannel, DeviceQueryDevice, DeviceQueryResponse, DeviceType,
    DirectJointCompatibility, RobotStateKind, StateValueLimitsEntry,
};

use crate::{descriptor, driver_name};

pub const DEFAULT_CHANNEL_TYPE: &str = "gripper";
const DOF: u32 = 1;

pub fn run(id: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let default_name = descriptor::name_from_id(id);
    let cora_topic = descriptor::topic_from_id(id);

    let value_limits = vec![
        StateValueLimitsEntry::symmetric(RobotStateKind::JointPosition, 0.1, 1),
        StateValueLimitsEntry::symmetric(RobotStateKind::JointVelocity, 1.0, 1),
        StateValueLimitsEntry::symmetric(RobotStateKind::JointEffort, 10.0, 1),
    ];

    let mut channel_extra = toml::Table::new();
    channel_extra.insert("cora_topic".to_string(), toml::Value::String(cora_topic));
    // joint_name placeholder: must match a name inside the publisher's
    // JointState.names[] at runtime. Operator typically overrides via TOML.
    channel_extra.insert(
        "joint_name".to_string(),
        toml::Value::String(default_name.clone()),
    );

    let channel = DeviceQueryChannel {
        channel_type: DEFAULT_CHANNEL_TYPE.to_string(),
        kind: DeviceType::Robot,
        available: true,
        channel_label: Some("Cora JointState gripper (dof=1)".to_string()),
        default_name: Some(DEFAULT_CHANNEL_TYPE.to_string()),
        modes: vec!["enabled".to_string(), "disabled".to_string()],
        profiles: Vec::new(),
        supported_states: vec![
            RobotStateKind::JointPosition,
            RobotStateKind::JointVelocity,
            RobotStateKind::JointEffort,
        ],
        supported_commands: Vec::new(),
        supports_fk: false,
        supports_ik: false,
        dof: Some(DOF),
        default_control_frequency_hz: None,
        direct_joint_compatibility: DirectJointCompatibility::default(),
        defaults: ChannelCommandDefaults::default(),
        value_limits,
        supported_sensor_kinds: Vec::new(),
        default_sample_rate_hz: None,
        sensor_shape_hints: Default::default(),
        optional_info: channel_extra,
    };
    let device = DeviceQueryDevice {
        id: id.to_string(),
        device_class: "robot".to_string(),
        device_label: "Cora JointState gripper passthrough".to_string(),
        default_device_name: Some(default_name.clone()),
        optional_info: toml::Table::new(),
        channels: vec![channel],
    };
    let response = DeviceQueryResponse {
        driver: driver_name().to_string(),
        devices: vec![device],
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!(
            "{}: device id={} name={} class=robot channel_type={} kind=robot dof={} supported_states=[joint_position, joint_velocity, joint_effort]",
            driver_name(),
            id,
            default_name,
            DEFAULT_CHANNEL_TYPE,
            DOF,
        );
    }
    Ok(())
}
