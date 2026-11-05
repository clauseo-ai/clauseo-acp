//! The MCP servers the profile adds. One, `browser`: agent-browser's MCP server behind S27's
//! guard.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::paths::ProfilePaths;
use crate::placeholders::{
    BROWSER_DIR, BROWSER_EXECUTABLE, CDP_PORT, DAEMON_PORT, PROXY_PORT, SESSION_ID, SOCKET_DIR,
    STREAM_PORT,
};

/// The reserved server name: a `mcp.servers.browser` entry in the settings is dropped.
pub const BROWSER: &str = "browser";

/// One stdio MCP server, as the host sends it in `session/new` and `session/load`. Strings
/// may hold [`IN_SERVERS`](crate::placeholders::IN_SERVERS) tokens for the host to fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileServer {
    /// The server's name.
    pub name: String,
    /// The program, an absolute path.
    pub command: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its variables.
    pub env: BTreeMap<String, String>,
}

/// The `browser` entry. The command is `clauseo-mcp`; its `browser-guard` runs agent-browser's
/// `mcp --tools core` with `--cwd` the session's browser state folder (`{browser_dir}`, one
/// token the host fills with a native path) and `--cdp-port {cdp_port}`, plus
/// `--daemon-port {daemon_port}` on Windows. It derives from `--cwd` and the proxy every other
/// folder and proxy variable agent-browser needs (downloads, screenshots, `HOME`, `TMPDIR` or
/// `TEMP`, and the Windows socket folder). The entry carries five variables, six on macOS and
/// Linux with the host's socket folder (`{socket_dir}`). A sidecar path that is no UTF-8 is
/// written lossily: the host's launch would then fail, which is no worse than a missing browser.
pub fn browser(paths: &ProfilePaths) -> ProfileServer {
    browser_for(paths, cfg!(windows))
}

/// [`browser`] for Windows (`windows`) or for macOS and Linux, so both are tested everywhere.
fn browser_for(paths: &ProfilePaths, windows: bool) -> ProfileServer {
    let agent_browser = paths.browser.agent_browser.to_string_lossy().into_owned();
    let mut args = vec![
        "browser-guard".to_owned(),
        "--agent-browser".to_owned(),
        agent_browser,
        "--cwd".to_owned(),
        BROWSER_DIR.to_owned(),
        "--cdp-port".to_owned(),
        CDP_PORT.to_owned(),
    ];
    if windows {
        args.extend(["--daemon-port".to_owned(), DAEMON_PORT.to_owned()]);
    }
    args.extend(["--", "mcp", "--tools", "core"].map(str::to_owned));
    let mut env = BTreeMap::from([
        (
            "AGENT_BROWSER_CONFIG".to_owned(),
            paths.browser_config.to_string_lossy().into_owned(),
        ),
        (
            "AGENT_BROWSER_PROXY".to_owned(),
            format!("http://127.0.0.1:{PROXY_PORT}"),
        ),
        ("AGENT_BROWSER_SESSION".to_owned(), SESSION_ID.to_owned()),
        (
            "AGENT_BROWSER_EXECUTABLE_PATH".to_owned(),
            BROWSER_EXECUTABLE.to_owned(),
        ),
        (
            "AGENT_BROWSER_STREAM_PORT".to_owned(),
            STREAM_PORT.to_owned(),
        ),
    ]);
    if !windows {
        env.insert("AGENT_BROWSER_SOCKET_DIR".to_owned(), SOCKET_DIR.to_owned());
    }
    ProfileServer {
        name: BROWSER.to_owned(),
        command: paths.browser.clauseo_mcp.clone(),
        args,
        env,
    }
}

#[cfg(test)]
mod tests;
