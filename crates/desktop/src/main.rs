//! The `clauseo-desktop` binary. Everything lives in the library (`lib.rs`).

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> tauri::Result<()> {
    clauseo_desktop::run()
}

// scratch for #51: app-change proof, never merged
