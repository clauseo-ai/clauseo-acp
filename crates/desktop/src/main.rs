//! The `clauseo-desktop` binary. Everything lives in the library (`lib.rs`).

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clauseo_desktop::{AppStages, Launched, SystemProbe, launch};

fn main() -> tauri::Result<()> {
    // First: the pre-write guard, else the agent sandbox check, before Tauri, logging or
    // settings (`jailed.rs`).
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    match launch(&args, &SystemProbe, &mut std::io::stderr(), &mut AppStages) {
        Launched::Exit(code) => std::process::exit(code),
        Launched::App(result) => result,
    }
}
