//! The `git` that does nothing, first on every agent's `PATH` (`HostConfig::with_no_git`): the
//! program the desktop copied, and the SHA-256 it copied. A child of `wiring.rs`, which holds
//! it, so `config.rs` stays within its size limit. The launch prep checks the program at every
//! launch of an agent with a kind (`agent/no_git.rs`) and refuses the launch with
//! `agent.tool_missing` when it is missing or changed.

use std::path::PathBuf;

use crate::config::HostConfig;

/// The program and what it must hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoGit {
    /// `<data>/tools/no-git/git` (`git.exe` on Windows), absolute; its folder goes first on the
    /// agent's `PATH`.
    pub(crate) program: PathBuf,
    /// The SHA-256 of the bytes the desktop copied there; `None` when the copy failed, which
    /// refuses every launch of an agent with a kind.
    pub(crate) sha256: Option<[u8; 32]>,
}

impl NoGit {
    /// The program at `program` holding the bytes whose SHA-256 is `sha256` (`None`: the copy
    /// failed).
    pub fn new(program: PathBuf, sha256: Option<[u8; 32]>) -> Self {
        Self { program, sha256 }
    }

    /// The program.
    pub fn program(&self) -> &std::path::Path {
        &self.program
    }

    /// The SHA-256 it must hold; `None` when the copy failed.
    pub fn sha256(&self) -> Option<[u8; 32]> {
        self.sha256
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "config.rs is at its 600-line limit; the PATH program's setter lives with its type"
)]
impl HostConfig {
    /// The `git` that does nothing (default: none, for tests whose agents have no kind). With
    /// it, every launch of a Claude or Codex agent checks the program first and puts its folder
    /// first on the agent's `PATH`, and its shells' (`agent/no_git.rs`); a program missing or
    /// changed refuses the launch with `agent.tool_missing`. The host's own process never gets
    /// the folder: only the agent's launch environment does.
    #[must_use]
    pub fn with_no_git(mut self, no_git: NoGit) -> Self {
        self.wiring.no_git = Some(no_git);
        self
    }

    /// The program `with_no_git` set, for a caller's tests.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn no_git(&self) -> Option<&NoGit> {
        self.wiring.no_git.as_ref()
    }
}
