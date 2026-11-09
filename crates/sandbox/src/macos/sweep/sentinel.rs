//! [`Sentinel`]: the scan's positive control. A tiny process under a launch's own profile,
//! `sandbox-exec <the profile> /bin/cat`, reading a pipe only the host holds, for the session's
//! life. Every scan of the mark must find it matching, or the scan's answer is unknown
//! (`SweepError::Unknown`), never "nothing runs": that is what makes the private call fail
//! closed once no agent is left to check it against (0038, "The session sweep").
//!
//! It dies with the host: when the host exits, crashed or not, the pipe's last writer goes and
//! `cat` reads its end. It is started as the launches are (#827's descriptor hygiene): under
//! `clauseo_process::spawn_locked`, so no pipe of another start is half made when it forks, and
//! with every other descriptor closed before it runs, so it holds no other launch's pipe open
//! and no other start can hold its own. It is never counted, stopped or killed by a scan's
//! callers: the scan leaves it out of what it finds.

use std::ffi::OsString;
use std::io;
use std::process::{Child, ChildStdin, Command, Stdio};

use super::{ProcId, start_of};
use crate::macos::launcher::SANDBOX_EXEC;

/// What the sentinel runs once its profile is applied: it reads its standard input, a pipe the
/// host holds, and writes nothing.
const PROGRAM: &str = "/bin/cat";

/// A running sentinel; dropping it kills it (by its unreaped pid, so never another process)
/// and reaps it.
#[derive(Debug)]
pub(crate) struct Sentinel {
    id: ProcId,
    /// The child and the pipe it reads; `None` for a unit test's stand-in.
    child: Option<(Child, ChildStdin)>,
}

impl Sentinel {
    /// Starts `sandbox-exec <args> /bin/cat` with no variables, in `/`, its standard input a
    /// pipe this process holds, its output discarded and no other descriptor open, under the
    /// lock every launch holds while it starts.
    #[expect(
        clippy::disallowed_methods,
        reason = "the sentinel is the host's own child, outside every launch's tree and cap"
    )]
    pub(super) fn start(args: &[OsString]) -> io::Result<Sentinel> {
        let mut command = Command::new(SANDBOX_EXEC);
        command
            .args(args)
            .arg(PROGRAM)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        clauseo_process::close_other_fds(&mut command);
        let mut child = clauseo_process::spawn_locked(|| command.spawn())?;
        let stdin = child.stdin.take();
        let pid = child.id();
        // An unreaped child keeps its pid and start; a sentinel that is gone already is useless.
        let (Some(stdin), Some(start)) = (stdin, start_of(pid)) else {
            end(&mut child);
            return Err(io::Error::other("the sentinel exited at once"));
        };
        Ok(Sentinel {
            id: ProcId::new(pid, start),
            child: Some((child, stdin)),
        })
    }

    /// A stand-in for a unit test: the process `id`, which this never signals.
    #[cfg(test)]
    pub(super) fn stand_in(id: ProcId) -> Sentinel {
        Sentinel { id, child: None }
    }

    /// Its pid and start.
    pub(super) fn id(&self) -> ProcId {
        self.id
    }

    /// Whether it still runs with its start.
    pub(super) fn running(&self) -> bool {
        start_of(self.id.pid()) == Some(self.id.start)
    }
}

impl Drop for Sentinel {
    fn drop(&mut self) {
        if let Some((mut child, stdin)) = self.child.take() {
            drop(stdin);
            end(&mut child);
        }
    }
}

/// Kills `child` and reaps it: until it is reaped its pid names it alone.
fn end(child: &mut Child) {
    if let Err(error) = child.kill().and_then(|()| child.wait().map(drop)) {
        tracing::warn!(%error, "the session sweep's sentinel could not be ended");
    }
}
