//! `query --json <id>` reports the gripper-cora capability: kind=Robot,
//! dof=1, supported_states=["joint_position","joint_velocity","joint_effort"].
//! The actual `publish_states` per-channel is chosen by the user in
//! `config.toml`; this query response just advertises what the driver can do.

use rollio_types::config::{
    ChannelKindInfo, DeviceQueryChannel, DeviceQueryDevice, DeviceQueryResponse, RobotChannelInfo,
    RobotStateKind,
};

use crate::driver_name;

pub const DEFAULT_CHANNEL_TYPE: &str = "gripper";
const DOF: u32 = 1;

pub fn run(id: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let channel = DeviceQueryChannel {
        channel_type: DEFAULT_CHANNEL_TYPE.to_string(),
        available: true,
        channel_label: Some("Cora JointState gripper (dof=1)".to_string()),
        default_name: Some("gripper".to_string()),
        info: ChannelKindInfo::Robot(RobotChannelInfo {
            modes: vec!["enabled".to_string(), "disabled".to_string()],
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
            direct_joint_compatibility: Default::default(),
            defaults: Default::default(),
            value_limits: Vec::new(),
        }),
        optional_info: toml::Table::new(),
    };
    let device = DeviceQueryDevice {
        id: id.to_string(),
        device_class: "robot".to_string(),
        device_label: "Cora JointState gripper passthrough".to_string(),
        default_device_name: Some("gripper_cora".to_string()),
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
            "{}: device id={} class=robot channel_type={} kind=robot dof={} supported_states=[joint_position, joint_velocity, joint_effort]",
            driver_name(),
            id,
            DEFAULT_CHANNEL_TYPE,
            DOF,
        );
    }
    Ok(())
}
