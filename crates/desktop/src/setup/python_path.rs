//! Clauseo's Python runtime on Claude's and Codex's session `PATH` (decision record 0048,
//! "PATH"): the [`PythonSlot`] the launch hook reads (`SetupHook::agent_path`, no I/O), and
//! [`PythonPath`], which fills it on the setup thread from `clauseo_agents::python_path_dirs`
//! and `clauseo_agents::python_tessdata_dir` (which read the disk) at start and on each
//! `AgentsEvent::Python`: `Ready` fills it, any other state empties it. Sign-in, status and
//! limits runs never read it.

use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

use clauseo_agents::PythonState;

/// The runtime's folders for a session, as the setup thread last looked them up: empty while
/// Python is not `Ready`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PythonFolders {
    /// The command folders for the session's `PATH`, in order.
    pub(crate) path: Vec<PathBuf>,
    /// The tesseract language folder, for `TESSDATA_PREFIX`, when the runtime has one.
    pub(crate) tessdata: Option<PathBuf>,
}

/// The runtime's folders, as the setup thread last looked them up.
pub(crate) type PythonSlot = Arc<RwLock<PythonFolders>>;

/// Looks the runtime's folders up (`clauseo_agents::python_path_dirs` and
/// `python_tessdata_dir`): they read the disk, so only the setup thread calls it.
pub(crate) type PythonLookup = Arc<dyn Fn() -> PythonFolders + Send + Sync>;

/// The slot and the lookup that fills it.
pub(crate) struct PythonPath {
    slot: PythonSlot,
    lookup: PythonLookup,
}

impl Default for PythonPath {
    /// A slot no hook reads, whose lookup finds nothing: the tests' backends, and a platform
    /// with no installs.
    fn default() -> Self {
        Self::new(PythonSlot::default(), Arc::new(PythonFolders::default))
    }
}

impl PythonPath {
    /// `slot` filled from `lookup`.
    pub(crate) fn new(slot: PythonSlot, lookup: PythonLookup) -> Self {
        Self { slot, lookup }
    }

    /// Fills the slot from the lookup, which is empty unless the runtime passes its receipt
    /// check: at the setup thread's start.
    pub(crate) fn fill(&self) {
        self.set((self.lookup)());
    }

    /// Follows one Python state: `Ready` fills the slot from the lookup, any other empties it.
    pub(crate) fn follow(&self, state: &PythonState) {
        if matches!(state, PythonState::Ready { .. }) {
            self.fill();
        } else {
            self.set(PythonFolders::default());
        }
    }

    fn set(&self, folders: PythonFolders) {
        *self.slot.write().unwrap_or_else(PoisonError::into_inner) = folders;
    }
}
