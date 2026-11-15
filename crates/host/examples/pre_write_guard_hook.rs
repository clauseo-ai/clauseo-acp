//! The pre-write guard as a small program of its own, run as the desktop app's fast path runs it
//! (`crates/desktop/src/pre_write_guard.rs`): `pre_write_guard_hook --pre-write-guard --matter
//! <folder> …`, one hook input on stdin. For probes of Claude Code's real hook calls (the
//! PowerShell read allowlist's end-to-end run) without building the app.

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some((first, rest)) = args.split_first() else {
        return ExitCode::from(64);
    };
    if first != clauseo_host::pre_write_guard::FLAG {
        return ExitCode::from(64);
    }
    let code = clauseo_host::pre_write_guard::run(
        rest,
        std::io::stdin().lock(),
        std::io::stdout(),
        std::io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
