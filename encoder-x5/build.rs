//! Build script for rollio-encoder-x5.
//! Compiles FFI shim + builds a stub libmultimedia.so for cross-link.
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let vendor_dir = manifest_dir.join("vendor");
    cc::Build::new()
        .file("src/ffi.c")
        .include(vendor_dir.join("include"))
        .warnings(true)
        .compile("x5_ffi");
    build_stub_so(&vendor_dir, &out_dir);
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=dylib=multimedia");
    for f in ["vendor/stubs.c", "src/ffi.c", "vendor/include/hb_media_codec.h",
              "vendor/include/hb_media_basic_types.h", "vendor/include/hb_media_error.h"] {
        println!("cargo:rerun-if-changed={f}");
    }
}
fn build_stub_so(vendor_dir: &Path, out_dir: &Path) {
    let stubs_c = vendor_dir.join("stubs.c");
    let obj = out_dir.join("stubs.o");
    let so = out_dir.join("libmultimedia.so");
    let target = env::var("TARGET").unwrap_or_default();
    let cc = cc_for_target(&target);
    let s = Command::new(&cc).args(["-c", "-fPIC", "-o"])
        .arg(&obj).arg(&stubs_c)
        .arg(format!("-I{}", vendor_dir.display()))
        .status().unwrap_or_else(|e| panic!("stub compile: {cc}: {e}"));
    assert!(s.success(), "stub compile failed");
    let mut link = Command::new(&cc);
    link.arg("-shared").arg("-o").arg(&so).arg(&obj)
        .arg("-Wl,-soname,libmultimedia.so.1");
    if target.contains("aarch64") {
        link.arg("--target=aarch64-linux-gnu");
    }
    let s = link.status().unwrap_or_else(|e| panic!("stub link: {cc}: {e}"));
    assert!(s.success(), "stub link failed");
}
fn cc_for_target(target: &str) -> String {
    let env_key = format!("CC_{}", target.replace('-', "_"));
    if let Ok(v) = env::var(&env_key) { return v; }
    if let Ok(v) = env::var("CC") { return v; }
    "clang".to_string()
}
