//! The `clauseo-guard` sidecar: the program an agent's `PreToolUse` hook runs before each file
//! edit and shell command (`clauseo_host::pre_write_guard`, whose `guard_program` picks it).
//!
//! Contract:
//! - `clauseo-guard --pre-write-guard [--matter <folder>]` runs
//!   `clauseo_host::pre_write_guard::main_entry`, the same function the desktop binary's guard
//!   mode runs: one hook input object on stdin, a refusal's reason or an ignored line on stderr,
//!   exit 0 or 2 (`DENY`), nothing on stdout. `tests/decisions.rs` holds the two to the same
//!   bytes and codes.
//! - Any other first argument prints [`USAGE`] on stderr and exits [`USAGE_CODE`], never 2: a
//!   hook set up wrong must not refuse every tool.
//! - It depends on `clauseo-host` alone and links no window toolkit, so it starts in about a
//!   millisecond where the desktop binary loads `WebKitGTK` or `WebView2` first. It goes beside
//!   the app's executable: a workspace build leaves it in `target/<profile>/`, and a release
//!   ships it as Tauri's `externalBin`.

// Like the app: no console window on Windows when an agent starts it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write as _;

/// The line printed for a call that is no guard call.
const USAGE: &str = "usage: clauseo-guard --pre-write-guard [--matter <folder>]\n";

/// The exit code with [`USAGE`]: BSD's `EX_USAGE`. An agent takes any code but 2 as a hook
/// error that lets the tool go.
const USAGE_CODE: i32 = 64;

fn main() {
    let code = clauseo_host::pre_write_guard::main_entry(std::env::args_os().skip(1))
        .unwrap_or_else(|| {
            // Nothing more to do when stderr is closed; the exit code still says it.
            drop(std::io::stderr().lock().write_all(USAGE.as_bytes()));
            USAGE_CODE
        });
    std::process::exit(code);
}
