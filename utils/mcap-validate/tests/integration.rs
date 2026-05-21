//! End-to-end smoke tests against the sample spec/mcap shipped in the repo.
//!
//! These run against `mcap_spec/recording_converted.mcap` if present.
//! Set `MCAP_VALIDATE_FIXTURE_MCAP` / `MCAP_VALIDATE_FIXTURE_SPEC` in CI to
//! point at custom fixtures.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mcap_validate::spec::Spec;
use mcap_validate::validate_one;

fn repo_root() -> PathBuf {
    // Cargo runs tests from the crate dir: validator-rs/. The repo's mcap_spec/
    // is one level up from that.
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_dir.parent().unwrap().to_path_buf()
}

fn fixture_mcap() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MCAP_VALIDATE_FIXTURE_MCAP") {
        return Some(PathBuf::from(p));
    }
    let p = repo_root().join("recording_converted.mcap");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

fn fixture_spec() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MCAP_VALIDATE_FIXTURE_SPEC") {
        return Some(PathBuf::from(p));
    }
    let p = repo_root()
        .join("collect_verify")
        .join("data_spec_door.toml");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

#[test]
fn smoke_loads_spec() {
    let Some(spec_path) = fixture_spec() else {
        eprintln!("skipping: no fixture spec");
        return;
    };
    let spec = Spec::load(&spec_path).expect("spec loads");
    assert!(
        !spec.metadata.is_empty(),
        "spec should declare metadata rules"
    );
    assert!(!spec.channels.is_empty(), "spec should declare channels");
}

#[test]
fn smoke_validates_against_real_mcap() {
    let (Some(spec_path), Some(mcap_path)) = (fixture_spec(), fixture_mcap()) else {
        eprintln!("skipping: missing fixture mcap and/or spec");
        return;
    };
    let spec = Arc::new(Spec::load(&spec_path).expect("spec loads"));
    let r = validate_one(Path::new(&mcap_path), &spec, false);
    // We don't know whether the sample passes or fails; just assert the
    // pipeline ran without panicking and surfaced *something* coherent.
    assert!(r.error.is_none() || !r.error.as_ref().unwrap().is_empty());
}
