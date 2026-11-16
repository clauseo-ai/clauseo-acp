//! Clauseo's Python runtime on Claude's and Codex's session `PATH` (decision record 0048,
//! "PATH"): the [`PythonSlot`] the launch hook reads (`SetupHook::agent_path`, no I/O), and
//! [`PythonPath`], which fills it on the setup thread from `clauseo_agents::python_path_dirs`
//! (which reads the disk) at start and on each `AgentsEvent::Python`: `Ready` fills it, any
//! other state empties it. Sign-in, status and limits runs never read it.

use std::path::PathBuf;
use std::sync::{Arc, PoisonError, RwLock};

use clauseo_agents::PythonState;

/// The runtime's folders for a session's `PATH`, as the setup thread last looked them up:
/// empty while Python is not `Ready`.
pub(crate) type PythonSlot = Arc<RwLock<Vec<PathBuf>>>;

/// Looks the runtime's folders up (`clauseo_agents::python_path_dirs`): it reads the disk, so
/// only the setup thread calls it.
pub(crate) type PythonLookup = Arc<dyn Fn() -> Vec<PathBuf> + Send + Sync>;

/// The slot and the lookup that fills it.
pub(crate) struct PythonPath {
    slot: PythonSlot,
    lookup: PythonLookup,
}

impl Default for PythonPath {
    /// A slot no hook reads, whose lookup finds nothing: the tests' backends, and a platform
    /// with no installs.
    fn default() -> Self {
        Self::new(PythonSlot::default(), Arc::new(Vec::<PathBuf>::new))
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
            self.set(Vec::new());
        }
    }

    fn set(&self, dirs: Vec<PathBuf>) {
        *self.slot.write().unwrap_or_else(PoisonError::into_inner) = dirs;
    }
}
