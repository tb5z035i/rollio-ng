fn main() {
    let lib_dir = std::env::var("DATALOOP_LIB_DIR").unwrap_or_else(|_| {
        let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
        let triple = match arch.as_str() {
            "x86_64" => "x86_64-linux-gnu",
            "aarch64" => "aarch64-linux-gnu",
            a => panic!("unsupported target arch: {a}"),
        };
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        format!("{manifest_dir}/../../.sysroot/{triple}/usr/lib")
    });
    println!("cargo:rustc-link-search=native={lib_dir}");
    println!("cargo:rustc-link-lib=dylib=dataloop");
}
