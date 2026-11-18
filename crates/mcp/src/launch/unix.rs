//! Unix: the mode becomes the server (`exec`), so its pid and the agent's handles are the
//! server's, and the process runs the real program.

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;

use super::Server;

/// Replaces this process with `server`; returns only the error when it could not.
#[expect(
    clippy::disallowed_methods,
    reason = "the launch mode's one start: it becomes the lawyer's MCP server"
)]
pub(super) fn exec(server: &Server) -> io::Error {
    Command::new(&server.program)
        .args(&server.args)
        .envs(server.env.iter().map(|(name, value)| (name, value)))
        .exec()
}
