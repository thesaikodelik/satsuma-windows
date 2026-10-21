//! Windows: embeds the app icon (resource 1), version information and the manifest
//! (per-monitor DPI awareness, Common Controls 6 for the editor windows) into satsuma.exe
//! and satsuma-cli.exe only, so other programs built on this crate (Satsuma Setup) can carry
//! their own.

fn main() {
    println!("cargo:rerun-if-changed=assets/satsuma.ico");
    println!("cargo:rerun-if-changed=assets/satsuma.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets");
    let rc = resource_script(&dir.join("satsuma.ico"), &dir.join("satsuma.manifest"), "Satsuma");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("satsuma.rc");
    std::fs::write(&out, rc).unwrap();
    if let Err(e) = embed_resource::compile_for(&out, ["satsuma", "satsuma-cli"], embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=Could not embed Windows resources: {e}");
    }
}

/// A resource script with the icon, the manifest and version information.
pub fn resource_script(icon: &std::path::Path, manifest: &std::path::Path, description: &str) -> String {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut n: Vec<u16> = version.split(['.', '-']).take(3).map(|p| p.parse().unwrap_or(0)).collect();
    n.resize(4, 0);
    let numbers = format!("{},{},{},{}", n[0], n[1], n[2], n[3]);
    let path = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Satsuma contributors"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "LegalCopyright", "MIT License"
      VALUE "ProductName", "Satsuma"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = path(icon),
        manifest = path(manifest),
    )
}
