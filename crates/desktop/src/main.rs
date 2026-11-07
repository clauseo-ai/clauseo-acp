//! The `clauseo-desktop` binary. Everything lives in the library (`lib.rs`).

// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clauseo_desktop::{AppStages, Launched, SystemProbe, launch};

fn main() -> tauri::Result<()> {
    // Before anything else, while the process has one thread: a set
    // WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS keeps the rule that stops the webview's DNS lookups
    // (`navigation/browser_args.rs`).
    #[cfg(windows)]
    clauseo_desktop::keep_no_dns_in_env();
    // First: the pre-write guard, else the agent sandbox check, before Tauri, logging or
    // settings (`jailed.rs`).
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    match launch(&args, &SystemProbe, &mut std::io::stderr(), &mut AppStages) {
        Launched::Exit(code) => std::process::exit(code),
        Launched::App(result) => result,
    }
}
