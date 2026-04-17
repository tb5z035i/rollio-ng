use serde::Serialize;
use toml::Value;

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSchema {
    pub format: &'static str,
    pub version: u32,
    pub sections: Vec<SchemaSection>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaSectionKind {
    Table,
    ArrayOfTables,
    DynamicTable,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaSection {
    pub name: &'static str,
    pub kind: SchemaSectionKind,
    pub description: &'static str,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<SchemaField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<&'static str>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub allows_extra_fields: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaField {
    pub name: &'static str,
    #[serde(rename = "type")]
    pub type_name: &'static str,
    pub description: &'static str,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<&'static str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applies_to: Option<Vec<&'static str>>,
}

pub fn config_schema() -> ConfigSchema {
    ConfigSchema {
        format: "rollio-config-schema",
        version: 1,
        sections: vec![
            SchemaSection {
                name: "root",
                kind: SchemaSectionKind::Table,
                description: "Top-level collection metadata and runtime mode.",
                fields: vec![
                    string_field_with_default(
                        "project_name",
                        "Logical project name embedded in saved configs and episode metadata.",
                        false,
                        "default",
                    ),
                    enum_field(
                        "mode",
                        "Collection semantics controlling whether teleoperation pairings are active.",
                        false,
                        "intervention",
                        &["teleop", "intervention"],
                    ),
                ],
                notes: vec![
                    "Legacy configs that omit mode infer teleop when [[pairing]] entries exist and intervention otherwise.",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "episode",
                kind: SchemaSectionKind::Table,
                description: "Episode-level recording and file-layout settings.",
                fields: vec![
                    enum_field(
                        "format",
                        "Episode container and directory layout.",
                        true,
                        "lerobot-v2.1",
                        &["lerobot-v2.1", "lerobot-v3.0", "mcap"],
                    ),
                    int_field("fps", "Target recording frame rate.", true, 30),
                    int_field(
                        "chunk_size",
                        "Number of frames per output chunk for chunked formats.",
                        false,
                        1000,
                    ),
                ],
                notes: vec![],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "controller",
                kind: SchemaSectionKind::Table,
                description: "Controller process orchestration settings.",
                fields: vec![
                    int_field(
                        "shutdown_timeout_ms",
                        "How long the controller waits for children to exit cleanly.",
                        false,
                        30000,
                    ),
                    int_field(
                        "child_poll_interval_ms",
                        "Polling cadence while watching child processes.",
                        false,
                        100,
                    ),
                ],
                notes: vec![],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "visualizer",
                kind: SchemaSectionKind::Table,
                description: "Visualizer WebSocket preview settings.",
                fields: vec![
                    int_field("port", "WebSocket port served by the visualizer.", false, 19090),
                    int_field(
                        "max_preview_width",
                        "Maximum preview frame width exposed to UIs.",
                        false,
                        320,
                    ),
                    int_field(
                        "max_preview_height",
                        "Maximum preview frame height exposed to UIs.",
                        false,
                        240,
                    ),
                    int_field("jpeg_quality", "Preview JPEG quality from 1 to 100.", false, 30),
                    int_field("preview_fps", "Maximum preview frame rate.", false, 30),
                    SchemaField {
                        name: "preview_workers",
                        type_name: "integer",
                        description: "Optional worker-thread override for preview encoding.",
                        required: false,
                        default: None,
                        enum_values: None,
                        applies_to: None,
                    },
                ],
                notes: vec![
                    "The runtime also derives camera and robot names from [[devices]].",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "devices",
                kind: SchemaSectionKind::ArrayOfTables,
                description: "Device definitions for every camera and robot participating in collection.",
                fields: vec![
                    string_field("name", "Stable logical name used throughout the config.", true),
                    enum_field(
                        "type",
                        "Logical device category.",
                        true,
                        "camera",
                        &["camera", "robot"],
                    ),
                    string_field("driver", "Driver family used to launch the device process.", true),
                    string_field("id", "Driver-specific device identifier discovered during setup.", true),
                    scoped_int_field("width", "Camera capture width.", false, None, &["camera"]),
                    scoped_int_field("height", "Camera capture height.", false, None, &["camera"]),
                    scoped_int_field("fps", "Per-device frame rate override.", false, None, &["camera"]),
                    scoped_enum_field(
                        "pixel_format",
                        "Camera pixel format.",
                        false,
                        None,
                        &["rgb24", "bgr24", "yuyv", "mjpeg", "depth16", "gray8"],
                        &["camera"],
                    ),
                    scoped_string_field(
                        "stream",
                        "Driver-specific camera stream selection such as color or depth.",
                        false,
                        &["camera"],
                    ),
                    scoped_int_field(
                        "channel",
                        "Optional camera channel index for multi-channel drivers.",
                        false,
                        None,
                        &["camera"],
                    ),
                    scoped_int_field("dof", "Robot degrees of freedom.", false, None, &["robot"]),
                    scoped_enum_field(
                        "mode",
                        "Robot control mode at startup.",
                        false,
                        None,
                        &[
                            "free-drive",
                            "command-following",
                            "identifying",
                            "disabled",
                        ],
                        &["robot"],
                    ),
                    scoped_float_field(
                        "control_frequency_hz",
                        "Robot control/state publication frequency.",
                        false,
                        None,
                        &["robot"],
                    ),
                    scoped_string_field(
                        "transport",
                        "Physical or simulated transport used by the driver.",
                        false,
                        &["camera", "robot"],
                    ),
                    scoped_string_field(
                        "interface",
                        "Optional shared bus/interface identifier such as can0.",
                        false,
                        &["robot"],
                    ),
                    scoped_string_field(
                        "product_variant",
                        "Driver-specific product/profile identifier.",
                        false,
                        &["robot"],
                    ),
                    scoped_string_field(
                        "end_effector",
                        "Optional end-effector label or subtype.",
                        false,
                        &["robot"],
                    ),
                    scoped_string_field(
                        "model_path",
                        "Optional kinematic model path for robot drivers.",
                        false,
                        &["robot"],
                    ),
                    scoped_float_array_field(
                        "gravity_comp_torque_scales",
                        "Per-joint gravity compensation scales.",
                        false,
                        &["robot"],
                    ),
                    scoped_float_array_field(
                        "mit_kp",
                        "Per-joint proportional gains for MIT-style controllers.",
                        false,
                        &["robot"],
                    ),
                    scoped_float_array_field(
                        "mit_kd",
                        "Per-joint derivative gains for MIT-style controllers.",
                        false,
                        &["robot"],
                    ),
                    scoped_int_field(
                        "command_latency_ms",
                        "Optional synthetic or expected command latency.",
                        false,
                        None,
                        &["robot"],
                    ),
                    scoped_float_field(
                        "state_noise_stddev",
                        "Optional synthetic measurement noise standard deviation.",
                        false,
                        None,
                        &["robot"],
                    ),
                ],
                notes: vec![
                    "Camera rows require width, height, fps, and pixel_format after static validation.",
                    "Robot rows require dof and mode after static validation.",
                    "Additional driver-specific keys are preserved via DeviceConfig.extra.",
                ],
                allows_extra_fields: true,
            },
            SchemaSection {
                name: "pairing",
                kind: SchemaSectionKind::ArrayOfTables,
                description: "Leader/follower relationships used by teleoperation routing.",
                fields: vec![
                    string_field("leader", "Logical name of the leader robot device.", true),
                    string_field("follower", "Logical name of the follower robot device.", true),
                    enum_field(
                        "mapping",
                        "Teleoperation mapping strategy.",
                        false,
                        "direct-joint",
                        &["direct-joint", "cartesian"],
                    ),
                    SchemaField {
                        name: "joint_index_map",
                        type_name: "integer[]",
                        description: "Leader joint indices mapped onto follower joints for direct-joint mode.",
                        required: false,
                        default: Some(Value::Array(Vec::new())),
                        enum_values: None,
                        applies_to: None,
                    },
                    SchemaField {
                        name: "joint_scales",
                        type_name: "float[]",
                        description: "Per-joint scaling factors for direct-joint mode.",
                        required: false,
                        default: Some(Value::Array(Vec::new())),
                        enum_values: None,
                        applies_to: None,
                    },
                ],
                notes: vec![
                    "Pairings must reference existing robot devices.",
                    "Cartesian mapping forbids joint_index_map and joint_scales.",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "ui",
                kind: SchemaSectionKind::Table,
                description: "Terminal and browser UI runtime settings.",
                fields: vec![
                    SchemaField {
                        name: "control_websocket_url",
                        type_name: "string",
                        description: "Optional explicit upstream URL for the control plane WebSocket (proxied at /ws/control).",
                        required: false,
                        default: None,
                        enum_values: None,
                        applies_to: None,
                    },
                    SchemaField {
                        name: "preview_websocket_url",
                        type_name: "string",
                        description: "Optional explicit upstream URL for the preview plane WebSocket (proxied at /ws/preview).",
                        required: false,
                        default: None,
                        enum_values: None,
                        applies_to: None,
                    },
                    string_field_with_default(
                        "http_host",
                        "HTTP host bound by the browser UI server.",
                        false,
                        "127.0.0.1",
                    ),
                    int_field("http_port", "HTTP port bound by the browser UI server.", false, 3000),
                    string_field_with_default("start_key", "Episode start shortcut.", false, "s"),
                    string_field_with_default("stop_key", "Episode stop shortcut.", false, "e"),
                    string_field_with_default("keep_key", "Episode keep shortcut.", false, "k"),
                    string_field_with_default(
                        "discard_key",
                        "Episode discard shortcut.",
                        false,
                        "x",
                    ),
                ],
                notes: vec![
                    "UI key bindings must be single printable characters.",
                    "The shortcuts d and r are reserved by the terminal UI.",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "encoder",
                kind: SchemaSectionKind::Table,
                description: "Video encoder defaults applied to every camera stream.",
                fields: vec![
                    enum_field(
                        "video_codec",
                        "Codec used for RGB or color camera streams.",
                        true,
                        "h264",
                        &["h264", "h265", "av1", "rvl"],
                    ),
                    enum_field(
                        "depth_codec",
                        "Codec used for depth or grayscale camera streams.",
                        false,
                        "rvl",
                        &["h264", "h265", "av1", "rvl"],
                    ),
                    enum_field(
                        "backend",
                        "Preferred encoder backend.",
                        false,
                        "auto",
                        &["auto", "cpu", "nvidia", "vaapi"],
                    ),
                    enum_field(
                        "artifact_format",
                        "Requested container or artifact format.",
                        false,
                        "auto",
                        &["auto", "mp4", "mkv", "rvl"],
                    ),
                    int_field("queue_size", "Encoder queue depth.", false, 32),
                ],
                notes: vec![
                    "Static validation rejects unsupported codec/artifact/backend combinations.",
                    "Legacy configs may still use encoder.codec as a backward-compatible alias for video_codec.",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "assembler",
                kind: SchemaSectionKind::Table,
                description: "Episode assembler buffering and handoff settings.",
                fields: vec![
                    int_field(
                        "missing_video_timeout_ms",
                        "Maximum wait before treating a missing encoded artifact as an error.",
                        false,
                        5000,
                    ),
                    string_field_with_default(
                        "staging_dir",
                        "Base staging directory used before final storage.",
                        false,
                        schema_default_staging_dir(),
                    ),
                    enum_field(
                        "encoded_handoff",
                        "How encoders hand off finalized artifacts to the assembler.",
                        false,
                        "file",
                        &["file", "iceoryx2"],
                    ),
                ],
                notes: vec![
                    "The assembler runtime derives encoder and episode staging subdirectories from staging_dir.",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "storage",
                kind: SchemaSectionKind::Table,
                description: "Episode persistence backend settings.",
                fields: vec![
                    enum_field(
                        "backend",
                        "Where finalized episodes are stored.",
                        true,
                        "local",
                        &["local", "http"],
                    ),
                    SchemaField {
                        name: "output_path",
                        type_name: "string",
                        description: "Output directory for the local backend.",
                        required: false,
                        default: Some(Value::String("./output".into())),
                        enum_values: None,
                        applies_to: None,
                    },
                    SchemaField {
                        name: "endpoint",
                        type_name: "string",
                        description: "Upload endpoint for the HTTP backend.",
                        required: false,
                        default: None,
                        enum_values: None,
                        applies_to: None,
                    },
                    int_field("queue_size", "Storage queue depth.", false, 32),
                ],
                notes: vec![
                    "Local storage requires output_path.",
                    "HTTP storage requires endpoint.",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "monitor",
                kind: SchemaSectionKind::DynamicTable,
                description: "Health-metric thresholds keyed by process id and metric name.",
                fields: vec![
                    float_field(
                        "metrics_frequency_hz",
                        "Metric publication and aggregation frequency.",
                        false,
                        1.0,
                    ),
                    SchemaField {
                        name: "thresholds",
                        type_name: "table",
                        description: "Nested process_id -> metric_name -> threshold definitions.",
                        required: false,
                        default: Some(Value::Table(Default::default())),
                        enum_values: None,
                        applies_to: None,
                    },
                ],
                notes: vec![
                    "Each threshold definition supports explanation plus lt and/or gt bounds.",
                ],
                allows_extra_fields: true,
            },
        ],
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn schema_default_staging_dir() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "/dev/shm/rollio"
    }

    #[cfg(not(target_os = "linux"))]
    {
        "./rollio"
    }
}

fn string_field(name: &'static str, description: &'static str, required: bool) -> SchemaField {
    SchemaField {
        name,
        type_name: "string",
        description,
        required,
        default: None,
        enum_values: None,
        applies_to: None,
    }
}

fn string_field_with_default(
    name: &'static str,
    description: &'static str,
    required: bool,
    default: &'static str,
) -> SchemaField {
    SchemaField {
        name,
        type_name: "string",
        description,
        required,
        default: Some(Value::String(default.into())),
        enum_values: None,
        applies_to: None,
    }
}

fn scoped_string_field(
    name: &'static str,
    description: &'static str,
    required: bool,
    applies_to: &'static [&'static str],
) -> SchemaField {
    SchemaField {
        name,
        type_name: "string",
        description,
        required,
        default: None,
        enum_values: None,
        applies_to: Some(applies_to.to_vec()),
    }
}

fn scoped_string_array_field(
    name: &'static str,
    description: &'static str,
    required: bool,
    applies_to: &'static [&'static str],
) -> SchemaField {
    SchemaField {
        name,
        type_name: "string[]",
        description,
        required,
        default: None,
        enum_values: None,
        applies_to: Some(applies_to.to_vec()),
    }
}

fn int_field(
    name: &'static str,
    description: &'static str,
    required: bool,
    default: i64,
) -> SchemaField {
    SchemaField {
        name,
        type_name: "integer",
        description,
        required,
        default: Some(Value::Integer(default)),
        enum_values: None,
        applies_to: None,
    }
}

fn scoped_int_field(
    name: &'static str,
    description: &'static str,
    required: bool,
    default: Option<i64>,
    applies_to: &'static [&'static str],
) -> SchemaField {
    SchemaField {
        name,
        type_name: "integer",
        description,
        required,
        default: default.map(Value::Integer),
        enum_values: None,
        applies_to: Some(applies_to.to_vec()),
    }
}

fn float_field(
    name: &'static str,
    description: &'static str,
    required: bool,
    default: f64,
) -> SchemaField {
    SchemaField {
        name,
        type_name: "float",
        description,
        required,
        default: Some(Value::Float(default)),
        enum_values: None,
        applies_to: None,
    }
}

fn scoped_float_field(
    name: &'static str,
    description: &'static str,
    required: bool,
    default: Option<f64>,
    applies_to: &'static [&'static str],
) -> SchemaField {
    SchemaField {
        name,
        type_name: "float",
        description,
        required,
        default: default.map(Value::Float),
        enum_values: None,
        applies_to: Some(applies_to.to_vec()),
    }
}

fn scoped_float_array_field(
    name: &'static str,
    description: &'static str,
    required: bool,
    applies_to: &'static [&'static str],
) -> SchemaField {
    SchemaField {
        name,
        type_name: "float[]",
        description,
        required,
        default: None,
        enum_values: None,
        applies_to: Some(applies_to.to_vec()),
    }
}

fn enum_field(
    name: &'static str,
    description: &'static str,
    required: bool,
    default: &'static str,
    enum_values: &'static [&'static str],
) -> SchemaField {
    SchemaField {
        name,
        type_name: "enum",
        description,
        required,
        default: Some(Value::String(default.into())),
        enum_values: Some(enum_values.to_vec()),
        applies_to: None,
    }
}

fn scoped_enum_field(
    name: &'static str,
    description: &'static str,
    required: bool,
    default: Option<&'static str>,
    enum_values: &'static [&'static str],
    applies_to: &'static [&'static str],
) -> SchemaField {
    SchemaField {
        name,
        type_name: "enum",
        description,
        required,
        default: default.map(|value| Value::String(value.into())),
        enum_values: Some(enum_values.to_vec()),
        applies_to: Some(applies_to.to_vec()),
    }
}

fn sprint_extra_a_schema() -> ConfigSchema {
    ConfigSchema {
        format: "rollio-config-schema",
        version: 2,
        sections: vec![
            SchemaSection {
                name: "root",
                kind: SchemaSectionKind::Table,
                description: "Top-level project metadata and collection mode.",
                fields: vec![
                    string_field_with_default(
                        "project_name",
                        "Logical project name embedded in saved configs and episode metadata.",
                        false,
                        "default",
                    ),
                    enum_field(
                        "mode",
                        "Collection semantics controlling whether teleoperation pairings are active.",
                        false,
                        "intervention",
                        &["teleop", "intervention"],
                    ),
                ],
                notes: vec![
                    "The new device-binary migration stores physical devices under [[devices]] with nested [[devices.channels]].",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "episode",
                kind: SchemaSectionKind::Table,
                description: "Episode-level recording and file-layout settings.",
                fields: vec![
                    enum_field(
                        "format",
                        "Episode container and directory layout.",
                        true,
                        "lerobot-v2.1",
                        &["lerobot-v2.1", "lerobot-v3.0", "mcap"],
                    ),
                    int_field("fps", "Target recording frame rate.", true, 30),
                    int_field(
                        "chunk_size",
                        "Number of frames per output chunk for chunked formats.",
                        false,
                        1000,
                    ),
                ],
                notes: vec![],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "controller",
                kind: SchemaSectionKind::Table,
                description: "Controller orchestration settings.",
                fields: vec![
                    int_field(
                        "shutdown_timeout_ms",
                        "How long the controller waits for children to exit cleanly.",
                        false,
                        30000,
                    ),
                    int_field(
                        "child_poll_interval_ms",
                        "Polling cadence while watching child processes.",
                        false,
                        100,
                    ),
                ],
                notes: vec![],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "devices",
                kind: SchemaSectionKind::ArrayOfTables,
                description: "Physical device processes launched by the controller.",
                fields: vec![
                    string_field("name", "Logical physical-device name.", true),
                    string_field("driver", "Executable driver family name.", true),
                    string_field("id", "Vendor-defined device identifier.", true),
                    string_field("bus_root", "Topic namespace root used by this device process.", true),
                ],
                notes: vec![
                    "Additional driver-specific fields may be stored on the device table via extra TOML keys.",
                ],
                allows_extra_fields: true,
            },
            SchemaSection {
                name: "devices.channels",
                kind: SchemaSectionKind::ArrayOfTables,
                description: "Enabled or disabled channels for a physical device.",
                fields: vec![
                    string_field("channel_type", "Fixed channel vocabulary item such as arm, e2, color, or depth.", true),
                    enum_field(
                        "kind",
                        "Channel kind.",
                        true,
                        "camera",
                        &["camera", "robot"],
                    ),
                    SchemaField {
                        name: "enabled",
                        type_name: "boolean",
                        description: "Whether the channel is active in this project config.",
                        required: false,
                        default: Some(Value::Boolean(true)),
                        enum_values: None,
                        applies_to: None,
                    },
                    scoped_enum_field(
                        "mode",
                        "Robot startup mode.",
                        false,
                        None,
                        &[
                            "free-drive",
                            "command-following",
                            "identifying",
                            "disabled",
                        ],
                        &["robot"],
                    ),
                    scoped_int_field("dof", "Robot degrees of freedom.", false, None, &["robot"]),
                    scoped_string_array_field(
                        "publish_states",
                        "Robot state topics published by the device process.",
                        false,
                        &["robot"],
                    ),
                    scoped_string_array_field(
                        "recorded_states",
                        "Subset of publish_states recorded by the assembler. Defaults to all publish_states.",
                        false,
                        &["robot"],
                    ),
                    scoped_float_field(
                        "control_frequency_hz",
                        "Robot publish/control rate in Hz.",
                        false,
                        None,
                        &["robot"],
                    ),
                    SchemaField {
                        name: "profile",
                        type_name: "inline-table",
                        description: "Camera profile inline table with width, height, fps, and pixel_format.",
                        required: false,
                        default: None,
                        enum_values: None,
                        applies_to: Some(vec!["camera"]),
                    },
                    SchemaField {
                        name: "command_defaults",
                        type_name: "inline-table",
                        description: "Optional command-default arrays such as joint_mit_kp, joint_mit_kd, parallel_mit_kp, and parallel_mit_kd.",
                        required: false,
                        default: None,
                        enum_values: None,
                        applies_to: None,
                    },
                ],
                notes: vec![
                    "Camera channels require profile when enabled.",
                    "Robot channels require dof, mode, and publish_states when enabled.",
                ],
                allows_extra_fields: true,
            },
            SchemaSection {
                name: "pairings",
                kind: SchemaSectionKind::ArrayOfTables,
                description: "Framework-owned teleoperation pairings between robot channels.",
                fields: vec![
                    string_field("leader_device", "Leader physical-device name.", true),
                    string_field("leader_channel_type", "Leader robot channel_type.", true),
                    string_field("follower_device", "Follower physical-device name.", true),
                    string_field("follower_channel_type", "Follower robot channel_type.", true),
                    enum_field(
                        "mapping",
                        "Teleoperation mapping strategy.",
                        false,
                        "direct-joint",
                        &["direct-joint", "cartesian"],
                    ),
                    string_field("leader_state", "Leader state topic kind, such as joint_position or end_effector_pose.", true),
                    string_field("follower_command", "Follower command topic kind, such as joint_mit or end_pose.", true),
                    SchemaField {
                        name: "joint_index_map",
                        type_name: "integer[]",
                        description: "Optional leader-to-follower joint remapping for direct-joint mode.",
                        required: false,
                        default: Some(Value::Array(Vec::new())),
                        enum_values: None,
                        applies_to: None,
                    },
                    SchemaField {
                        name: "joint_scales",
                        type_name: "float[]",
                        description: "Optional per-joint scaling for direct-joint mode.",
                        required: false,
                        default: Some(Value::Array(Vec::new())),
                        enum_values: None,
                        applies_to: None,
                    },
                ],
                notes: vec![
                    "Setup should derive legal pairings from query.direct_joint_compatibility plus channel capabilities.",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "visualizer",
                kind: SchemaSectionKind::Table,
                description: "Visualizer preview settings. Runtime sources are derived from enabled channels.",
                fields: vec![
                    int_field("port", "WebSocket port served by the visualizer.", false, 19090),
                    int_field(
                        "max_preview_width",
                        "Maximum preview frame width exposed to UIs.",
                        false,
                        320,
                    ),
                    int_field(
                        "max_preview_height",
                        "Maximum preview frame height exposed to UIs.",
                        false,
                        240,
                    ),
                    int_field("jpeg_quality", "Preview JPEG quality from 1 to 100.", false, 30),
                    int_field("preview_fps", "Maximum preview frame rate.", false, 60),
                    SchemaField {
                        name: "preview_workers",
                        type_name: "integer",
                        description: "Optional worker-thread override for preview encoding.",
                        required: false,
                        default: None,
                        enum_values: None,
                        applies_to: None,
                    },
                ],
                notes: vec![],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "encoder",
                kind: SchemaSectionKind::Table,
                description: "Encoder defaults applied to every enabled camera channel.",
                fields: vec![
                    enum_field(
                        "video_codec",
                        "Codec used for color-like camera channels.",
                        false,
                        "h264",
                        &["h264", "h265", "av1", "rvl"],
                    ),
                    enum_field(
                        "depth_codec",
                        "Codec used for depth-like camera channels.",
                        false,
                        "rvl",
                        &["h264", "h265", "av1", "rvl"],
                    ),
                ],
                notes: vec![
                    "This section keeps the existing encoder fields such as video_codec, depth_codec, backend, artifact_format, and queue_size.",
                ],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "assembler",
                kind: SchemaSectionKind::Table,
                description: "Assembler settings for staged episodes and video handoff.",
                fields: vec![],
                notes: vec![],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "storage",
                kind: SchemaSectionKind::Table,
                description: "Storage backend settings. This control-plane surface remains unchanged in the first migration phase.",
                fields: vec![],
                notes: vec![],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "monitor",
                kind: SchemaSectionKind::Table,
                description: "Metric publication and threshold settings.",
                fields: vec![],
                notes: vec![],
                allows_extra_fields: false,
            },
            SchemaSection {
                name: "ui",
                kind: SchemaSectionKind::Table,
                description: "Terminal and browser UI runtime settings.",
                fields: vec![],
                notes: vec![],
                allows_extra_fields: false,
            },
        ],
    }
}

pub fn build_config_schema() -> ConfigSchema {
    sprint_extra_a_schema()
}
