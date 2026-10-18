//! Declares the `AppManifest`. With it, Tauri checks app commands against the capabilities too,
//! and the command list starts empty: a new command is denied until a capability grants it.

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[])),
    )?;
    Ok(())
}
