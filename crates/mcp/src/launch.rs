//! The `launch` mode: `clauseo-mcp launch <file>`, the stdio entry the host gives an agent for
//! each of the lawyer's local MCP servers, so the server's arguments and environment never sit
//! in the agent's session request or on its command line. Claude Agent SDK 0.3.293 starts
//! `claude` with every entry as `--mcp-config <json>` on argv; Codex keeps them in its config.
//!
//! The file is the host's (`actor/turn/profile_loads/mcp_launch.rs` in `clauseo-host`): a
//! private file (0600 on Unix) in the session's run folder,
//! `<data>/run/<session>/mcp/<n>-<name>.json`. It stays for the session, so an agent that
//! restarts the server reads it again. Its shape, every key required and no other allowed:
//!
//! ```json
//! {"v": 1, "command": "/abs/program", "args": ["--stdio"], "env": [["NAME", "value"]]}
//! ```
//!
//! The server gets the mode's own environment (what the agent gives a stdio server) with the
//! file's variables over it, the mode's stdin, stdout, stderr and working folder, and the
//! file's program, an absolute path, with its arguments:
//! - Unix: the mode `exec`s it, so the server is the process the agent started, under the real
//!   program's path (the host's leftovers sweep spares the session's MCP servers by program).
//! - Windows, which has no `exec`: the mode puts itself in a new Job Object that ends what is in
//!   it when its last handle closes, starts the server (std's `Command`, which runs a `.cmd`
//!   such as `npx.cmd` through `cmd.exe` with its own quoting, or refuses), waits for it and
//!   exits with its code. A mode the agent kills takes the server with it.
//!
//! No value from the file reaches stdout, stderr or a log: a file that does not read is named
//! by its line and column only, never by its content.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::Failure;

/// The mode's command line.
pub const USAGE: &str = "usage: clauseo-mcp launch <file>";

/// The exit code for a bad command line or a file that does not read.
const EXIT_USAGE: i32 = 2;
/// The exit code when the server's program could not start.
const EXIT_FAILED: i32 = 1;

/// The only version of the file this build reads.
const VERSION: u32 = 1;

/// The largest file the mode reads. A server's arguments and variables fit well inside.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// What the file says to start.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Launch {
    /// The file's version, [`VERSION`].
    v: u32,
    /// The program, an absolute path.
    command: String,
    /// Its arguments.
    args: Vec<String>,
    /// The variables set over the mode's own, in order.
    env: Vec<(String, String)>,
}

/// What the mode starts: the program, its arguments and the variables over the mode's own.
#[derive(Debug)]
pub(crate) struct Server {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) env: Vec<(OsString, OsString)>,
}

/// The mode, given the arguments after `launch`: reads the file and starts its server. On Unix
/// it returns only when the program could not start.
pub fn run(args: &[OsString]) -> Result<i32, Failure> {
    let [file] = args else {
        return Err(usage(USAGE.to_owned()));
    };
    let server = read(Path::new(file))?;
    start(&server)
}

#[cfg(unix)]
fn start(server: &Server) -> Result<i32, Failure> {
    Err(failed(&unix::exec(server)))
}

#[cfg(windows)]
fn start(server: &Server) -> Result<i32, Failure> {
    windows::run(server).map_err(|error| failed(&error))
}

/// The server `path` names, or why not. No message holds a value from the file.
pub(crate) fn read(path: &Path) -> Result<Server, Failure> {
    let file = File::open(path).map_err(|error| {
        usage(format!(
            "the launch file could not be opened: {}",
            error.kind()
        ))
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            usage(format!(
                "the launch file could not be read: {}",
                error.kind()
            ))
        })?;
    if u64::try_from(bytes.len()).is_ok_and(|len| len > MAX_FILE_BYTES) {
        return Err(usage("the launch file is too large".to_owned()));
    }
    let parsed = serde_json::from_slice::<Launch>(&bytes);
    // The bytes hold the server's secrets: clear them before anything else happens.
    bytes.fill(0);
    let launch = parsed.map_err(|error| {
        // serde_json's message can quote the value it refused, so only its place is given.
        usage(format!(
            "the launch file does not read (line {}, column {})",
            error.line(),
            error.column()
        ))
    })?;
    if launch.v != VERSION {
        return Err(usage(format!("the launch file's version is not {VERSION}")));
    }
    let program = PathBuf::from(launch.command);
    if !program.is_absolute() {
        return Err(usage(
            "the launch file's program is not an absolute path".to_owned(),
        ));
    }
    if launch.env.iter().any(|(name, _)| !name_ok(name)) {
        return Err(usage(
            "the launch file names a variable no environment can hold".to_owned(),
        ));
    }
    Ok(Server {
        program,
        args: launch.args.into_iter().map(OsString::from).collect(),
        env: launch
            .env
            .into_iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect(),
    })
}

/// Whether an environment can hold the variable `name`: not empty, and no `=` or NUL.
fn name_ok(name: &str) -> bool {
    !name.is_empty() && !name.contains(['=', '\0'])
}

fn usage(message: String) -> Failure {
    Failure {
        code: EXIT_USAGE,
        message,
    }
}

fn failed(error: &std::io::Error) -> Failure {
    Failure {
        code: EXIT_FAILED,
        message: format!("the MCP server's program could not start: {}", error.kind()),
    }
}
