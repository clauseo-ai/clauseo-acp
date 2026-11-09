//! [`SessionMark`]: a launch's mark and canary, the two files the session sweep asks the kernel
//! about, and the launch's sentinel that vouches for each scan. The one place the sweep changes
//! files.

use std::ffi::{CStr, CString, OsString};
use std::fs::{DirBuilder, OpenOptions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::seatbelt::{self, set_immutable};
use super::sentinel::Sentinel;
use super::{Checker, ProcId, SweepError, matches, now, now_wall, processes};
use crate::macos::profile::{SESSION_MARK, check_tag};

/// The mark's file name in its folder.
const MARK: &str = "mark";
/// The canary's file name, beside the mark.
const CANARY: &str = "canary";

/// How long a sentinel may take to carry its profile: `sandbox-exec` applies it and execs in
/// place within milliseconds.
const SENTINEL_WAIT: Duration = Duration::from_secs(2);
/// The pause between two looks at a starting sentinel.
const SENTINEL_PAUSE: Duration = Duration::from_millis(5);

/// The profile of the control a mark with no sentinel of its own (one an earlier host left)
/// is scanned with: `/bin/cat` may run and read, and may write the mark's data, nothing else.
const CONTROL_PROFILE: &str = "(version 1)(deny default)\
                               (allow process-exec (literal \"/bin/cat\"))\
                               (allow file-read* file-map-executable)\
                               (allow file-write-data (literal (param \"SESSION_MARK\")))";

/// A launch's session mark: `<dir>/<tag>/mark`, which only this launch's profile lets its
/// processes write (`file-write-data`, nothing else), and `<dir>/<tag>/canary` beside it, which
/// no profile names. Both are regular files of this user, mode 0600, made before the launch: the
/// kernel answers "denied" for a path that does not exist, for every process. The mark is
/// immutable (`UF_IMMUTABLE`) while it is in use, so the one write the profile allows on it
/// writes nothing: the kernel's answer to the scan's question is still "allowed".
///
/// It belongs to one launch (R17: one launcher per launch): its agent ([`SessionMark::claim`],
/// at most one) and its sentinel ([`SessionMark::start_sentinel`]), the process under the
/// launch's own profile that every scan must find matching.
///
/// `dir` must sit outside every folder any profile lets an agent write (the launcher checks it
/// against its policy): the data dir's own folder, which the profile hides.
///
/// When the last holder drops a mark it made (the launcher, the launch's [`JailedSession`] and
/// the process cap's watchdog), the mark removes itself if a scan finds no process of it
/// ([`SessionMark::remove_if_unused`]), then ends its sentinel. Nothing is killed on drop: a
/// mark a process still matches stays, so the gate, session end or a later launcher's clean-up
/// ([`SeatbeltLauncher::new`]) can still find that process.
///
/// [`JailedSession`]: super::JailedSession
/// [`SeatbeltLauncher::new`]: crate::SeatbeltLauncher::new
#[derive(Debug)]
pub struct SessionMark {
    tag: String,
    mark: PathBuf,
    canary: PathBuf,
    mark_c: CString,
    canary_c: CString,
    created: SystemTime,
    /// The launch's agent, once launched.
    root: OnceLock<ProcId>,
    /// The launch's sentinel and what restarts it.
    sentinel: Mutex<Slot>,
    /// Whether a sentinel was ever started: until then no process ran under the profile.
    started: AtomicBool,
    /// Whether session end ended it ([`super::end_session`]).
    ended: AtomicBool,
    /// Whether dropping it removes it when unused: a mark this process made, not one
    /// [`SessionMark::open`] found.
    pub(super) clean_on_drop: bool,
}

impl SessionMark {
    /// Makes the mark of launch `tag` (a launcher's tag: `clauseo-` and then ASCII letters,
    /// digits or `-`) in `dir`, which must be a folder of this user that neither its group nor
    /// others may write: the folder `<dir>/<tag>` (0700, which must not exist yet) and the two
    /// files in it (0600, never through a link), the mark then made immutable. The paths are
    /// canonical, as the kernel's check needs.
    pub fn create(dir: &Path, tag: &str) -> io::Result<SessionMark> {
        check_tag(tag).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        let dir = dunce::canonicalize(dir)?;
        check_folder(&dir)?;
        let folder = dir.join(tag);
        make_folder(&folder)?;
        let mark = folder.join(MARK);
        let canary = folder.join(CANARY);
        make_file(&mark)?;
        make_file(&canary)?;
        let c = |path: &Path| CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other);
        let mark_c = c(&mark)?;
        set_immutable(&mark_c, true)?;
        Ok(SessionMark {
            tag: tag.to_owned(),
            mark_c,
            canary_c: c(&canary)?,
            mark,
            canary,
            created: now_wall(),
            root: OnceLock::new(),
            sentinel: Mutex::new(Slot::default()),
            started: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            clean_on_drop: true,
        })
    }

    /// The mark already in `folder` (canonical; named by a launcher's tag), left by an earlier
    /// launcher: both files must be regular files. Dropping it removes nothing.
    pub(crate) fn open(folder: &Path) -> io::Result<SessionMark> {
        let tag = folder
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("not a tag"))?;
        check_tag(tag).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        if !std::fs::symlink_metadata(folder)?.is_dir() {
            return Err(io::Error::other("not a folder"));
        }
        let mark = folder.join(MARK);
        let canary = folder.join(CANARY);
        let c = |path: &Path| CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other);
        let found = SessionMark {
            tag: tag.to_owned(),
            mark_c: c(&mark)?,
            canary_c: c(&canary)?,
            mark,
            canary,
            created: UNIX_EPOCH,
            root: OnceLock::new(),
            sentinel: Mutex::new(Slot::default()),
            started: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            clean_on_drop: false,
        };
        found.check_files().map_err(io::Error::other)?;
        Ok(found)
    }

    /// Removes the mark if a scan with no start filter finds no process of it, the drift check
    /// passing and a sentinel vouching for it; answers whether it went. The sentinel is the
    /// launch's own while it runs. A mark no process ever ran under (made here, no sentinel
    /// ever started) goes without a scan. Any other is scanned with a control of its own
    /// ([`SessionMark::control_args`]), started and ended for this one scan. A failed scan
    /// removes nothing: a process it could not see may still match. Nothing is killed: an
    /// earlier host's leftovers are ended by the next launcher (`sweep::clean_stale`).
    pub fn remove_if_unused(&self) -> bool {
        let unused = if self.sentinel().is_some() {
            processes(&[self], UNIX_EPOCH).is_ok_and(|found| found.is_empty())
        } else if self.clean_on_drop && !self.started.load(Ordering::Relaxed) {
            true
        } else {
            let vouched =
                seatbelt::api().and_then(|api| self.start_sentinel(api, &self.control_args()));
            let unused =
                vouched.is_ok() && processes(&[self], UNIX_EPOCH).is_ok_and(|f| f.is_empty());
            self.end_sentinel();
            unused
        };
        if !unused {
            return false;
        }
        match self.remove() {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(tag = self.tag, %error, "an unused session mark could not be removed");
                false
            }
        }
    }

    /// The arguments of a control for this mark alone: `sandbox-exec` with a minimal profile
    /// ([`CONTROL_PROFILE`]) whose one write is this mark's data. For a mark with no sentinel
    /// of its own: one an earlier host left.
    pub(crate) fn control_args(&self) -> Vec<OsString> {
        let mut define = OsString::from(format!("{SESSION_MARK}="));
        define.push(&self.mark);
        vec![
            OsString::from("-p"),
            OsString::from(CONTROL_PROFILE),
            OsString::from("-D"),
            define,
        ]
    }

    /// Starts the launch's sentinel, `sandbox-exec <args> /bin/cat` (`args`: the launch's
    /// profile and its parameters), and waits until `check` finds it matching the mark: its
    /// pid and start. [`SweepError::Unknown`] when it does not match within 2 s (the kernel's
    /// answers are not what the sweep needs: no launch then), or it could not start. A sentinel
    /// that dies later is restarted with the same `args` ([`SessionMark::revive`]).
    pub(crate) fn start_sentinel(
        &self,
        check: &dyn Checker,
        args: &[OsString],
    ) -> Result<ProcId, SweepError> {
        self.started.store(true, Ordering::Relaxed);
        let sentinel = self.spawn_matching(check, args)?;
        let id = sentinel.id();
        let mut slot = self.slot();
        slot.current = Some(sentinel);
        slot.args = args.to_vec();
        slot.retired = false;
        Ok(id)
    }

    /// Starts `sandbox-exec <args> /bin/cat` and waits until `check` finds it matching the mark.
    fn spawn_matching(
        &self,
        check: &dyn Checker,
        args: &[OsString],
    ) -> Result<Sentinel, SweepError> {
        let sentinel = Sentinel::start(args).map_err(|source| SweepError::Sentinel { source })?;
        let id = sentinel.id();
        let deadline = now() + SENTINEL_WAIT;
        loop {
            if check.sandboxed(id.pid()) == Ok(true) && matches(check, id.pid(), self) == Ok(true) {
                return Ok(sentinel);
            }
            if now() >= deadline || !sentinel.running() {
                return Err(SweepError::Unknown {
                    tag: self.tag.clone(),
                });
            }
            thread::park_timeout(SENTINEL_PAUSE);
        }
    }

    /// Restarts the sentinel when one was started, has not been ended
    /// ([`SessionMark::end_sentinel`], session end) and no longer runs: a fresh process under the
    /// same profile is as good a control as the first. Each restart is logged (`kind`
    /// `sentinel_restarted`), and so is a restart that fails (`sentinel_restart_failed`; the
    /// scans stay unknown). Answers whether it started one.
    pub(crate) fn revive(&self, check: &dyn Checker) -> bool {
        let mut slot = self.slot();
        if slot.retired || slot.args.is_empty() || self.ended() {
            return false;
        }
        if slot.current.as_ref().is_some_and(Sentinel::running) {
            return false;
        }
        let dead = slot.current.take().map(|s| s.id());
        // Reaped here: its pid named it alone until now.
        let args = slot.args.clone();
        match self.spawn_matching(check, &args) {
            Ok(sentinel) => {
                tracing::warn!(
                    kind = "sentinel_restarted",
                    tag = self.tag,
                    dead = dead.map(|id| id.pid()),
                    sentinel = sentinel.id().pid(),
                    "the session sweep's sentinel was dead and is restarted"
                );
                slot.current = Some(sentinel);
                true
            }
            Err(error) => {
                tracing::error!(
                    kind = "sentinel_restart_failed",
                    tag = self.tag,
                    %error,
                    "the session sweep's sentinel was dead and could not be restarted"
                );
                false
            }
        }
    }

    /// A unit test's stand-in sentinel: the process `id`, never signalled.
    #[cfg(test)]
    pub(super) fn stand_in_sentinel(&self, id: ProcId) {
        self.started.store(true, Ordering::Relaxed);
        let mut slot = self.slot();
        slot.current = Some(Sentinel::stand_in(id));
        slot.retired = false;
    }

    /// The sentinel's pid and start, if one was started and not ended (it may have died since).
    pub fn sentinel(&self) -> Option<ProcId> {
        self.slot().current.as_ref().map(Sentinel::id)
    }

    /// Whether a sentinel was started, not ended, and no longer runs: it was killed.
    pub(crate) fn sentinel_dead(&self) -> bool {
        self.slot()
            .current
            .as_ref()
            .is_some_and(|sentinel| !sentinel.running())
    }

    /// Kills and reaps the sentinel, if any, for good: it is not restarted, and every later
    /// scan of the mark is unknown ([`SweepError::Unknown`]), never "nothing runs". Session end
    /// calls it after [`Gone`](super::Gone); otherwise only a test should.
    pub fn end_sentinel(&self) {
        let sentinel = {
            let mut slot = self.slot();
            slot.retired = true;
            slot.current.take()
        };
        drop(sentinel);
    }

    /// The sentinel's slot, locked.
    fn slot(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.sentinel.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Session end's last step, after [`Gone`](super::Gone): marks it ended (the watchdog
    /// stops), ends the sentinel and removes the files.
    pub(crate) fn finish(&self) -> io::Result<()> {
        self.ended.store(true, Ordering::Relaxed);
        self.end_sentinel();
        self.remove()
    }

    /// Whether session end ended it.
    pub(crate) fn ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }

    /// The launch's tag.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The mark's path.
    pub fn path(&self) -> &Path {
        &self.mark
    }

    /// The canary's path.
    pub fn canary(&self) -> &Path {
        &self.canary
    }

    /// When the mark was made: no process of its launch started earlier.
    pub fn created(&self) -> SystemTime {
        self.created
    }

    /// The profile parameter naming the mark: `SESSION_MARK` and its path.
    pub fn param(&self) -> (String, PathBuf) {
        (SESSION_MARK.to_owned(), self.mark.clone())
    }

    /// Removes the two files and their folder, the mark's immutable flag cleared first. Only
    /// after an end answered [`Gone`](super::Gone): without the mark no scan can find the
    /// session's processes.
    pub fn remove(&self) -> io::Result<()> {
        match set_immutable(&self.mark_c, false) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            Ok(()) | Err(_) => {}
        }
        remove(&self.canary, &self.mark)
    }

    /// The launch's agent, if it was launched.
    pub(crate) fn root(&self) -> Option<ProcId> {
        self.root.get().copied()
    }

    /// Records the launch's agent; `false` when the mark has one already (a second launch,
    /// which R17 rules out and the launcher refuses).
    pub(crate) fn claim(&self, root: ProcId) -> bool {
        self.root.set(root).is_ok()
    }

    /// The mark's path as the kernel takes it.
    pub(crate) fn mark_c(&self) -> &CStr {
        &self.mark_c
    }

    /// The canary's path as the kernel takes it.
    pub(crate) fn canary_c(&self) -> &CStr {
        &self.canary_c
    }

    /// Both files are still regular files (`lstat`), else [`SweepError::Mark`].
    pub(crate) fn check_files(&self) -> Result<(), SweepError> {
        for (file, path) in [(MARK, &self.mark), (CANARY, &self.canary)] {
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.is_file() => {}
                Ok(_) => {
                    return Err(SweepError::Mark {
                        file,
                        kind: io::ErrorKind::InvalidData,
                    });
                }
                Err(error) => {
                    return Err(SweepError::Mark {
                        file,
                        kind: error.kind(),
                    });
                }
            }
        }
        Ok(())
    }
}

/// A mark's sentinel: the one running, if any, the arguments that restart it, and whether it was
/// ended for good.
#[derive(Debug, Default)]
struct Slot {
    current: Option<Sentinel>,
    /// The `sandbox-exec` arguments it was started with; empty until one was.
    args: Vec<OsString>,
    /// Ended by [`SessionMark::end_sentinel`]: never restarted.
    retired: bool,
}

impl Drop for SessionMark {
    /// A mark this process made, still on disk, goes when no process matches it; its sentinel
    /// goes in any case.
    fn drop(&mut self) {
        if self.clean_on_drop && self.check_files().is_ok() {
            self.remove_if_unused();
        }
        self.end_sentinel();
    }
}

/// Refuses `dir` (canonical) as the marks folder unless it is a folder (not a link) of this
/// user that neither its group nor others may write: anyone else who could write it could
/// make or swap a launch's folder in it.
pub(crate) fn check_folder(dir: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(dir)?;
    let refuse = |why: &str| io::Error::other(format!("{} {why}", dir.display()));
    if !meta.is_dir() {
        return Err(refuse("is not a folder"));
    }
    if meta.uid() != seatbelt::uid() {
        return Err(refuse("belongs to another user"));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(refuse("may be written by its group or by others"));
    }
    Ok(())
}

/// Makes `folder` at 0700. One that exists already is refused: every tag carries a random
/// nonce, so only someone else's doing can have made it.
#[expect(
    clippy::disallowed_methods,
    reason = "the session mark's folder, made by the host outside every jail"
)]
fn make_folder(folder: &Path) -> io::Result<()> {
    DirBuilder::new().mode(0o700).create(folder)
}

/// Makes `path` a new regular file at 0600, never through a link (`O_CREAT | O_EXCL`), and
/// checks what it opened is a regular file.
#[expect(
    clippy::disallowed_methods,
    reason = "the session mark and canary, made by the host outside every jail"
)]
fn make_file(path: &Path) -> io::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if file.metadata()?.is_file() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{} is not a regular file",
            path.display()
        )))
    }
}

/// Removes the canary, the mark and their folder; a file already gone is fine.
#[expect(
    clippy::disallowed_methods,
    reason = "the session mark's removal after its session is gone"
)]
fn remove(canary: &Path, mark: &Path) -> io::Result<()> {
    for path in [canary, mark] {
        match std::fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            Ok(()) | Err(_) => {}
        }
    }
    match mark.parent().map(std::fs::remove_dir) {
        Some(Err(error)) if error.kind() != io::ErrorKind::NotFound => Err(error),
        Some(Ok(()) | Err(_)) | None => Ok(()),
    }
}
