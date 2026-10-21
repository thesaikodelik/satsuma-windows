//! Windows: embeds the installer icon (the app's with an install badge, resource 1), version
//! information and the manifest into satsuma-setup.exe.

#[path = "../satsuma-app/build.rs"]
#[allow(dead_code)]
mod app;

fn main() {
    println!("cargo:rerun-if-changed=../satsuma-app/assets/setup.ico");
    println!("cargo:rerun-if-changed=../satsuma-app/assets/satsuma.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("..").join("satsuma-app").join("assets");
    let dir = dir.canonicalize().unwrap_or(dir);
    let rc = app::resource_script(&dir.join("setup.ico"), &dir.join("satsuma.manifest"), "Satsuma Setup");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("setup.rc");
    std::fs::write(&out, rc).unwrap();
    if let Err(e) = embed_resource::compile_for(&out, ["satsuma-setup"], embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=Could not embed Windows resources: {e}");
    }
}
