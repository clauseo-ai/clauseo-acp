//! `clauseo-sandbox-run`: Windows tier B's runner (S15 `PR5`). The host starts it through
//! `spawn_in_tree` with `--launch <data_dir>\sandbox\run\<16 hex>.json` (the outer runner, the
//! desktop user); it starts itself again as the matter's pool user with `--inner <pipe base>`
//! (the inner runner), which starts the agent under the (B) token; at the session's end the outer
//! starts it once more as the pool user with `--sweep` (the end sweeper). It shows no window; its
//! stdio is the agent's, and its exit code the agent's, or one of `RunExit`'s on a failure.
//!
//! The work is in `clauseo_sandbox::windows::runner`. On macOS and Linux it exits 2.
#![windows_subsystem = "windows"]

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    use clauseo_sandbox::windows::runner::{Mode, RunExit, mode, run_inner, run_outer, run_sweep};

    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let code = match mode(&args) {
        Some(Mode::Launch(file)) => match std::env::current_exe() {
            Ok(exe) => run_outer(&file, &exe),
            Err(_) => RunExit::Usage.code(),
        },
        Some(Mode::Inner(base)) => run_inner(&base),
        Some(Mode::Sweep) => run_sweep(),
        None => RunExit::Usage.code(),
    };
    exit_with(code)
}

/// Windows exit codes are 32 bits; `ExitCode` takes only a `u8`, so a code above 255 (an
/// agent's, or a `RunExit`) goes through `exit`.
#[cfg(windows)]
#[expect(
    clippy::exit,
    reason = "the runner's exit code is the agent's 32-bit code"
)]
fn exit_with(code: u32) -> ExitCode {
    match u8::try_from(code) {
        Ok(small) => ExitCode::from(small),
        Err(_) => std::process::exit(i32::from_ne_bytes(code.to_ne_bytes())),
    }
}

#[cfg(not(windows))]
#[expect(clippy::print_stderr, reason = "the one line a run off Windows gets")]
fn main() -> ExitCode {
    eprintln!("clauseo-sandbox-run runs agents in Clauseo's sandbox on Windows only");
    ExitCode::from(2)
}
