//! Single-pass MCAP scan that produces everything the validators need:
//! channels (from summary), metadata (from metadata records), per-channel
//! sequence→log_time buckets for sync_group checks, and per-channel observed
//! (parent, child) pairs for tf_pair checks.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use memmap2::Mmap;

use crate::fbs;
use crate::spec::Spec;

#[derive(Debug, Default)]
pub struct ChannelInfo {
    pub schema: Option<String>,
    pub encoding: String,
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub channels: HashMap<String, ChannelInfo>,
    pub metadata: HashMap<String, String>,
    /// per-channel { sequence -> log_time(ns) }
    pub sync_buckets: HashMap<String, BTreeMap<u32, u64>>,
    /// per-channel set of observed (parent_frame_id, child_frame_id)
    pub tf_observed: HashMap<String, HashSet<(String, String)>>,
}

/// Open + scan an MCAP file. `skip_constraints=true` short-circuits the
/// (expensive) message stream pass and returns only summary-derived data.
pub fn scan(path: &Path, spec: &Spec, skip_constraints: bool) -> Result<ScanResult> {
    let file = File::open(path)
        .with_context(|| format!("opening mcap file {}", path.display()))?;
    let mmap = unsafe { Mmap::map(&file) }
        .with_context(|| format!("mmap-ing mcap file {}", path.display()))?;

    let summary = mcap::Summary::read(&mmap)
        .with_context(|| format!("reading mcap summary of {}", path.display()))?
        .ok_or_else(|| anyhow!("MCAP file {} has no summary; cannot validate", path.display()))?;

    let mut result = ScanResult::default();

    // ---- channels (from summary) ----
    for ch in summary.channels.values() {
        let schema_name = ch.schema.as_ref().map(|s| s.name.clone());
        result.channels.insert(
            ch.topic.clone(),
            ChannelInfo {
                schema: schema_name,
                encoding: ch.message_encoding.clone(),
            },
        );
    }

    // ---- metadata records ----
    for idx in &summary.metadata_indexes {
        let meta = mcap::read::metadata(&mmap, idx)
            .with_context(|| format!("reading metadata record {:?}", idx.name))?;
        for (k, v) in meta.metadata {
            result.metadata.insert(k, v);
        }
    }

    if skip_constraints {
        return Ok(result);
    }

    // ---- which topics do we actually need messages from? ----
    let mut sync_topics: HashSet<String> = HashSet::new();
    for sg in &spec.constraints.sync_group {
        for c in &sg.channels {
            if result.channels.contains_key(c) {
                sync_topics.insert(c.clone());
            }
        }
    }
    let mut tf_topics: HashSet<String> = HashSet::new();
    for tp in &spec.constraints.tf_pair {
        if result.channels.contains_key(&tp.channel) {
            tf_topics.insert(tp.channel.clone());
        }
    }

    if sync_topics.is_empty() && tf_topics.is_empty() {
        return Ok(result);
    }

    // ---- single pass over messages ----
    for msg in mcap::MessageStream::new(&mmap)
        .with_context(|| format!("opening message stream for {}", path.display()))?
    {
        let m = msg.with_context(|| format!("decoding message in {}", path.display()))?;
        let topic = m.channel.topic.as_str();

        if sync_topics.contains(topic) {
            result
                .sync_buckets
                .entry(topic.to_string())
                .or_default()
                .insert(m.sequence, m.log_time);
        }
        if tf_topics.contains(topic) {
            if let Some(pair) = fbs::read_frame_transform_pair(&m.data) {
                result
                    .tf_observed
                    .entry(topic.to_string())
                    .or_default()
                    .insert(pair);
            }
        }
    }

    Ok(result)
}
