//! Declares the `AppManifest`. With it, Tauri checks app commands against the capabilities too,
//! and the command list starts empty: a new command is denied until a capability grants it.
//!
//! On Windows (MSVC), the linker embeds `windows-app.manifest` (Common Controls 6, which
//! tauri-plugin-dialog's `TaskDialogIndirect` needs) into every binary this crate links, the lib's
//! unit-test binary included. tauri-build would embed its manifest into the app only
//! (`rustc-link-arg-bins`), and the test binary then fails to start with 0xc0000139
//! (tauri-apps/tauri#13419); `rustc-link-arg-tests` does not reach lib unit tests.

use std::io::Write as _;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut windows = tauri_build::WindowsAttributes::new();
    if std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc") {
        windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        let dir = std::env::var("CARGO_MANIFEST_DIR")?;
        let manifest = std::path::Path::new(&dir).join("windows-app.manifest");
        let mut out = std::io::stdout().lock();
        writeln!(out, "cargo:rerun-if-changed={}", manifest.display())?;
        writeln!(out, "cargo:rustc-link-arg=/MANIFEST:EMBED")?;
        writeln!(
            out,
            "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
            manifest.display()
        )?;
    }
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(windows)
            .app_manifest(tauri_build::AppManifest::new().commands(&[])),
    )?;
    Ok(())
}
