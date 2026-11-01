use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=dummy/dummy.rs");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let target = env::var("TARGET").expect("TARGET");
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let dest = out.join("dummy.bin");

    let status = Command::new(rustc)
        .args(["--edition", "2024", "--crate-type", "bin", "--crate-name", "dummy"])
        .args(["--target", &target])
        .args(["-C", "opt-level=z", "-C", "panic=abort"])
        .args(["-C", "codegen-units=1", "-C", "strip=symbols"])
        .arg("-o")
        .arg(&dest)
        .arg("dummy/dummy.rs")
        .status()
        .expect("failed to run rustc for dummy");

    if !status.success() {
        panic!("building dummy/dummy.rs failed");
    }
}
