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
/// the matter folder (`stored`, and the real path the agent works in).
pub(super) fn launch_deny(
    run: &RunFolders,
    stored: &Path,
    data: &Path,
    claude_home: Option<&Path>,
) -> data_fence::LaunchDeny {
    let own = own_folders(run, stored, claude_home);
    let cwd = crate::agent::session_folder(stored);
    let projects = claude_home.map_or_else(Vec::new, |home| {
        data_fence::claude_projects(home, &[&cwd, stored])
    });
    data_fence::launch_deny(data_spellings(data), data, &own, &projects)
}

/// The session's own folders, which Claude's deny rules leave out: its run folder, this
/// matter's notes, the matter folder (`agent::session_folder` of `stored`) and, for Claude,
/// the plans folder in its home.
fn own_folders(run: &RunFolders, stored: &Path, claude_home: Option<&Path>) -> Vec<PathBuf> {
    let plans = claude_home.map(|home| home.join("plans"));
    [
        run.root.clone(),
        run.notes.clone(),
        crate::agent::session_folder(stored),
    ]
    .into_iter()
    .chain(plans)
    .collect()
}

/// `<data>/run`.
pub(super) fn run_dir(data: &Path) -> PathBuf {
    data.join("run")
}

/// `<data>/browser/run`: the parent of every session's browser folder, the guard's `--cwd`
/// (`RunFolders::browser`). It sits in `<data>/browser`, which Codex's shell fence denies
/// whole, read and write (`agent/codex_fence.rs` `DATA_DENIED`), so a sandboxed shell cannot
/// plant a link where the unfenced guard writes. Swept at start like `<data>/run`
/// (`sweep.rs`).
pub(super) fn browser_run_dir(data: &Path) -> PathBuf {
    data.join(crate::browser::CONFIG_DIR).join("run")
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

/// Removes this launch's run folder and its browser folder and all they hold, each when it
/// is a real folder; the notes folder stays. Only the folders the host made: the paths as
/// built (`<data>/run/<session id>`, `<data>/browser/run/<session id>`), never
/// canonicalized, removed with `remove_dir_all`, which removes a link inside and never
/// follows it. Called once the agent's process has exited, or when it never started. File
/// system calls: off the actor.
pub(super) fn remove_folders(run: &RunFolders) {
    let id = run.key.id();
    let named = |folder: &Path, parents: &[&str]| {
        let mut at = Some(folder);
        let names = std::iter::once(id.as_str()).chain(parents.iter().copied());
        names.into_iter().all(|name| {
            let this = at.take();
            at = this.and_then(Path::parent);
            this.and_then(Path::file_name) == Some(std::ffi::OsStr::new(name))
        })
    };
    for (folder, parents) in [
        (&run.root, &["run"][..]),
        (&run.browser, &["run", crate::browser::CONFIG_DIR][..]),
    ] {
        if !named(folder, parents) {
            tracing::warn!("not a session folder the host made; it stays");
            continue;
        }
        if crate::write::is_real_dir(folder) {
            remove_tree(folder);
        }
    }
}
