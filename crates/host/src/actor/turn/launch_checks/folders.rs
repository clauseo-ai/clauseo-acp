//! The data-fence and folder helpers of a launch: the deny rules Claude's data folder gets
//! (`agent/data_fence.rs`), the paths a session's folders sit at under the data folder, their
//! making ([`make`], [`make_guard`]) and the removal of a launch's run
//! folder. The callers in `launch_checks` call them off the actor.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, MatterId, ThreadId};

use super::{RunFolders, RunSweep};
use crate::agent::data_fence;
use crate::error::Error;

/// [`RunFolders::make`]: makes `run`'s folders, each through no link below `data`. Before
/// that, the host's first launch sets aside the `run` folder an earlier run left ([`RunSweep`]).
/// When one cannot be made, the run folder is taken back and the launch fails as
/// `agent.home_failed`: these are the agent's own folders, as its home is. File system calls: off
/// the actor.
pub(super) fn make(run: &RunFolders, data: &Path, sweep: &RunSweep) -> Result<(), Error> {
    sweep.once(data);
    let tmp = crate::agent::temp::temp_dir(&run.root);
    clear_work(&run.work);
    for folder in [&run.work, &tmp, &run.notes] {
        if let Err(error) = crate::write::create_private_dirs_under(data, folder) {
            run.remove();
            return Err(Error::refused(
                ErrorCode::AgentHomeFailed,
                format!("cannot make the folder {}: {error}", folder.display()),
            ));
        }
    }
    // The work and notes folders may be an earlier session's ([`ready_own`]).
    if let Err((folder, error)) = ready_own(&run.work, &run.notes) {
        run.remove();
        return Err(Error::refused(
            ErrorCode::AgentHomeFailed,
            format!("cannot ready the folder {}: {error}", folder.display()),
        ));
    }
    make_fallback_folder(data, &run.notes);
    Ok(())
}

/// Makes the pre-write guard's record of its refusals for `run`, for any agent but Claude
/// (`claude`), whose hook passes none (`updates/guard_blocks.rs`): the session's guard folder,
/// create-new, then the file in it with `O_EXCL`
/// ([`record::make`](crate::pre_write_guard::record::make)), and keeps the file and the identity
/// it read from it in [`RunFolders::guard`]. It sits in `<data>/browser`, which no agent's
/// sandboxed shell or patch tool may write (Codex's sandbox names no writable root there,
/// `agent/codex_fence.rs`; Claude's deny rules, `Read` and `Edit` of `browser/**`), so only
/// the host makes it; a Codex command its reviewer approves runs unsandboxed. Anything
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
    let mut deny = data_fence::launch_deny(data_spellings(data), data, &own, &projects);
    deny.own_reads = data_fence::own_reads(data, &[&run.work, &run.notes]);
    deny
}

/// The session's own folders, which Claude's deny rules leave out: its run folder, its work
/// folder, this matter's notes, the matter folder (`agent::session_folder` of `stored`) and,
/// for Claude, the plans folder in its home; and this matter's `history.git`
/// (`agent::history`), which the Windows shell rules then leave a command to name for reading
/// with `git`, while `matters/**` still denies it to Claude's Read and Edit
/// (`data_fence::DENIED`).
fn own_folders(run: &RunFolders, stored: &Path, claude_home: Option<&Path>) -> Vec<PathBuf> {
    let plans = claude_home.map(|home| home.join("plans"));
    let history = crate::agent::history::matter_history(&run.notes);
    [
        run.root.clone(),
        run.work.clone(),
        run.notes.clone(),
        crate::agent::session_folder(stored),
    ]
    .into_iter()
    .chain(plans)
    .chain(history)
    .collect()
}

/// `<data>/run`.
pub(super) fn run_dir(data: &Path) -> PathBuf {
    data.join("run")
}

/// `<data>/browser/run`: the parent of every session's browser folder, the guard's `--cwd`
/// (`RunFolders::browser`). It sits in `<data>/browser`, where Codex's sandbox names no
/// writable root (`agent/codex_fence.rs`), so a sandboxed shell cannot plant a link where the
/// unfenced guard writes; a command Codex's reviewer approves runs unsandboxed. Swept at start
/// like `<data>/run` (`sweep.rs`).
pub(super) fn browser_run_dir(data: &Path) -> PathBuf {
    data.join(crate::browser::CONFIG_DIR).join("run")
}

/// `<data>/agent-state/<matter>/notes`.
pub(super) fn notes_dir(data: &Path, matter: MatterId) -> PathBuf {
    crate::agent::matter_state_dir(data, matter).join("notes")
}

/// Makes the browser's fallback downloads folder beside `notes`
/// (`agent::loads::browser::fallback_downloads`) at the launch, before the fences list the
/// matter's state, so they always deny it; one the guard made mid-session would be in neither
/// until the next launch. A failure is logged, and the launch goes on.
fn make_fallback_folder(data: &Path, notes: &Path) {
    let fallback = crate::agent::loads::browser::fallback_downloads(notes);
    if let Err(error) = crate::write::create_private_dirs_under(data, &fallback) {
        tracing::warn!(
            error = %crate::log_file::Cause(&error),
            "the browser's fallback downloads folder was not made"
        );
    }
}

/// `<data>/agent-state/<matter>/work/<thread>`: the conversation's work folder, the same at
/// every launch of its agent, so a resumed session finds the paths it wrote before.
pub(crate) fn work_dir(data: &Path, matter: MatterId, thread: ThreadId) -> PathBuf {
    data.join("agent-state")
        .join(matter.to_string())
        .join("work")
        .join(thread.to_string())
}

/// Readies the conversation's work folder `work` before it is made: anything there that is not
/// a real folder (a link, a junction or a file an agent put in its place) is removed itself,
/// never followed, so the launch makes the folder anew instead of failing at every launch of
/// the conversation. A failure is a warning; making the folder then fails the launch.
pub(super) fn clear_work(work: &Path) {
    if std::fs::symlink_metadata(work).is_ok() && !crate::write::is_real_dir(work) {
        tracing::warn!("a conversation's work folder was not a folder; it is made anew");
        if let Err(error) = crate::write::remove_link(work) {
            tracing::warn!(%error, "the entry in a work folder's place stays");
        }
    }
}

/// The name of Claude Code's project folder, which it reads in each added directory.
const CLAUDE_DIR: &str = ".claude";

/// Readies the made work and notes folders, either of which may be an earlier session's: each
/// is set to 0700 whatever it was (on Unix), and loses a `.claude` in it, through no link (a
/// link or file removed itself, a folder with all it holds). Both are Claude's added
/// directories (`agent::loads::profile`), and each launch starts them with no `.claude` in
/// either. The folder, or the error, that failed.
pub(super) fn ready_own<'a>(
    work: &'a Path,
    notes: &'a Path,
) -> Result<(), (&'a Path, std::io::Error)> {
    for folder in [work, notes] {
        ready_one(folder).map_err(|error| (folder, error))?;
    }
    Ok(())
}

/// [`ready_own`] for one folder on Unix, through a handle on it: opened refusing a link (its
/// parent is the host's alone, so the last part is the one checked), then set to 0700 and its
/// `.claude` looked up and removed through that handle, never by the folder's path. A link or
/// file there is unlinked; a folder is first renamed out, beside `folder` where no agent
/// writes, then removed with all it holds. A link or anything else in `folder`'s place fails.
#[cfg(unix)]
fn ready_one(folder: &Path) -> std::io::Result<()> {
    use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let handle = rustix::fs::openat(CWD, folder, flags, Mode::empty())?;
    rustix::fs::fchmod(&handle, Mode::RWXU)?;
    let kind = match rustix::fs::statat(&handle, CLAUDE_DIR, AtFlags::SYMLINK_NOFOLLOW) {
        Err(rustix::io::Errno::NOENT) => return Ok(()),
        found => FileType::from_raw_mode(found?.st_mode),
    };
    if kind != FileType::Directory {
        return Ok(rustix::fs::unlinkat(&handle, CLAUDE_DIR, AtFlags::empty())?);
    }
    let aside = aside_of(folder)?;
    rustix::fs::renameat(&handle, CLAUDE_DIR, CWD, &aside)?;
    crate::write::remove_tree(&aside)
}

/// [`ready_own`] for one folder elsewhere (Windows), by its path.
#[cfg(not(unix))]
fn ready_one(folder: &Path) -> std::io::Result<()> {
    let claude = folder.join(CLAUDE_DIR);
    match std::fs::symlink_metadata(&claude) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(meta) if meta.is_dir() => crate::write::remove_tree(&claude),
        Ok(_) => crate::write::remove_link(&claude),
    }
}

/// A fresh name beside `folder` for its `.claude` on the way out: `.claude-removed-<pid>-<n>`
/// in its parent, which only the host writes. Should the removal fail, a launch's deny rules
/// name it with the parent's other entries.
#[cfg(unix)]
fn aside_of(folder: &Path) -> std::io::Result<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let parent = folder.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "a folder with no parent")
    })?;
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(parent.join(format!(".claude-removed-{}-{n}", std::process::id())))
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
