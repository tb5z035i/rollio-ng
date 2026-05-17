//! `[devices.extra]` and `[devices.channels.extra]` shapes for tactile-cora.

use rollio_types::config::{BinaryDeviceConfig, DeviceChannelConfigV2};
use thiserror::Error;

use crate::cora::{BridgeConfig, Qos};

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("device \"{device}\": [devices.extra].{key} must be {expected}")]
    DeviceExtraType {
        device: String,
        key: &'static str,
        expected: &'static str,
    },
    #[error("device \"{device}\" channel \"{channel}\": [devices.channels.extra].{key} is required")]
    MissingChannelExtra {
        device: String,
        channel: String,
        key: &'static str,
    },
    #[error("device \"{device}\" channel \"{channel}\": [devices.channels.extra].{key} must be {expected}")]
    ChannelExtraType {
        device: String,
        channel: String,
        key: &'static str,
        expected: &'static str,
    },
    #[error("device \"{device}\" channel \"{channel}\": unsupported cora_qos value \"{got}\" (expected \"reliable\" | \"best_effort\")")]
    BadQos {
        device: String,
        channel: String,
        got: String,
    },
    #[error("device \"{device}\" channel \"{channel}\": pointcloud_field_map must be a 6-element array of strings (got {got})")]
    BadFieldMap {
        device: String,
        channel: String,
        got: String,
    },
    #[error("device \"{device}\" channel \"{channel}\": tactile_point_count must be a positive integer")]
    BadPointCount {
        device: String,
        channel: String,
    },
}

#[derive(Debug, Clone)]
pub struct DeviceExtra {
    pub cora_domain_id: i32,
    pub cora_participant_name: String,
    pub cora_use_shared_memory: bool,
    pub cora_use_udp: bool,
    pub cora_callback_threads: u32,
}

impl DeviceExtra {
    pub fn parse(device: &BinaryDeviceConfig) -> Result<Self, ConfigError> {
        let extra = &device.extra;
        let cora_domain_id = match extra.get("cora_domain_id") {
            None => 0,
            Some(toml::Value::Integer(n)) => *n as i32,
            Some(_) => {
                return Err(ConfigError::DeviceExtraType {
                    device: device.name.clone(),
                    key: "cora_domain_id",
                    expected: "integer",
                })
            }
        };
        let cora_participant_name = match extra.get("cora_participant_name") {
            None => format!("rollio_{}", device.name),
            Some(toml::Value::String(s)) => s.clone(),
            Some(_) => {
                return Err(ConfigError::DeviceExtraType {
                    device: device.name.clone(),
                    key: "cora_participant_name",
                    expected: "string",
                })
            }
        };
        let cora_use_shared_memory = match extra.get("cora_use_shared_memory") {
            None => true,
            Some(toml::Value::Boolean(b)) => *b,
            Some(_) => {
                return Err(ConfigError::DeviceExtraType {
                    device: device.name.clone(),
                    key: "cora_use_shared_memory",
                    expected: "boolean",
                })
            }
        };
        let cora_use_udp = match extra.get("cora_use_udp") {
            None => true,
            Some(toml::Value::Boolean(b)) => *b,
            Some(_) => {
                return Err(ConfigError::DeviceExtraType {
                    device: device.name.clone(),
                    key: "cora_use_udp",
                    expected: "boolean",
                })
            }
        };
        let cora_callback_threads = match extra.get("cora_callback_threads") {
            None => 2,
            Some(toml::Value::Integer(n)) if *n > 0 => *n as u32,
            Some(_) => {
                return Err(ConfigError::DeviceExtraType {
                    device: device.name.clone(),
                    key: "cora_callback_threads",
                    expected: "positive integer",
                })
            }
        };
        Ok(Self {
            cora_domain_id,
            cora_participant_name,
            cora_use_shared_memory,
            cora_use_udp,
            cora_callback_threads,
        })
    }

    pub fn to_bridge_config(&self) -> BridgeConfig {
        BridgeConfig {
            domain_id: self.cora_domain_id,
            participant_name: self.cora_participant_name.clone(),
            use_shared_memory: self.cora_use_shared_memory,
            use_udp: self.cora_use_udp,
            callback_threads: self.cora_callback_threads,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TactileChannelExtra {
    pub cora_topic: String,
    pub cora_qos: Qos,
    pub tactile_point_count: u32,
    /// Six slot mappings into `[x, y, z, fx, fy, fz]`. Empty string means
    /// "leave this slot zero".
    pub pointcloud_field_map: [String; 6],
}

impl TactileChannelExtra {
    pub fn parse(
        device: &BinaryDeviceConfig,
        channel: &DeviceChannelConfigV2,
    ) -> Result<Self, ConfigError> {
        let cora_topic = match channel.extra.get("cora_topic") {
            Some(toml::Value::String(s)) if !s.is_empty() => s.clone(),
            Some(toml::Value::String(_)) => {
                return Err(ConfigError::ChannelExtraType {
                    device: device.name.clone(),
                    channel: channel.channel_type.clone(),
                    key: "cora_topic",
                    expected: "non-empty string",
                })
            }
            Some(_) => {
                return Err(ConfigError::ChannelExtraType {
                    device: device.name.clone(),
                    channel: channel.channel_type.clone(),
                    key: "cora_topic",
                    expected: "string",
                })
            }
            None => {
                return Err(ConfigError::MissingChannelExtra {
                    device: device.name.clone(),
                    channel: channel.channel_type.clone(),
                    key: "cora_topic",
                })
            }
        };
        let cora_qos = match channel.extra.get("cora_qos") {
            None => Qos::Reliable,
            Some(toml::Value::String(s)) => match s.as_str() {
                "reliable" => Qos::Reliable,
                "best_effort" => Qos::BestEffort,
                other => {
                    return Err(ConfigError::BadQos {
                        device: device.name.clone(),
                        channel: channel.channel_type.clone(),
                        got: other.to_string(),
                    })
                }
            },
            Some(_) => {
                return Err(ConfigError::ChannelExtraType {
                    device: device.name.clone(),
                    channel: channel.channel_type.clone(),
                    key: "cora_qos",
                    expected: "string",
                })
            }
        };
        let tactile_point_count = match channel.extra.get("tactile_point_count") {
            Some(toml::Value::Integer(n)) if *n > 0 && (*n as u64) <= u32::MAX as u64 => *n as u32,
            _ => {
                return Err(ConfigError::BadPointCount {
                    device: device.name.clone(),
                    channel: channel.channel_type.clone(),
                })
            }
        };
        let pointcloud_field_map = match channel.extra.get("pointcloud_field_map") {
            Some(toml::Value::Array(arr)) if arr.len() == 6 => {
                let mut out: [String; 6] = Default::default();
                for (i, v) in arr.iter().enumerate() {
                    match v {
                        toml::Value::String(s) => out[i] = s.clone(),
                        _ => {
                            return Err(ConfigError::BadFieldMap {
                                device: device.name.clone(),
                                channel: channel.channel_type.clone(),
                                got: format!("{:?}", arr),
                            })
                        }
                    }
                }
                out
            }
            Some(v) => {
                return Err(ConfigError::BadFieldMap {
                    device: device.name.clone(),
                    channel: channel.channel_type.clone(),
                    got: format!("{:?}", v),
                })
            }
            None => {
                return Err(ConfigError::MissingChannelExtra {
                    device: device.name.clone(),
                    channel: channel.channel_type.clone(),
                    key: "pointcloud_field_map",
                })
            }
        };
        Ok(Self {
            cora_topic,
            cora_qos,
            tactile_point_count,
            pointcloud_field_map,
        })
    }
}
