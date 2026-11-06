//! The data-fence and folder-path helpers of a launch: the deny rules Claude's data folder gets
//! (`agent/data_fence.rs`), the paths a session's folders sit at under the data folder, and the
//! removal of a launch's run folder. Nothing here is made; the callers in `launch_checks` make and
//! remove the folders, off the actor.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::RunFolders;
use crate::agent::data_fence;

/// The data folder canonicalised, then as configured when that differs; as configured alone
/// when it does not resolve (the data-folder refusal has refused that launch already).
fn data_spellings(data: &Path) -> Vec<PathBuf> {
    let mut spellings: Vec<PathBuf> = dunce::canonicalize(data).into_iter().collect();
    if !spellings.iter().any(|canonical| canonical == data) {
        spellings.push(data.to_owned());
    }
    spellings
}

/// Claude's data folder deny rules for this launch: every entry of `data` that is not the
/// session's own ([`own_folders`]), nor, for Claude, a `tool-results` folder in its project for
/// the matter folder (`stored`, and the real path the agent works in) in its state folder
/// (`claude_state`, its `CLAUDE_CONFIG_DIR`).
pub(super) fn launch_deny(
    run: &RunFolders,
    stored: &Path,
    data: &Path,
    claude_state: Option<&Path>,
) -> data_fence::LaunchDeny {
    let own = own_folders(run, stored);
    let cwd = crate::agent::session_folder(stored);
    let projects = claude_state.map_or_else(Vec::new, |state| {
        data_fence::claude_projects(state, &[&cwd, stored])
    });
    data_fence::launch_deny(data_spellings(data), data, &own, &projects)
}

/// The session's own folders, which Claude's deny rules leave out: its run folder, this
/// matter's notes, the matter folder (`agent::session_folder` of `stored`) and the agent's
/// state folder for this matter (R17 PR-C), whose plans folder and tool results Claude needs;
/// [`data_fence::DENIED`] still names the config surfaces and transcripts in it. Every other
/// matter's state folder that exists is denied whole.
fn own_folders(run: &RunFolders, stored: &Path) -> Vec<PathBuf> {
    [
        run.root.clone(),
        run.notes.clone(),
        crate::agent::session_folder(stored),
    ]
    .into_iter()
    .chain(run.state.clone())
    .collect()
}

/// `<data>/run`.
pub(super) fn run_dir(data: &Path) -> PathBuf {
    data.join("run")
}

/// `<data>/agent-state/<matter>/notes`.
pub(super) fn notes_dir(data: &Path, matter: MatterId) -> PathBuf {
    data.join("agent-state")
        .join(matter.to_string())
        .join("notes")
}

/// Removes `folder` and all it holds, through no link; a failure is a warning.
pub(super) fn remove_tree(folder: &Path) {
    if let Err(error) = crate::write::remove_tree(folder) {
        tracing::warn!(%error, "a session's run folder stays");
    }
}
