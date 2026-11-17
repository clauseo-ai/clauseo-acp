//! The data-fence and folder helpers of a launch: the deny rules Claude's data folder gets
//! (`agent/data_fence.rs`), the paths a session's folders sit at under the data folder, their
//! making ([`make`], [`make_guard`]) and the removal of a launch's run
//! folder. The callers in `launch_checks` call them off the actor.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, MatterId};

use super::{RunFolders, RunSweep};
use crate::agent::data_fence;
use crate::error::Error;

/// [`RunFolders::make`]: makes `run`'s folders, each through no link below `data`. Before
/// that, the host's first launch sets aside the `run` folder an earlier run left ([`RunSweep`]).
/// When one cannot be made, the run folder is taken back. File system calls: off the actor.
pub(super) fn make(run: &RunFolders, data: &Path, sweep: &RunSweep) -> Result<(), Error> {
    sweep.once(data);
    let tmp = crate::agent::temp::temp_dir(&run.root);
    for folder in [&run.work, &tmp, &run.notes] {
        if let Err(error) = crate::write::create_private_dirs_under(data, folder) {
            run.remove();
            return Err(Error::refused(
                ErrorCode::AgentLaunchFailed,
                format!("cannot make the folder {}: {error}", folder.display()),
            ));
        }
    }
    // The notes folder may be an earlier session's: it is set to 0700 whatever it was.
    #[cfg(unix)]
    if let Err(error) = crate::write::make_private_dir(&run.notes) {
        run.remove();
        return Err(Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!(
                "cannot make the folder {} private: {error}",
                run.notes.display()
            ),
        ));
    }
    Ok(())
}

/// Makes the pre-write guard's record of its refusals for `run`, for any agent but Claude
/// (`claude`), whose hook passes none (`updates/guard_blocks.rs`): the session's guard folder,
/// create-new, then the file in it with `O_EXCL`
/// ([`record::make`](crate::pre_write_guard::record::make)), and keeps the file and the identity
/// it read from it in [`RunFolders::guard`]. It sits in `<data>/browser`, which no agent's shell
/// or patch tool may write (Codex's fence denies it, `agent/codex_fence.rs` `DATA_DENIED`;
/// Claude's deny rules, `Read` and `Edit` of `browser/**`), so only the host makes it. Anything
/// already at the session's guard folder (a folder, a file, a symlink or a junction) refuses the
/// launch, the run folder taken back, as [`make`] does. File system calls: off the actor.
pub(super) fn make_guard(run: &mut RunFolders, data: &Path, claude: bool) -> Result<(), Error> {
    if claude {
        return Ok(());
    }
    match crate::pre_write_guard::record::make(data, &run.key.id()) {
        Ok(made) => {
            run.guard = Some(made);
            Ok(())
        }
        Err(error) => {
            run.remove();
            Err(Error::refused(
                ErrorCode::AgentLaunchFailed,
                format!("cannot make the guard's record file: {error}"),
            ))
        }
    }
}

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
/// the matter folder (`stored`, and the real path the agent works in); and the own work and
/// notes folders, which its allow rules let it list and read (`data_fence::own_allow_rules`);
/// and the run's packs plugin, denied for `Edit` alone (`LaunchDeny::edit_denied`).
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
    let mut deny = data_fence::launch_deny(data_spellings(data), data, &own, &projects);
    deny.own_reads = data_fence::own_reads(data, &[&run.work, &run.notes]);
    // The own run folder is left out above; its packs plugin, which Claude Code loads, is
    // still denied for `Edit`.
    for place in data_fence::own_reads(data, &[&run.skill_packs]) {
        if !deny.edit_denied.contains(&place) {
            deny.edit_denied.push(place);
        }
    }
    deny
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

/// Removes this launch's run folder, its browser folder and its guard folder and all they hold,
/// each when it is a real folder, with Claude's short temp folder the run folder names
/// (`agent::claude_tmp`) and the run's Claude settings files (`agent::launch_settings`); the notes
/// folder stays. Only the folders the host made: the paths as built (`<data>/run/<session id>`,
/// `<data>/browser/run/<session id>`, `<data>/browser/guard/<session id>`), never canonicalized,
/// removed with `remove_dir_all`, which removes a link inside and never follows it. Called once the
/// agent's process has exited, or when it never started. File system calls: off the actor.
pub(super) fn remove_folders(run: &RunFolders) {
    crate::agent::launch_settings::remove(&run.launch_settings);
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
    let guard = run.guard.as_ref().and_then(|(file, _)| file.parent());
    let guard_parents = [
        crate::pre_write_guard::record::FOLDER,
        crate::browser::CONFIG_DIR,
    ];
    let folders = [
        (run.root.as_path(), &["run"][..]),
        (
            run.browser.as_path(),
            &["run", crate::browser::CONFIG_DIR][..],
        ),
    ];
    for (folder, parents) in folders
        .into_iter()
        .chain(guard.map(|guard| (guard, &guard_parents[..])))
    {
        if !named(folder, parents) {
            tracing::warn!("not a session folder the host made; it stays");
            continue;
        }
        if folder == run.root {
            crate::agent::claude_tmp::remove_claude_tmp(folder);
        }
        if crate::write::is_real_dir(folder) {
            remove_tree(folder);
        }
    }
}
