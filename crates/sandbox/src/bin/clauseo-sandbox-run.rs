//! `clauseo-sandbox-run`: Windows tier B's runner (S15 `PR5`). The host starts it through
//! `spawn_in_tree` with `--launch <data_dir>\sandbox\run\<16 hex>.json` (the outer runner, the
//! desktop user); it starts itself again as the matter's pool user with `--inner <pipe base>`
//! (the inner runner), which starts the agent under the (B) token; at the session's end the outer
//! asks the Task Scheduler to run the `SYSTEM` task `\Clauseo\SandboxSweep`. It shows no window;
//! its stdio is the agent's, and its exit code the agent's, or one of `RunExit`'s on a failure.
//! What it says itself on stderr goes through `runner::stderr`: one `kind=` line a refusal.
//!
//! The work is in `clauseo_sandbox::windows::runner`. On macOS and Linux it exits 2.
#![windows_subsystem = "windows"]

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    use clauseo_sandbox::windows::runner::stderr::{RunnerKind, install_panic_hook};
    use clauseo_sandbox::windows::runner::{Mode, mode, run_inner, run_outer};

    // Before anything can panic: a panic says only its kind (`stderr::install_panic_hook`).
    // `run_outer` and `run_inner` install it again, for any other caller.
    install_panic_hook();
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let code = match mode(&args) {
        Some(Mode::Launch(file)) => match std::env::current_exe() {
            Ok(exe) => run_outer(&file, &exe),
            Err(_) => refused(RunnerKind::Internal),
        },
        Some(Mode::Inner(base)) => run_inner(&base),
        None => refused(RunnerKind::Usage),
    };
    exit_with(code)
}

/// Writes the refusal of `kind`; its exit code is the kind's `RunExit`, so the host takes the
/// kind (it does only when the two agree).
#[cfg(windows)]
fn refused(kind: clauseo_sandbox::windows::runner::stderr::RunnerKind) -> u32 {
    use clauseo_sandbox::windows::runner::RunExit;
    use clauseo_sandbox::windows::runner::stderr::{Detail, refuse};
    refuse(kind, Detail::none());
    kind.exit().unwrap_or(RunExit::Usage).code()
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
fn main() -> ExitCode {
    use clauseo_sandbox::windows::runner::stderr::{
        Detail, RunnerKind, install_panic_hook, refuse,
    };
    install_panic_hook();
    refuse(RunnerKind::NotWindows, Detail::none());
    ExitCode::from(2)
}
