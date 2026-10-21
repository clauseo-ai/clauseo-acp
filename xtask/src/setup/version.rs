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
    Cmd::new(
        Program::Named(binary.name),
        ["version"],
        super::VERSION_TIMEOUT,
    )
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
        (Attempt::Installed, _) => after_install_fix(machine, name),
        (Attempt::Failed, _) => format!(
            "the install of {name} above failed: do what its fix says, then run cargo xtask \
             setup again"
        ),
    };
    Some((message, fix))
}

/// The fix when this run installed `name` and the check still does not give the pinned copy.
/// Running the install again would not change that. Either bin is off PATH, or some copy earlier
/// on PATH (or the new one) runs and is not the pinned version.
fn after_install_fix(machine: &Machine<'_>, name: &str) -> String {
    match &machine.cargo_bin {
        Some(bin) if machine.os != "windows" && !on_path(bin, &machine.path) => format!(
            "{name} was installed to {}, which is not on PATH: put it on PATH (in your shell \
             profile, for every new shell), then run cargo xtask setup again",
            bin.display()
        ),
        // A shell opened before winget ran lacks the PATH winget set.
        _ if machine.os == "windows" => format!(
            "open a new shell, which picks up the PATH winget set, then run `where {name}` to see \
             which copy of {name} it runs"
        ),
        _ => format!("run `which -a {name}` to see which copy of {name} this shell runs"),
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
