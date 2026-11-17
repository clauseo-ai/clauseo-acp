//! What a Codex agent's launch recorded (`agents.rs`, `Agent::codex`).

use std::path::PathBuf;

use super::super::key_retry::KeyRetry;
use crate::pre_write_guard::self_test::SelfTest;

/// What a Codex agent's launch recorded.
#[derive(Debug)]
pub(in crate::actor::turn) struct CodexAgent {
    /// The matter folder as resolved through links at the launch: `_codex/hooks/list` names it.
    /// Each prompt and session call checks the stored folder for `.codex` first, resolved again.
    pub(in crate::actor::turn) folder: PathBuf,
    /// Its `CODEX_CONFIG` got a hooks object: its session start checks Codex trusts the guard.
    pub(in crate::actor::turn) guard: bool,
    /// Its one `authenticate` with its key (`key_retry.rs`).
    pub(in crate::actor::turn) key_retry: KeyRetry,
    /// The guard hook as Codex runs it, self-tested before the first prompt (none with no hook).
    pub(in crate::actor::turn) self_test: Option<SelfTest>,
}
