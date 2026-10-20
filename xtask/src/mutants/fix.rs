//! The fix lines of `cargo xtask mutants`, typed; `tests.rs` pins each text.

use crate::changes::GitError;
use std::fmt;

/// One fix line of `cargo xtask mutants`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A mutant no test failed on.
    Missed,
    /// The run reached `MUTANTS_TIMEOUT`.
    Split,
    /// cargo-mutants is missing, or at another version than `pinned`.
    Setup { pinned: &'static str },
    /// git did not start.
    GitNotStarted,
    /// `command`, a git command, failed.
    GitFailed { command: String },
    /// A git command ran past its limit; the message names it.
    GitTimedOut,
    /// The file or folder at `path` could not be written or removed.
    Write { path: &'static str },
    /// `cmd` did not start, failed without saying why in `outcomes.json`, or stopped early.
    ByHand { cmd: String },
    /// `outcomes.json` could not be read or parsed.
    Unparsed,
    /// The unmutated tree failed to build or pass its tests in cargo-mutants' copy.
    Baseline,
    /// A mutant got an outcome xtask does not expect; its log is at `log`.
    Unexpected { log: String },
    /// A mutant made the tests run past cargo-mutants' time limit.
    Slow,
    /// Some mutants did not build.
    Unviable,
}

impl Fix {
    /// The fix for a git command that gave no diff.
    pub(crate) fn git(error: &GitError) -> Fix {
        match error {
            GitError::NotStarted(_) => Fix::GitNotStarted,
            GitError::Failed { command, .. } => Fix::GitFailed {
                command: command.clone(),
            },
            GitError::TimedOut(_) => Fix::GitTimedOut,
        }
    }
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::Missed => f.write_str(
                "add a test that fails on this change, or exclude it with a reason in \
                 .cargo/mutants.toml (the code owner approves it)",
            ),
            Fix::Split => {
                f.write_str("split the change: cargo xtask mutants caps a run at 20 minutes")
            }
            Fix::Setup { pinned } => write!(
                f,
                "run `cargo xtask setup`, which installs cargo-mutants {pinned}, the version \
                 xtask/src/setup.rs pins"
            ),
            Fix::GitNotStarted => f.write_str("install git, or put it on PATH"),
            Fix::GitFailed { command } => write!(
                f,
                "run `{command}` by hand to see why it fails; `git fetch origin main` fetches a \
                 missing origin/main"
            ),
            Fix::GitTimedOut => {
                f.write_str("run the git command above by hand to see why it does not finish")
            }
            Fix::Write { path } => write!(
                f,
                "make {path} writable (check the disk has room), then run cargo xtask mutants again"
            ),
            Fix::ByHand { cmd } => write!(f, "run `{cmd}` by hand to see what goes wrong"),
            Fix::Unparsed => f.write_str(
                "run cargo xtask mutants again; if it fails the same way, make \
                 xtask/src/mutants/outcomes.rs read the outcomes.json of the cargo-mutants \
                 version setup pins",
            ),
            Fix::Baseline => f.write_str(
                "read the log: the build and tests must pass in a copy of the tree that holds \
                 .git but not what .gitignore excludes, as .cargo/mutants.toml sets",
            ),
            Fix::Unexpected { log } => {
                write!(f, "read {log}, then run cargo xtask mutants again")
            }
            Fix::Slow => f.write_str(
                "none when the change makes the code loop forever, which the tests then catch; \
                 if the tests were only slow on a busy machine, run cargo xtask mutants again",
            ),
            Fix::Unviable => f.write_str(
                "none: a mutant that does not compile cannot be tested, and the compiler \
                 already refuses that change",
            ),
        }
    }
}
