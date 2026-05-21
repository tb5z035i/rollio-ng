//! Library entry points used both by the `mcap-validate` binary and by
//! integration tests.
//!
//! `fbs_foxglove` and `fbs_discover` are merged blobs of all flatc-generated
//! types per namespace. They are produced by `tools/merge_fbs.py` from the
//! per-file flatc output (which lived in `src/fbs/*_generated.rs`). Merging
//! is required because the per-file output uses `use crate::X_generated::*;`
//! glob imports which collide on the shared `pub mod <namespace>` block when
//! the files are loaded as separate top-level modules.

mod fbs_discover;
mod fbs_foxglove;

pub mod batch;
pub mod fbs;
pub mod mcap_scan;
pub mod report;
pub mod spec;
pub mod validate;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;

use crate::spec::Spec;

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileReport {
    pub path: PathBuf,
    pub spec: String,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub issues: Vec<String>,
    /// `Some(msg)` if the file could not be opened/parsed at all (distinct
    /// from per-rule validation issues).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Validate a single MCAP file. Errors during scan (file open / summary read /
/// fbs decode) are captured into `FileReport.error`; the function itself only
/// returns `Err` for truly unexpected failures.
pub fn validate_one(path: &Path, spec: &Spec, skip_constraints: bool) -> FileReport {
    let start = Instant::now();
    let spec_name = spec.name_or_default().to_string();

    match mcap_scan::scan(path, spec, skip_constraints) {
        Ok(scan) => {
            let mut issues = Vec::new();
            issues.extend(validate::validate_metadata(spec, &scan));
            issues.extend(validate::validate_channels(spec, &scan));
            if !skip_constraints {
                issues.extend(validate::validate_sync_groups(spec, &scan));
                issues.extend(validate::validate_tf_pairs(spec, &scan));
            }
            FileReport {
                path: path.to_path_buf(),
                spec: spec_name,
                ok: issues.is_empty(),
                elapsed_ms: start.elapsed().as_millis() as u64,
                issues,
                error: None,
            }
        }
        Err(e) => FileReport {
            path: path.to_path_buf(),
            spec: spec_name,
            ok: false,
            elapsed_ms: start.elapsed().as_millis() as u64,
            issues: Vec::new(),
            error: Some(format!("{:#}", e)),
        },
    }
}

/// Convenience wrapper used by tests / single-file CLI mode.
pub fn validate_path(path: &Path, spec: Arc<Spec>, skip_constraints: bool) -> Result<FileReport> {
    Ok(validate_one(path, &spec, skip_constraints))
}
