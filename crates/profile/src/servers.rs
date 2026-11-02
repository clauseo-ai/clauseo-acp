//! The MCP servers the profile adds. One, `browser`: agent-browser's MCP server behind S27's
//! guard.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::paths::ProfilePaths;
use crate::placeholders::{BROWSER_DIR, BROWSER_EXECUTABLE, PROXY_PORT, SESSION_ID};

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
/// token the host fills with a native path), and derives from it and
/// the proxy every other folder and proxy variable agent-browser needs (downloads, socket,
/// screenshots, `HOME`, `TMPDIR` or `TEMP`), so the entry carries exactly four variables. A
/// sidecar path that is no UTF-8 is written lossily: the host's launch would then fail, which is
/// no worse than a missing browser.
pub fn browser(paths: &ProfilePaths) -> ProfileServer {
    let agent_browser = paths.browser.agent_browser.to_string_lossy().into_owned();
    ProfileServer {
        name: BROWSER.to_owned(),
        command: paths.browser.clauseo_mcp.clone(),
        args: vec![
            "browser-guard".to_owned(),
            "--agent-browser".to_owned(),
            agent_browser,
            "--cwd".to_owned(),
            BROWSER_DIR.to_owned(),
            "--".to_owned(),
            "mcp".to_owned(),
            "--tools".to_owned(),
            "core".to_owned(),
        ],
        env: BTreeMap::from([
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
        ]),
    }
}
