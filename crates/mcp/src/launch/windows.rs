//! Windows: no `exec`, so the mode joins a Job Object of its own that ends its processes when
//! its last handle closes (`browser_guard/job.rs`), starts the server in it, waits and passes the
//! server's exit code on. The job is joined first, so the server is in it from its creation; a
//! mode the agent kills closes the job's handle and the server ends with it.

use std::io;
use std::os::windows::io::OwnedHandle;
use std::process::Command;
use std::sync::OnceLock;

use super::Server;
use crate::browser_guard::job;

/// The mode's job, open until the process exits: closing its last handle ends every process in
/// it, the mode included, so it must not close before `main` has set the exit code.
static JOB: OnceLock<OwnedHandle> = OnceLock::new();

/// Starts `server` inside the mode's job, waits for it and gives its exit code.
#[expect(
    clippy::disallowed_methods,
    reason = "the launch mode's one start: the lawyer's MCP server, in the mode's job"
)]
pub(super) fn run(server: &Server) -> io::Result<i32> {
    let made = job::join_new()?;
    // The mode runs once, so the lock is empty and takes `made`.
    let _held: &OwnedHandle = JOB.get_or_init(|| made);
    let status = Command::new(&server.program)
        .args(&server.args)
        .envs(server.env.iter().map(|(name, value)| (name, value)))
        .status()?;
    Ok(status.code().unwrap_or(1))
}
