//! Batch / parallel validation. The input path is either a single .mcap file
//! or a directory which we walk recursively for `*.mcap` files. All matched
//! files are validated against the same spec via a rayon thread pool.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use rayon::prelude::*;
use walkdir::WalkDir;

use crate::spec::Spec;
use crate::{validate_one, FileReport};

pub fn collect_mcaps(root: &Path) -> Result<Vec<PathBuf>> {
    if root.is_file() {
        return Ok(vec![root.to_path_buf()]);
    }
    if !root.is_dir() {
        bail!("path {} is neither a file nor a directory", root.display());
    }
    let mut out = Vec::new();
    for entry in WalkDir::new(root).follow_links(true) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) == Some("mcap") {
            out.push(entry.into_path());
        }
    }
    out.sort();
    Ok(out)
}

pub struct BatchOptions {
    pub jobs: usize,
    pub skip_constraints: bool,
    pub fail_fast: bool,
}

pub fn run_batch(files: Vec<PathBuf>, spec: Arc<Spec>, opts: BatchOptions) -> Result<Vec<FileReport>> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.jobs)
        .build()?;

    let stop = Arc::new(AtomicBool::new(false));
    let reports: Vec<FileReport> = pool.install(|| {
        files
            .par_iter()
            .map(|p| {
                if opts.fail_fast && stop.load(Ordering::Relaxed) {
                    return FileReport {
                        path: p.clone(),
                        spec: spec.name_or_default().to_string(),
                        ok: false,
                        elapsed_ms: 0,
                        issues: Vec::new(),
                        error: Some("skipped (fail-fast)".into()),
                    };
                }
                let r = validate_one(p, &spec, opts.skip_constraints);
                if opts.fail_fast && !r.ok {
                    stop.store(true, Ordering::Relaxed);
                }
                r
            })
            .collect()
    });

    Ok(reports)
}
