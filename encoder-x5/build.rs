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

    // Use the cc crate's compiler resolution — it handles all the
    // CC_<target> env-var variants (hyphenated, underscored, etc.)
    // and picks the correct cross-compiler automatically.
    let compiler = cc::Build::new()
        .file(&stubs_c)
        .get_compiler();
    let cc_path = compiler.path();

    // Compile stubs.o — forward cc-crate's args (includes --target, etc.)
    let mut compile = Command::new(cc_path);
    compile.args(["-c", "-fPIC", "-o"]).arg(&obj).arg(&stubs_c)
        .arg(format!("-I{}", vendor_dir.display()));
    for arg in compiler.args() {
        compile.arg(arg);
    }
    let s = compile.status().unwrap_or_else(|e| {
        panic!("stub compile: {}: {e}", cc_path.display())
    });
    assert!(s.success(), "stub compile failed");

    // Link into shared library.
    let mut link = Command::new(cc_path);
    link.arg("-shared").arg("-o").arg(&so).arg(&obj)
        .arg("-Wl,-soname,libmultimedia.so.1");
    for arg in compiler.args() {
        link.arg(arg);
    }
    link.arg("-nodefaultlibs");
    let s = link.status().unwrap_or_else(|e| {
        panic!("stub link: {}: {e}", cc_path.display())
    });
    assert!(s.success(), "stub link failed");
}
