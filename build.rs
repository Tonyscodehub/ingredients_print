use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=res/app.rc");
    println!("cargo:rerun-if-changed=res/app.manifest");
    println!("cargo:rerun-if-changed=res/app.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Compile the icon + manifest resource with MinGW windres and link it in.
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app_res.o");
    let windres = env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".into());
    let status = Command::new(&windres)
        .args(["--input-format=rc", "-O", "coff", "-i", "res/app.rc", "-o"])
        .arg(&out)
        .status()
        .expect("failed to run windres");
    assert!(status.success(), "windres failed");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
