//! Build script for `imu-cora`.
//!
//! Builds the C++ shim under `cpp/` (which links the Cora SDK via
//! `find_package(cora REQUIRED)`), generates FFI bindings from `cpp/include/cora_bridge.h`,
//! and emits link directives so the resulting binary can find the SDK libraries at runtime.
//!
//! SDK discovery is by environment variable only — `examples/cora_sdk/` in the repo is
//! NOT looked at automatically. Discovery order:
//!   1. `CORA_SDK_ROOT`
//!   2. `CORA_SDK_X86_64_ROOT` / `CORA_SDK_AARCH64_ROOT` per Cargo target arch
//!   3. CMake's default `find_package(cora)` search path (system install)
//!
//! On non-Linux hosts (e.g. macOS dev machines) the cmake + link steps are skipped so
//! `cargo check` still works; a real binary build requires Linux + the SDK.

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
            "cargo:warning=imu-cora: target_os={} is not 'linux'; \
             skipping C++ shim build and link directives. Cargo check works, but a \
             functional binary requires Linux + the Cora SDK.",
            target_os
        );
        return;
    }

    let sdk_root = resolve_sdk_root();

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

    let runtime_rpath = env::var("CORA_SDK_RUNTIME_RPATH")
        .ok()
        .or_else(|| sdk_root.as_ref().map(|r| r.join("lib").display().to_string()));
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
    read_env_path(arch_var)
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
