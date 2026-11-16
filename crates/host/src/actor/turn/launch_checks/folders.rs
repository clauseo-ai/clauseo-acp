//! The data-fence and folder helpers of a launch: the deny rules Claude's data folder gets
//! (`agent/data_fence.rs`), the paths a session's folders sit at under the data folder, their
//! making ([`make`], [`make_guard`]) and the removal of a launch's run
//! folder. The callers in `launch_checks` call them off the actor.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, MatterId};

use super::{RunFolders, RunSweep};
use crate::agent::data_fence;
use crate::agent::loads::sandbox::OtherMatter;
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

/// For a Claude launch: the data folder `data` (as configured) resolved once, and, when that
/// differs (`CLAUSEO_DATA_DIR` names a link), every folder of `run` under it re-spelled under
/// the real path (`RunFolders::respelled`), so the agent is handed only real spellings: its
/// sandbox denies the link by name and sits under the denied home, and Seatbelt has to read a
/// link to walk it, so nothing is reachable through the link spelling whatever the allow lists
/// say (stage C's Mac linked-data row on 0fcfd522b). The deny lists keep both spellings
/// ([`launch_deny`]). Returns the data folder to make the run's folders under. File system
/// calls: off the actor.
pub(super) fn respell_run(run: &mut RunFolders, data: &Path) -> PathBuf {
    let Ok(real) = dunce::canonicalize(data) else {
        return data.to_owned();
    };
    if real == data {
        return real;
    }
    run.respelled = Some((data.to_owned(), real.clone()));
    for path in [
        &mut run.root,
        &mut run.work,
        &mut run.notes,
        &mut run.browser,
        &mut run.agent_browser,
        &mut run.browser_config,
        &mut run.downloads,
        &mut run.browser_profile,
        &mut run.skill_packs,
        &mut run.relay_token,
    ] {
        *path = respelled(path, data, &real);
    }
    real
}

impl RunFolders {
    /// `path` under the data folder as configured, under its real path when the run was
    /// re-spelled ([`RunFolders::respelled`]); as it is otherwise.
    pub(crate) fn real(&self, path: &Path) -> PathBuf {
        self.respelled.as_ref().map_or_else(
            || path.to_owned(),
            |(link, real)| respelled(path, link, real),
        )
    }
}

/// `path` under `from` as the same path under `to`; as it is otherwise.
pub(super) fn respelled(path: &Path, from: &Path, to: &Path) -> PathBuf {
    path.strip_prefix(from)
        .map_or_else(|_| path.to_owned(), |rest| to.join(rest))
}

/// The launch's fences, made by the launch prep: Claude's data folder deny rules
/// ([`launch_deny`]), and for a Claude sandbox where the other matters' folders
/// ([`matter_spellings`] of what [`super::launch_paths`] set, as stored) and this session's
/// matter folder lead. A matter registered after the launch is in no list (the accepted
/// residual).
pub(super) fn fences(run: &mut RunFolders, stored: &Path, data: &Path, home: Option<&Path>) {
    run.data_deny = launch_deny(run, stored, data, home);
    if run.claude_tmp.is_some() {
        matter_spellings(&mut run.other_matters);
        run.matter_walked = walked(&crate::agent::session_folder(stored));
    }
}

/// Sets where each of `others`' folders leads ([`walked`]). Claude Code's sandbox resolves no
/// link in a deny, so a matter registered through one is denied by both, unless where it leads
/// is a place the sandbox must leave open (`agent::loads::sandbox::links`).
pub(super) fn matter_spellings(others: &mut [OtherMatter]) {
    for other in others {
        other.walked = walked(&other.stored);
    }
}

/// Where `folder` leads (the network-safe walk, `network_paths::touchable`) when that differs
/// from it; none when it is on a share, does not resolve, or is not absolute.
pub(super) fn walked(folder: &Path) -> Option<PathBuf> {
    let net = crate::network_paths::NetEnv::current();
    let disk = crate::network_paths::resolve::Disk::resolved(&net);
    let walked = folder
        .is_absolute()
        .then(|| crate::network_paths::touchable(folder, folder, &disk))
        .flatten();
    walked.filter(|walked| walked != folder)
}

/// Claude's data folder deny rules for this launch: every entry of `data` that is not the
/// session's own ([`own_folders`]), nor, for Claude, a `tool-results` folder in its project for
/// the matter folder (`stored`, and the real path the agent works in); and the own work and
/// notes folders, which its allow rules let it list and read (`data_fence::own_allow_rules`).
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
    // Under a linked data folder `data` is the real path (`respell_run`); the link is denied too.
    let mut spellings = data_spellings(data);
    let link = run.respelled.as_ref().map(|(link, _)| link);
    spellings.extend(link.filter(|link| !spellings.contains(link)).cloned());
    let mut deny = data_fence::launch_deny(spellings, data, &own, &projects);
    deny.own_reads = data_fence::own_reads(data, &[&run.work, &run.notes]);
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
