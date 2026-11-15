//! The paths a Claude session's sandbox and its guard's stetdoc read fence keep it from.

use std::path::PathBuf;

use super::super::super::Actor;
use super::RunFolders;
use crate::pre_write_guard::GuardFence;

/// The built-in skills' folder under each spelling of `<data>`.
pub(super) fn built_in_skills(run: &RunFolders) -> Vec<PathBuf> {
    run.data_deny
        .spellings
        .iter()
        .map(|data| {
            crate::agent::data_fence::BUILT_IN_SKILLS
                .iter()
                .fold(data.clone(), |path, name| path.join(name))
        })
        .collect()
}

/// Every other matter's folder the host knows now; one registered later is the residual. The
/// Mac sandbox denies these and the guard's stetdoc read fence refuses them. When #1085 lands,
/// both take its matters this conversation is not invited into instead.
pub(super) fn other_matter_folders(actor: &Actor, run: &RunFolders) -> Vec<PathBuf> {
    actor
        .matters
        .iter()
        .filter(|(id, _)| **id != run.key.matter)
        .map(|(_, entry)| PathBuf::from(&entry.row.folder_path))
        .collect()
}

/// Where a Claude session's stetdoc reads may not go (`pre_write_guard::GuardFence`): each
/// spelling of `<data>` but the session's own folders in it (its run folder, the matter's
/// notes, the built-in skills), and every other matter's folder.
pub(super) fn guard_fence(actor: &Actor, run: &RunFolders) -> GuardFence {
    let mut own = vec![run.root.clone(), run.notes.clone()];
    own.extend(built_in_skills(run));
    GuardFence {
        data: run.data_deny.spellings.clone(),
        own,
        matters: other_matter_folders(actor, run),
        // Seam: `data_fence::protected_roots(&run.data_deny)` once ps-module-fence lands.
        protected: Vec::new(),
        overflow: false,
    }
}
