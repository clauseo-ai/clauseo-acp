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

/// What this run did to install a binary before its last version check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attempt {
    /// Nothing: a plain `setup`, or the first check already matched.
    None,
    /// Installed it.
    Installed,
    /// Tried and failed; that failure has printed its own fix.
    Failed,
}

/// The message and fix for a version check that failed, or `None` when it matched. `how` says how
/// to install the binary on this machine, which the fix repeats only when `attempt` is
/// `Attempt::None`. After an install, the fix names where the install put the binary and the
/// likely cause; after a failed install, it points at that failure's fix.
pub(crate) fn version_failure(
    binary: &Binary,
    check: &VersionCheck,
    how: &str,
    attempt: Attempt,
    machine: &Machine<'_>,
) -> Option<(String, String)> {
    let name = binary.name;
    let message = match check {
        VersionCheck::Matches => return None,
        VersionCheck::Differs(found) => {
            format!("{name} version is {found}, the pin is {}", binary.version)
        }
        VersionCheck::Broken(stderr) => format!(
            "{name} is installed, but `{}` failed: {}",
            version_cmd(binary),
            if stderr.is_empty() {
                "no output"
            } else {
                stderr.as_str()
            }
        ),
        VersionCheck::Missing(why) if attempt == Attempt::Installed => {
            format!("{name} was installed, but this shell cannot run it ({why})")
        }
        VersionCheck::Missing(why) => format!("{name} is not installed ({why})"),
    };
    let fix = match (attempt, check) {
        (Attempt::None, VersionCheck::Broken(_)) => format!("reinstall it: {how}"),
        (Attempt::None, _) => how.to_owned(),
        (Attempt::Installed, _) => after_install_fix(machine, name, check),
        (Attempt::Failed, _) => format!(
            "the install of {name} above failed: do what its fix says, then run cargo xtask \
             setup again"
        ),
    };
    Some((message, fix))
}

/// The fix when this run installed `name` and `check` still does not give the pinned copy: where
/// the install put it, and the likely cause. Running the install again would change neither.
fn after_install_fix(machine: &Machine<'_>, name: &str, check: &VersionCheck) -> String {
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
    let installed = format!("{name} was installed to {bin_shown}");
    let profile = "in your shell profile, for every new shell";
    match (check, on_path(bin, &machine.path)) {
        // A copy at another version ran, so it is the one this shell finds first.
        (VersionCheck::Differs(_), true) => format!(
            "{installed}, but this shell runs another copy that comes earlier on PATH: run \
             `which -a {name}` to list the copies, then remove the other one or put {bin_shown} \
             before it on PATH"
        ),
        (VersionCheck::Differs(_) | VersionCheck::Broken(_), false) => format!(
            "{installed}, which is not on PATH, so this shell runs another copy: put {bin_shown} on \
             PATH before that copy ({profile}; `which -a {name}` lists the copies), then run \
             cargo xtask setup again"
        ),
        // Some copy ran and failed: the new one, or another that comes earlier on PATH.
        (VersionCheck::Broken(_), true) => format!(
            "{installed}, but `{name} version` fails in this shell: run `which -a {name}` to see \
             which copy runs. If it is {}, run `{} version` to see why it fails; otherwise remove \
             the other copy or put {bin_shown} before it on PATH",
            bin.join(name).display(),
            bin.join(name).display()
        ),
        (_, false) => format!(
            "{installed}, which is not on PATH: put {bin_shown} on PATH ({profile}), then run cargo \
             xtask setup again"
        ),
        // Nothing ran although bin is on PATH: the installed file itself cannot start here.
        (_, true) => format!(
            "{installed}, which is on PATH, but it could not be started: run \
             `{}` to see why, then run cargo xtask setup again",
            bin.join(name).display()
        ),
    }
}

/// Whether `dir` is one of the directories in `path`. Both sides are compared after
/// `canonicalize`, so a symlinked or relative directory still matches; an entry that cannot be
/// canonicalized (it does not exist, say) is compared as written.
fn on_path(dir: &Path, path: &std::ffi::OsStr) -> bool {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let dir_real = real(dir);
    std::env::split_paths(path).any(|entry| entry == dir || real(&entry) == dir_real)
}

#[cfg(test)]
mod tests;
