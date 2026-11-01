//! The `clauseo-desktop` binary. Everything lives in the library (`lib.rs`).

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> tauri::Result<()> {
    // The pre-write guard hook (`pre_write_guard.rs`): before Tauri, logging or settings.
    if clauseo_desktop::guard_requested(std::env::args_os().nth(1).as_deref()) {
        let code = clauseo_host::pre_write_guard::run(std::io::stdin().lock(), std::io::stdout());
        std::process::exit(code);
    }
    clauseo_desktop::run()
}
