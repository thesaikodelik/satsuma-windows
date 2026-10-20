//! Windows: embeds the app icon (resource 1), version information and the manifest
//! (per-monitor DPI awareness, Common Controls 6 for the editor windows).

fn main() {
    println!("cargo:rerun-if-changed=assets/satsuma.ico");
    println!("cargo:rerun-if-changed=assets/satsuma.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon_with_id("assets/satsuma.ico", "1")
            .set_manifest_file("assets/satsuma.manifest")
            .set("ProductName", "Satsuma")
            .set("FileDescription", "Satsuma — drag-gesture file converter")
            .set("CompanyName", "Satsuma contributors")
            .set("LegalCopyright", "MIT License");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Could not embed Windows resources: {e}");
        }
    }
}
