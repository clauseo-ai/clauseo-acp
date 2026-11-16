//! `clauseo-guard`: the pre-write guard as a console program, for Codex's hook on Windows. Codex
//! runs its hook line under PowerShell, which waits for a console program and reads its exit code;
//! `Clauseo.exe` is a GUI program in a release build, so on Windows the line names this program,
//! installed beside it (`agent::loads::codex_guard_program`). The body is the app's own
//! `--pre-write-guard` path: the arguments after the flag go to `pre_write_guard::run` with the
//! standard streams, and its exit code is returned as it is. Any other start ends with the refusing
//! code and the shared line on stderr.

use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::process::ExitCode;

use clauseo_host::pre_write_guard::{DENY, FLAG, UNCHECKED, run};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = match args.split_first() {
        Some((flag, rest)) if flag.as_os_str() == OsStr::new(FLAG) => run(
            rest,
            std::io::stdin().lock(),
            std::io::stdout().lock(),
            std::io::stderr().lock(),
        ),
        _ => {
            // Nothing more can be done when stderr is gone; the exit code still refuses.
            drop(writeln!(std::io::stderr(), "{UNCHECKED}"));
            DENY
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
