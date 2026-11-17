//! Which re-check a session's matter folder gets before each prompt and session call
//! (`codex::prepare::recheck`), by its agent's kind.

use std::path::PathBuf;

use super::Agent;
use crate::agent::{claude_files, codex};

/// The stored matter folder and its kind's refusal: a Codex agent's `.codex`, a Claude agent's
/// entries (`agent/claude_files.rs`); `None` for an agent of no kind.
pub(in crate::actor::turn) fn folder_recheck(
    agent: &Agent,
) -> Option<(PathBuf, codex::prepare::Refusal)> {
    let refusal: codex::prepare::Refusal = if agent.codex.is_some() {
        codex::refuse_codex_folder
    } else if agent.claude_home.is_some() {
        claude_files::refuse_claude_entries
    } else {
        return None;
    };
    Some((agent.folder.clone(), refusal))
}
