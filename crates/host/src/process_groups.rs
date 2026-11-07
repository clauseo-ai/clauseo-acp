//! The agents' process groups a crashed desktop left running (decision record 0051). When the
//! desktop is killed, its shutdown ladder never runs: the agent usually ends on its stdin's end,
//! but what it started in its group lives on, and a jailed survivor keeps its profile's grant to
//! that session's proxy and relay ports, which a later session may bind again.
//!
//! So each agent launch records its group (Unix): `<data>/process-groups/<matter>-<thread>-
//! <launch>-<host pid>.json`, `{"v":2,"pgid":…,"leader_start":…,"sid":…,"boot":…}`, written
//! atomically (0600) on the host thread in the step that started the process, whether the launch
//! then succeeded or failed ([`Recorder::write`]), and removed once the session's removal has
//! seen the tree gone ([`remove`]). Not in the run folder: a jailed agent can write there, and a
//! forged record could name another of the lawyer's groups. No jail profile grants this folder,
//! and Claude's data-folder fence denies it.
//!
//! At the next start, once the store's lock is held and before the actor serves (so before any
//! launch claims a port), [`start`] ends each recorded group that is provably the same group,
//! then deletes the record; anything it cannot prove is left alone:
//!
//! - another boot: nothing (pids start again at each boot);
//! - a process holds the group's id (`kill(pgid, 0)` is not `ESRCH`; a zombie or another
//!   user's counts): the group is ended only when that process's start (a zombie's too, on
//!   Linux) is the recorded leader's. Another start means the id was given again; an unreadable
//!   one is unprovable (another user's process, and on macOS a zombie leader, which
//!   `PROC_PIDTBSDINFO` cannot read). Both are left alone;
//! - no process holds it, but members of the group run: ended only when every member is in the
//!   recorded session, that session is not the group's own id, and no member started before the
//!   recorded leader. The agent is started with `process_group(0)`, never `setsid`, so its tree
//!   stays in the host's session; a group that took the id later (a `setsid` or double-fork
//!   daemon whose leader then exited) is in another session or has older members. Otherwise, or
//!   when the listing fails or may be cut short, the whole group is left. A later group in the
//!   host's own session is not told apart (see the residuals);
//! - neither: nothing to do.
//!
//! Each kill is SIGKILL, then up to 500 ms for the group to empty; the sweep takes at most 2 s on
//! the host's clock and leaves the records it did not reach for the next start. A record that is
//! a link, has another link (`nlink > 1`), is not this user's, is of another version (no `sid`)
//! or does not parse is deleted unread and counted. The log holds counts only, never a pid, path
//! or command line.
//!
//! Residuals: a group that took the id later in the host's own session passes the session test: a
//! shell job when desktop was started from that terminal, or on Linux another app of the same
//! graphical session that made a group and whose leader exited, with every member started after the
//! recorded leader; a descendant that left the group (`setsid`, `setpgid`) is the jail's to
//! contain; a launch whose record write failed, or whose spawn-time start was not read, is not
//! swept; between the leader's start check and `killpg` the group could empty and its id be taken
//! again (a pid wrap within microseconds, inherent to `killpg`); and a restart in another pid
//! namespace (Flatpak, a container) or with a `hidepid` `/proc` sees other pids or unreadable ones:
//! those records end nothing, as their leaders and members read as gone, reused or unsure.
//!
//! Windows records nothing: the agent's Job Object (`KILL_ON_JOB_CLOSE`) ends its tree when the
//! desktop's process dies.

use std::path::{Path, PathBuf};

use crate::config::SessionKey;

#[cfg(unix)]
mod sweep;

/// The records' folder under the data folder.
const FOLDER: &str = "process-groups";

/// The record of `key`'s launch under `data`. A path only.
pub(crate) fn record_path(data: &Path, key: SessionKey) -> PathBuf {
    // The host's pid too: launch ids start again at 1 in each run, and a record an earlier run
    // left (not reached by its sweep) must not be replaced.
    data.join(FOLDER).join(format!(
        "{}-{}-{}-{}.json",
        key.matter,
        key.thread,
        key.launch,
        std::process::id()
    ))
}

/// Writes launches' records: knows this boot's stamp, read once at start. With none (Windows,
/// or a boot that could not be read), it writes nothing.
#[derive(Debug, Clone, Default)]
pub(crate) struct Recorder {
    #[cfg(unix)]
    boot: Option<String>,
}

/// The start-up sweep on `data` (Unix; Windows does nothing), then the records' folder made
/// (0700) for this run's launches. Blocking, at most about 2 s on `clock`: run on the starting
/// thread before the actor serves.
pub(crate) fn start(data: &Path, clock: &dyn crate::clock::Clock) -> Recorder {
    #[cfg(unix)]
    {
        let folder = data.join(FOLDER);
        let boot = sweep::sweep(&folder, clock);
        if let Err(error) = crate::write::create_private_dirs_under(data, &folder) {
            tracing::warn!(
                kind = ?error.kind(),
                "the process-group records folder was not made; launches go on unrecorded"
            );
        }
        Recorder { boot }
    }
    #[cfg(not(unix))]
    {
        let _ = (data, clock);
        Recorder::default()
    }
}

impl Recorder {
    /// Records the group of the agent `tree` watches (it leads its own, `process_group(0)`) at
    /// `path`, the launch's [`record_path`]. One small file, not synced (a reboot voids it anyway):
    /// cheap enough for the host thread. A failure logs its kind and the launch goes on: the record
    /// is a backstop, not a gate.
    pub(crate) fn write(&self, path: &Path, tree: &clauseo_process::TreeWatch) {
        #[cfg(unix)]
        {
            let Some(boot) = &self.boot else {
                return;
            };
            let pid = tree.group();
            // The start the spawn read, so a leader that has already exited (a failed launch)
            // is recorded too. Its session is the host's own when it can no longer be read:
            // `process_group(0)` keeps the agent in the session it was started from.
            let start = tree
                .leader_start()
                .or_else(|| clauseo_process::start_stamp(pid));
            let sid = clauseo_process::session_id(pid)
                .or_else(|| clauseo_process::session_id(std::process::id()));
            let (Ok(pgid), Some(start), Some(sid)) = (i32::try_from(pid), start, sid) else {
                tracing::warn!(
                    "the agent's start or session was not read; its group is not recorded"
                );
                return;
            };
            let record = serde_json::json!({
                "v": sweep::VERSION,
                "pgid": pgid,
                "leader_start": start.encode(),
                "sid": sid,
                "boot": boot,
            });
            if let Err(error) =
                crate::write::replace_private_file_unsynced(path, record.to_string().as_bytes())
            {
                tracing::warn!(
                    kind = ?error.kind(),
                    "the agent's process group was not recorded; the launch goes on"
                );
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (self, path, tree);
        }
    }
}

/// Removes a launch's record once its tree is proven gone (the removal's completed wait). A
/// missing record (Windows, a failed write, no agent started) is fine. Off the host thread.
pub(crate) fn remove(path: &Path) {
    #[cfg(unix)]
    match crate::write::remove_link(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(
            kind = ?error.kind(),
            "a session's process-group record was not removed; the next start checks it"
        ),
    }
    #[cfg(not(unix))]
    let _ = path;
}
