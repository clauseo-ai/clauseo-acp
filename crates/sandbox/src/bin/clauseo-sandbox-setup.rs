//! `clauseo-sandbox-setup`: the elevated set-up of Windows tier B (S15 `PR4h`). The app starts
//! it through `UserSetup` and the uninstaller's hook with `--remove`; its manifest asks for an
//! administrator (`build.rs`, D-B14), so Windows raises one prompt. It shows no window and
//! answers through its exit code (`SetupExit`) and, for `--set-up` and `--reclaim`,
//! `<data_dir>\sandbox\setup-result.json`, written as the user who asked.
//!
//! The work is in `clauseo_sandbox::windows::setup::exe`. On macOS and Linux it exits `Usage`
//! (1) with one line.
#![windows_subsystem = "windows"]

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    // `exe::main` keeps the DLL search to System32 before anything else, so the delay-loaded
    // `netapi32.dll` and `userenv.dll` (`build.rs`) load only after it. `args_os` reads the
    // command line through kernel32 alone; nothing in this crate or its dependencies calls
    // either DLL before `main` (no statics run code at start-up).
    let exit = clauseo_sandbox::windows::setup::exe::main(std::env::args_os().skip(1));
    ExitCode::from(exit.as_code())
}

#[cfg(not(windows))]
#[expect(clippy::print_stderr, reason = "the one line a run off Windows gets")]
fn main() -> ExitCode {
    eprintln!("clauseo-sandbox-setup sets up Clauseo's protection on Windows only");
    // `SetupExit::Usage`, which this build doesn't compile in.
    ExitCode::from(1)
}
