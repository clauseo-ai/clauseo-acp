//! The version check of a pinned binary, and the message and fix when it fails.

use super::{Binary, Machine};
use crate::process::{Cmd, Program};
use std::path::Path;

/// How the installed version compares with the pin.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VersionCheck {
    Matches,
    Differs(String),
    /// Found, but `<name> version` failed, could not start or ran past its timeout; holds what it
    /// printed on stderr, or why it did not run.
    Broken(String),
    /// Not found; holds the error.
    Missing(String),
}

/// Reads `<name> version` output: the trimmed stdout must equal the pin.
/// `ran` is the exit status, stdout and stderr of `<name> version`, or why it was not found.
pub(crate) fn compare_version(
    pinned: &str,
    ran: Result<(bool, String, String), String>,
) -> VersionCheck {
    match ran {
        Ok((true, stdout, _)) if stdout.trim() == pinned => VersionCheck::Matches,
        Ok((true, stdout, _)) => VersionCheck::Differs(stdout.trim().to_owned()),
        Ok((false, _, stderr)) => VersionCheck::Broken(stderr.trim().to_owned()),
        Err(error) => VersionCheck::Missing(error),
    }
}

/// `<name> version`, the command that prints the installed version.
pub(crate) fn version_cmd(binary: &Binary) -> Cmd {
    Cmd::new(Program::Named(binary.name), ["version"])
}

/// The message and fix for a version check that failed, or `None` when it matched. `how` says how
/// to install the binary on this machine, which a plain fix repeats. When `installed_now` is set,
/// this run has just installed the binary and it still does not give the pin, so the fix names
/// where the install put it and the likely cause instead of repeating `how`.
pub(crate) fn version_failure(
    binary: &Binary,
    check: &VersionCheck,
    how: &str,
    installed_now: bool,
    machine: &Machine<'_>,
) -> Option<(String, String)> {
    let name = binary.name;
    let install_fix = || {
        if installed_now {
            after_install_fix(machine, name)
        } else {
            how.to_owned()
        }
    };
    match check {
        VersionCheck::Matches => None,
        VersionCheck::Differs(found) => Some((
            format!("{name} version is {found}, the pin is {}", binary.version),
            install_fix(),
        )),
        VersionCheck::Broken(stderr) => Some((
            format!(
                "{name} is installed, but `{}` failed: {}",
                version_cmd(binary),
                if stderr.is_empty() {
                    "no output"
                } else {
                    stderr.as_str()
                }
            ),
            format!("reinstall it: {how}"),
        )),
        VersionCheck::Missing(why) if installed_now => Some((
            format!("{name} was installed, but this shell cannot run it ({why})"),
            install_fix(),
        )),
        VersionCheck::Missing(why) => {
            Some((format!("{name} is not installed ({why})"), install_fix()))
        }
    }
}

/// The fix when this run installed `name` and this shell still does not give the pinned copy:
/// where the install put it, and the likely cause. Running the install again would change neither.
fn after_install_fix(machine: &Machine<'_>, name: &str) -> String {
    if machine.os == "windows" {
        return format!(
            "{name} was installed to winget's Links folder, usually \
             %LOCALAPPDATA%\\Microsoft\\WinGet\\Links, but this shell does not run that copy. \
             Another copy earlier on PATH, or a PATH not yet refreshed, is the likely cause: run \
             `where {name}` to list the copies, or open a new shell and run cargo xtask setup again"
        );
    }
    // An install on macOS or Linux has a bin directory: without one it fails before this.
    let Some(bin) = &machine.cargo_bin else {
        return format!("run `which -a {name}` to list the copies of {name} on PATH");
    };
    let bin_shown = bin.display();
    if on_path(bin, &machine.path) {
        format!(
            "{name} was installed to {bin_shown}, but this shell runs another copy that comes \
             earlier on PATH: run `which -a {name}` to list the copies, then remove the other one \
             or put {bin_shown} before it on PATH"
        )
    } else {
        format!(
            "{name} was installed to {bin_shown}, which is not on PATH: put {bin_shown} on PATH \
             (in your shell profile, for every new shell), then run cargo xtask setup again"
        )
    }
}

/// Whether `dir` is one of the directories in `path`.
fn on_path(dir: &Path, path: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path).any(|entry| entry == dir)
}

#[cfg(test)]
mod tests;
