//! The `clauseo-desktop` binary. Everything lives in the library (`lib.rs`).

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> tauri::Result<()> {
    // The pre-write guard hook, when the `clauseo-guard` sidecar is missing (or a hook written
    // before it shipped names this exe): before Tauri, logging or settings.
    if let Some(code) = clauseo_host::pre_write_guard::main_entry(std::env::args_os().skip(1)) {
        std::process::exit(code);
    }
    clauseo_desktop::run()
}
