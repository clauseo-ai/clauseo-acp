//! The agents' process groups a crashed desktop left running (decision record 0051). When the
//! desktop is killed, its shutdown ladder never runs: the agent usually ends on its stdin's end,
//! but what it started in its group lives on, and a jailed survivor keeps its profile's grant to
//! that session's proxy and relay ports, which a later session may bind again.
//!
//! So each agent launch records its group (Unix): `<data>/process-groups/<matter>-<thread>-
//! <launch>.json`, `{"v":1,"pgid":…,"leader_start":…,"boot":…}`, written atomically (0600) on
//! the host thread right after the child exists ([`Recorder::write`]), and removed once the
//! session's removal has seen the tree gone ([`remove`]). Not in the run folder: a jailed agent
//! can write there, and a forged record could name another of the lawyer's groups. No jail
//! profile grants this folder, and Claude's data-folder fence denies it.
//!
//! At the next start, before the actor serves (so before any launch claims a port), [`start`]
//! ends each recorded group that is provably the same group, then deletes the record:
//!
//! - another boot: nothing (pids start again at each boot);
//! - a process whose pid is the group's id: ended only when its start is the recorded leader's;
//!   any other start means the id was free and given again, so the old group is gone;
//! - no such process, but members of the group: a group id is not given again while a member
//!   lives, so the group is the original, and it is ended. A member that started before the
//!   recorded leader cannot be its descendant: then the group is not signalled as a whole, only
//!   its later members one by one, and the earlier ones are counted in the log;
//! - neither: nothing to do.
//!
//! Each kill is SIGKILL, then up to 500 ms for the group to empty; the sweep waits at most 2 s in
//! all and leaves the records it did not reach for the next start. A record that is a link, has
//! another link (`nlink > 1`), is not this user's, or does not parse is deleted unread and
//! counted. The log holds counts only, never a pid, path or command line.
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
    data.join(FOLDER)
        .join(format!("{}-{}-{}.json", key.matter, key.thread, key.launch))
}

/// Writes launches' records: knows this boot's stamp, read once at start. With none (Windows,
/// or a boot that could not be read), it writes nothing.
#[derive(Debug, Clone, Default)]
pub(crate) struct Recorder {
    #[cfg(unix)]
    boot: Option<String>,
}

/// The start-up sweep on `data` (Unix; Windows does nothing), then the records' folder made
/// (0700) for this run's launches. Blocking, at most about 2 s of waits: run on the starting
/// thread before the actor serves.
pub(crate) fn start(data: &Path) -> Recorder {
    #[cfg(unix)]
    {
        let folder = data.join(FOLDER);
        let boot = sweep::sweep(&folder);
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
        let _ = data;
        Recorder::default()
    }
}

impl Recorder {
    /// Records agent `pid`'s group (it leads its own, `process_group(0)`) at `path`, the
    /// launch's [`record_path`]. One small file, not synced (a reboot voids it anyway): cheap
    /// enough for the host thread. A failure logs its kind and the launch goes on: the record is
    /// a backstop, not a gate.
    pub(crate) fn write(&self, path: &Path, pid: u32) {
        #[cfg(unix)]
        {
            let Some(boot) = &self.boot else {
                return;
            };
            let (Ok(pgid), Some(start)) = (i32::try_from(pid), clauseo_process::start_stamp(pid))
            else {
                tracing::warn!("the agent's start was not read; its group is not recorded");
                return;
            };
            let record = serde_json::json!({
                "v": 1,
                "pgid": pgid,
                "leader_start": start.encode(),
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
            let _ = (self, path, pid);
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
