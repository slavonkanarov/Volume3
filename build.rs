fn main() {
    println!("cargo:rerun-if-changed=assets/volume3.manifest");
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("windows-msvc")) {
        let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("assets/volume3.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
