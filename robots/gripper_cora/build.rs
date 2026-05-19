//! Build script for `gripper-cora`. Mirrors `sensors/imu_cora/build.rs`:
//! discovers the Cora SDK via `CORA_SDK_ROOT` (or arch-specific overrides, or
//! system install), drives cmake on `cpp/`, bindgen on `cpp/include/cora_bridge.h`,
//! emits link directives + RPATH. On non-Linux hosts the cmake/link steps are
//! skipped so `cargo check` works.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let shim_dir = manifest_dir.join("cpp");
    let header_path = shim_dir.join("include").join("cora_bridge.h");

    println!("cargo:rerun-if-env-changed=CORA_SDK_ROOT");
    println!("cargo:rerun-if-env-changed=CORA_SDK_X86_64_ROOT");
    println!("cargo:rerun-if-env-changed=CORA_SDK_AARCH64_ROOT");
    println!("cargo:rerun-if-env-changed=CORA_SDK_RUNTIME_RPATH");
    println!("cargo:rerun-if-changed={}", shim_dir.display());

    generate_bindings(&header_path);

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "linux" {
        println!(
            "cargo:warning=gripper-cora: target_os={} is not 'linux'; \
             skipping C++ shim build and link directives. Cargo check works, but a \
             functional binary requires Linux + the Cora SDK.",
            target_os
        );
        return;
    }

    let sdk_root = resolve_sdk_root();

    // No SDK available (no env var, no arch-matched prebuild, no system
    // install). Skip the cmake + link directives so `cargo check` on a
    // dev host without the SDK still succeeds; a binary build will fail
    // at link time, which is the same behavior as the original
    // env-var-only flow.
    if sdk_root.is_none() && !system_cora_available() {
        println!(
            "cargo:warning=gripper-cora: Cora SDK not found via CORA_SDK_ROOT, \
             arch-specific override, prebuild/, or system install; \
             skipping C++ shim build. Set CORA_SDK_ROOT or place an \
             extracted SDK under prebuild/cora-sdk_*_linux_<arch>/ for a \
             functional binary."
        );
        return;
    }

    let mut cfg = cmake::Config::new(&shim_dir);
    cfg.profile("Release");
    if let Some(root) = sdk_root.as_ref() {
        cfg.define("CMAKE_PREFIX_PATH", root.join("lib/cmake/cora"));
    }
    let install_dir = cfg.build();

    println!(
        "cargo:rustc-link-search=native={}",
        install_dir.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=cora_bridge_shim");
    println!("cargo:rustc-link-lib=dylib=stdc++");

    if let Some(root) = sdk_root.as_ref() {
        println!(
            "cargo:rustc-link-search=native={}",
            root.join("lib").display()
        );
    }
    println!("cargo:rustc-link-lib=dylib=cora_framework");
    println!("cargo:rustc-link-lib=dylib=fastrtps");
    println!("cargo:rustc-link-lib=dylib=fastcdr");

    let runtime_rpath = env::var("CORA_SDK_RUNTIME_RPATH").ok().or_else(|| {
        sdk_root
            .as_ref()
            .map(|r| r.join("lib").display().to_string())
    });
    if let Some(rp) = runtime_rpath {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", rp);
    }
}

fn generate_bindings(header_path: &Path) {
    let bindings = bindgen::Builder::default()
        .header(header_path.display().to_string())
        .allowlist_function("cora_bridge_.*")
        .allowlist_type("cora_.*_t")
        .allowlist_var("CORA_BRIDGE_.*")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("bindgen failed for cora_bridge.h");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("cora_bridge_sys.rs");
    bindings
        .write_to_file(&out_path)
        .expect("failed to write bindgen output");
}

fn resolve_sdk_root() -> Option<PathBuf> {
    if let Some(p) = read_env_path("CORA_SDK_ROOT") {
        return Some(p);
    }
    let target = env::var("TARGET").unwrap_or_default();
    let arch_var = if target.starts_with("x86_64") {
        "CORA_SDK_X86_64_ROOT"
    } else if target.starts_with("aarch64") {
        "CORA_SDK_AARCH64_ROOT"
    } else {
        return None;
    };
    if let Some(p) = read_env_path(arch_var) {
        return Some(p);
    }
    resolve_prebuild_sdk_root(&target)
}

/// CMake's default search path includes `/opt/cora` and `/usr/lib/cmake/`.
/// Probe a couple of well-known locations so the no-SDK fast-path can
/// distinguish "operator built without the SDK" from "SDK lives in a
/// system prefix CMake will find on its own".
fn system_cora_available() -> bool {
    [
        "/opt/cora/lib/cmake/cora/coraConfig.cmake",
        "/usr/lib/cmake/cora/coraConfig.cmake",
        "/usr/local/lib/cmake/cora/coraConfig.cmake",
    ]
    .iter()
    .any(|p| Path::new(p).exists())
}

/// Look for an extracted Cora SDK under `<workspace>/prebuild/`. Matches
/// directory names ending in `_linux_<arch>` (e.g.
/// `cora-sdk_1.2.0_20260517124657_linux_aarch64`) and expects the SDK tree
/// rooted at `<entry>/opt/cora` with a `lib/cmake/cora/coraConfig.cmake`.
fn resolve_prebuild_sdk_root(target: &str) -> Option<PathBuf> {
    let arch = if target.starts_with("x86_64") {
        "linux_x86_64"
    } else if target.starts_with("aarch64") {
        "linux_aarch64"
    } else {
        return None;
    };
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?);
    let workspace_root = manifest_dir.parent()?.parent()?;
    let prebuild = workspace_root.join("prebuild");
    println!("cargo:rerun-if-changed={}", prebuild.display());
    let entry = std::fs::read_dir(&prebuild)
        .ok()?
        .filter_map(Result::ok)
        .find(|e| {
            e.path().is_dir()
                && e.file_name()
                    .to_string_lossy()
                    .ends_with(&format!("_{arch}"))
        })?;
    let candidate = entry.path().join("opt/cora");
    let config = candidate.join("lib/cmake/cora/coraConfig.cmake");
    if config.exists() {
        Some(candidate)
    } else {
        None
    }
}

fn read_env_path(name: &str) -> Option<PathBuf> {
    let raw = env::var(name).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    let config = path.join("lib/cmake/cora/coraConfig.cmake");
    if !config.exists() {
        println!(
            "cargo:warning={}={} does not contain lib/cmake/cora/coraConfig.cmake — \
             falling back to default CMake search path",
            name,
            path.display()
        );
        return None;
    }
    Some(path)
}
