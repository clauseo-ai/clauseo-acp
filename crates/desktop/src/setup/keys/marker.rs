//! The "key stored" marker (decision record 0048, R20 P5b A4): which agents Clauseo stored an
//! API key for, kept outside the keychain in `<data_dir>/agents/key-stored.json`, beside the
//! `firm-policy` markers (`setup/firm_policy.rs`) and written as they are, whole and atomic
//! through `Files::write_atomic`. It holds agent ids only, never a key or a keychain item's
//! name: `{"agents": ["claude-acp"]}`.
//!
//! It lets the first load tell, when the keychain does not answer within its bound, which
//! agents may have a stored key: an agent without the marker launches without a key, one with
//! it is refused with `agent.key_unreadable`. It fails closed:
//!
//! - a save sets the marker before the keychain write, and a marker that cannot be written
//!   refuses the save; a clear removes the key first, then the marker. A marker left on by a
//!   failure only refuses launches until a read answers.
//! - no file (a fresh install) marks no agent; a file that cannot be read or does not parse
//!   marks every agent ([`Marked::All`]), logged by the error's kind only. A write over such a
//!   file starts from every known agent marked, so it never unmarks one by accident.
//! - each keychain read that answers brings the marker in step, best effort
//!   ([`KeyMarker::reconcile`]): a key sets it, no key clears it.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_protocol::AgentId;
use clauseo_settings::{Files, SystemFiles};
use serde::{Deserialize, Serialize};

use crate::bridge::lock;

/// The marker file's name, in `<data_dir>/agents/`.
pub(crate) const MARKER_FILE: &str = "key-stored.json";

/// What the marker file holds.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkerText {
    agents: BTreeSet<AgentId>,
}

/// Which agents the marker marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Marked {
    /// The file could not be read or parsed: every agent counts as marked.
    All,
    /// These agents, and no other.
    Only(BTreeSet<AgentId>),
}

impl Marked {
    /// Whether `agent` counts as marked.
    pub(crate) fn contains(&self, agent: &AgentId) -> bool {
        match self {
            Self::All => true,
            Self::Only(agents) => agents.contains(agent),
        }
    }
}

/// The marker file, the files it goes through, and the agents a write over an unreadable file
/// marks.
pub(crate) struct KeyMarker {
    /// `None` in the tests that keep no marker: every agent marked, no write made.
    path: Option<PathBuf>,
    files: Arc<dyn Files + Send + Sync>,
    known: BTreeSet<AgentId>,
    /// Each read, change and write of the file as one step.
    writes: Mutex<()>,
}

impl std::fmt::Debug for KeyMarker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyMarker")
            .field("kept", &self.path.is_some())
            .finish_non_exhaustive()
    }
}

impl KeyMarker {
    /// The marker in `data_dir`, over the real file system, for the `known` agents.
    pub(crate) fn new<'a>(data_dir: &Path, known: impl IntoIterator<Item = &'a AgentId>) -> Self {
        Self::with_files(path(data_dir), Arc::new(SystemFiles), known)
    }

    /// The marker at `path`, through `files`.
    pub(crate) fn with_files<'a>(
        path: PathBuf,
        files: Arc<dyn Files + Send + Sync>,
        known: impl IntoIterator<Item = &'a AgentId>,
    ) -> Self {
        Self {
            path: Some(path),
            files,
            known: known.into_iter().cloned().collect(),
            writes: Mutex::default(),
        }
    }

    /// No marker kept: every agent counts as marked, as before it, and no write is made.
    #[cfg(test)]
    pub(crate) fn unkept() -> Self {
        Self {
            path: None,
            files: Arc::new(SystemFiles),
            known: BTreeSet::new(),
            writes: Mutex::default(),
        }
    }

    /// Which agents are marked; an unreadable or corrupt file is logged by its kind and marks
    /// every agent.
    pub(crate) fn marked(&self) -> Marked {
        let _writes = lock(&self.writes);
        match self.read() {
            Ok(marked) => marked,
            Err(error) => {
                tracing::warn!(
                    kind = ?error.kind(),
                    "the key-stored marker is unreadable: every agent counts as marked"
                );
                Marked::All
            }
        }
    }

    /// Marks `agent`: a save's first step, before the keychain write. An error means nothing
    /// may be stored.
    pub(crate) fn set(&self, agent: &AgentId) -> io::Result<()> {
        self.change(agent, true)
    }

    /// Unmarks `agent`, best effort: after a clear removed its key. A failure is logged and
    /// leaves it marked, which only refuses its launches until a read answers.
    pub(crate) fn clear(&self, agent: &AgentId) {
        self.change_logged(agent, false);
    }

    /// Brings `agent`'s mark in step with a keychain read that answered: marked when a key is
    /// stored, unmarked when none is. Best effort: a failure is logged.
    pub(crate) fn reconcile(&self, agent: &AgentId, stored: bool) {
        self.change_logged(agent, stored);
    }

    /// [`KeyMarker::change`], a failure logged by the agent and the error's kind.
    fn change_logged(&self, agent: &AgentId, on: bool) {
        if let Err(error) = self.change(agent, on) {
            let kind = error.kind();
            tracing::warn!(%agent, on, ?kind, "the key-stored marker was not written");
        }
    }

    /// Marks `agent` (`on`) or unmarks it, writing the file only when that changes it. Over an
    /// unreadable file, every known agent starts marked.
    fn change(&self, agent: &AgentId, on: bool) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let _writes = lock(&self.writes);
        let (mut agents, readable) = match self.read() {
            Ok(Marked::Only(agents)) => (agents, true),
            Ok(Marked::All) | Err(_) => (self.known.clone(), false),
        };
        if readable && agents.contains(agent) == on {
            return Ok(());
        }
        if on {
            agents.insert(agent.clone());
        } else {
            agents.remove(agent);
        }
        let text = serde_json::to_vec_pretty(&MarkerText { agents }).map_err(io::Error::other)?;
        self.files.write_atomic(path, &text)
    }

    /// The file's marks: none when it is not there; an error when it cannot be read or parsed.
    fn read(&self) -> io::Result<Marked> {
        let Some(path) = &self.path else {
            return Ok(Marked::All);
        };
        let Some(bytes) = self.files.read(path)? else {
            return Ok(Marked::Only(BTreeSet::new()));
        };
        let text: MarkerText = serde_json::from_slice(&bytes)
            .map_err(|_corrupt| io::Error::from(io::ErrorKind::InvalidData))?;
        Ok(Marked::Only(text.agents))
    }
}

/// Where the marker is in `data_dir`.
pub(crate) fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join(MARKER_FILE)
}

#[cfg(test)]
mod tests;
