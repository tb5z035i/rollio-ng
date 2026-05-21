//! 1:1 port of the four validator functions in `mcap_spec/validation.py`.
//! Error message wording is kept identical so existing log-grep tooling and
//! the diff-based equivalence test against the Python version keep passing.

use std::collections::HashSet;

use crate::mcap_scan::ScanResult;
use crate::spec::Spec;

pub fn validate_metadata(spec: &Spec, scan: &ScanResult) -> Vec<String> {
    let mut errors = Vec::new();
    for (key, rule) in &spec.metadata {
        let Some(actual) = scan.metadata.get(key) else {
            errors.push(format!("missing required metadata: {}", pyrepr(key)));
            continue;
        };
        if let Some(allowed) = &rule.allowed {
            if !allowed.iter().any(|v| v == actual) {
                errors.push(format!(
                    "metadata {}={} not in enum {}",
                    pyrepr(key),
                    pyrepr(actual),
                    pyrepr_list(allowed),
                ));
            }
        }
    }
    errors
}

pub fn validate_channels(spec: &Spec, scan: &ScanResult) -> Vec<String> {
    let mut errors = Vec::new();
    for ch in &spec.channels {
        let name = ch.name.as_str();
        let Some(actual) = scan.channels.get(name) else {
            errors.push(format!("missing required channel: {}", pyrepr(name)));
            continue;
        };
        if let Some(want) = &ch.schema {
            let got = actual.schema.as_deref().unwrap_or("");
            if want != got {
                errors.push(format!(
                    "channel {} schema mismatch: spec={}, actual={}",
                    pyrepr(name),
                    pyrepr(want),
                    pyrepr(got),
                ));
            }
        }
        if let Some(want) = &ch.encoding {
            if want != &actual.encoding {
                errors.push(format!(
                    "channel {} encoding mismatch: spec={}, actual={}",
                    pyrepr(name),
                    pyrepr(want),
                    pyrepr(&actual.encoding),
                ));
            }
        }
    }
    errors
}

pub fn validate_sync_groups(spec: &Spec, scan: &ScanResult) -> Vec<String> {
    let mut errors = Vec::new();
    if spec.constraints.sync_group.is_empty() {
        return errors;
    }

    for sg in &spec.constraints.sync_group {
        let channels = &sg.channels;
        let max_ms = sg.max_time_diff_ms;
        let max_ns = (max_ms * 1_000_000.0) as u64;

        let missing: Vec<&String> = channels
            .iter()
            .filter(|c| !scan.channels.contains_key(c.as_str()))
            .collect();
        if !missing.is_empty() {
            errors.push(format!(
                "sync_group {} skipped: missing channels {}",
                fmt_channel_list(channels),
                fmt_channel_list_refs(&missing)
            ));
            continue;
        }

        // intersection of sequence ids across all channels in the group
        let mut iter = channels.iter();
        let Some(first) = iter.next() else { continue };
        let Some(first_buckets) = scan.sync_buckets.get(first) else {
            errors.push(format!(
                "sync_group {}: no shared sequence id across channels",
                fmt_channel_list(channels)
            ));
            continue;
        };
        let mut common: HashSet<u32> = first_buckets.keys().copied().collect();
        for c in iter {
            let Some(b) = scan.sync_buckets.get(c) else {
                common.clear();
                break;
            };
            common.retain(|s| b.contains_key(s));
        }
        if common.is_empty() {
            errors.push(format!(
                "sync_group {}: no shared sequence id across channels",
                fmt_channel_list(channels)
            ));
            continue;
        }

        let mut worst_diff: u64 = 0;
        let mut worst_seq: Option<u32> = None;
        for s in &common {
            let mut min_t = u64::MAX;
            let mut max_t = u64::MIN;
            for c in channels {
                let t = scan.sync_buckets[c][s];
                if t < min_t {
                    min_t = t;
                }
                if t > max_t {
                    max_t = t;
                }
            }
            let diff = max_t - min_t;
            if diff > worst_diff {
                worst_diff = diff;
                worst_seq = Some(*s);
            }
        }

        if worst_diff > max_ns {
            errors.push(format!(
                "sync_group {} timing exceeded: worst {:.3}ms > {}ms (seq={})",
                fmt_channel_list(channels),
                worst_diff as f64 / 1e6,
                fmt_max_ms(max_ms),
                worst_seq
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "None".into()),
            ));
        }
    }

    errors
}

pub fn validate_tf_pairs(spec: &Spec, scan: &ScanResult) -> Vec<String> {
    let mut errors = Vec::new();
    if spec.constraints.tf_pair.is_empty() {
        return errors;
    }

    // group required pairs by channel
    let mut by_channel: std::collections::HashMap<&str, Vec<&crate::spec::TfPair>> =
        std::collections::HashMap::new();
    for p in &spec.constraints.tf_pair {
        by_channel.entry(p.channel.as_str()).or_default().push(p);
    }

    for (ch, ps) in by_channel {
        if !scan.channels.contains_key(ch) {
            errors.push(format!(
                "tf_pair on {} unverifiable: channel missing in mcap",
                pyrepr(ch)
            ));
            continue;
        }
        let empty = HashSet::new();
        let observed = scan.tf_observed.get(ch).unwrap_or(&empty);
        for p in ps {
            let key = (p.parent.clone(), p.child.clone());
            if !observed.contains(&key) {
                errors.push(format!(
                    "tf_pair missing on {}: ({} -> {})",
                    pyrepr(ch),
                    pyrepr(&p.parent),
                    pyrepr(&p.child),
                ));
            }
        }
    }

    errors
}

// =========================
// Formatting helpers — keep error wording byte-identical to validation.py so
// the diff-based equivalence test stays clean. Python uses repr(): single
// quotes for ASCII strings, list literals like `['a', 'b']`, and integer-valued
// floats render without trailing `.0` (`5` not `5.0`).
// =========================

/// Approximate Python's `repr()` for strings. Sufficient for the channel /
/// frame / metadata key strings we emit: ASCII, no embedded quotes/backslashes
/// in practice. If a single quote *is* present we fall back to double quotes
/// like Python does.
fn pyrepr(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{}\"", s)
    } else {
        let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
        format!("'{}'", escaped)
    }
}

fn pyrepr_list<S: AsRef<str>>(v: &[S]) -> String {
    let inner: Vec<String> = v.iter().map(|s| pyrepr(s.as_ref())).collect();
    format!("[{}]", inner.join(", "))
}

fn fmt_channel_list(v: &[String]) -> String {
    pyrepr_list(v)
}
fn fmt_channel_list_refs(v: &[&String]) -> String {
    let owned: Vec<&str> = v.iter().map(|s| s.as_str()).collect();
    pyrepr_list(&owned)
}

fn fmt_max_ms(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}
