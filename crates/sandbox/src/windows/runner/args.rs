//! The runner's command lines: what the host starts [`RUNNER_EXE`](super::RUNNER_EXE) with,
//! and the mode its arguments name.

use std::path::{Path, PathBuf};

use crate::windows::sids::Sid;

/// What [`RUNNER_EXE`](super::RUNNER_EXE) was started to do, from its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// `--launch <file>`: the outer runner.
    Launch(PathBuf),
    /// `--inner <pipe base> <desktop user's SID>`: the inner runner, which grants that user
    /// `TOKEN_QUERY` on its token first (`token::runner_token_first_sddl`).
    Inner(String, Sid),
}

/// The mode `args` (the arguments after the program) name; `None` for any other command line.
pub fn mode(args: &[std::ffi::OsString]) -> Option<Mode> {
    match args {
        [flag, file] if flag == "--launch" => Some(Mode::Launch(PathBuf::from(file))),
        [flag, base, user] if flag == "--inner" => {
            let user = Sid::parse(user.to_str()?).ok()?;
            base.to_str().map(|base| Mode::Inner(base.to_owned(), user))
        }
        _ => None,
    }
}

/// The arguments the host starts [`RUNNER_EXE`](super::RUNNER_EXE) with.
pub fn runner_args(launch_file: &Path) -> Vec<std::ffi::OsString> {
    vec!["--launch".into(), launch_file.as_os_str().to_owned()]
}
