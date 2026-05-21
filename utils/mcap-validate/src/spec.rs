//! Spec TOML model. Each spec is self-contained — no inheritance / merging.
//! Mirrors the format documented in `mcap_spec/data_spec_template.toml`.

use std::path::Path;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Spec {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub metadata: IndexMap<String, MetadataRule>,
    #[serde(default)]
    pub channels: Vec<ChannelRule>,
    #[serde(default)]
    pub constraints: Constraints,
}

#[derive(Debug, Deserialize)]
pub struct MetadataRule {
    #[serde(rename = "enum", default)]
    pub allowed: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChannelRule {
    pub name: String,
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Constraints {
    #[serde(default)]
    pub sync_group: Vec<SyncGroup>,
    #[serde(default)]
    pub tf_pair: Vec<TfPair>,
}

#[derive(Debug, Deserialize)]
pub struct SyncGroup {
    pub channels: Vec<String>,
    #[serde(default)]
    pub max_time_diff_ms: f64,
}

#[derive(Debug, Deserialize)]
pub struct TfPair {
    pub channel: String,
    pub parent: String,
    pub child: String,
}

impl Spec {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading spec file {}", path.display()))?;
        let text = std::str::from_utf8(&bytes)
            .with_context(|| format!("spec file {} is not valid UTF-8", path.display()))?;
        toml::from_str(text)
            .with_context(|| format!("parsing spec TOML {}", path.display()))
    }

    pub fn name_or_default(&self) -> &str {
        self.name.as_deref().unwrap_or("<unnamed>")
    }
}
