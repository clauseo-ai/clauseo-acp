//! The MCP servers the profile adds: `browser`, agent-browser's MCP server behind S27's guard
//! (Codex and Claude, [`browser_mcp`]), then `case-law`, the paid case-law
//! connector through the host's relay (R24).

use std::collections::BTreeMap;
use std::path::PathBuf;

use clauseo_protocol::AgentKind;

use crate::paths::ProfilePaths;
use crate::placeholders::{
    BROWSER_DIR, BROWSER_EXECUTABLE, CDP_PORT, DAEMON_PORT, MATTER_DOWNLOADS,
    MATTER_FALLBACK_DOWNLOADS, PROXY_PORT, PROXY_SECRET_FILE, RELAY_PORT, RELAY_TOKEN_FILE,
    SESSION_ID, SOCKET_DIR,
};

/// The reserved server name: a `mcp.servers.browser` entry in the settings is dropped.
pub const BROWSER: &str = "browser";

/// Whether an agent of `kind` gets the `browser` MCP entry ([`browser`]): Codex and Claude,
/// on every OS.
///
/// In a Codex turn the shell `agent-browser` cannot work: an approved command still runs under
/// Seatbelt, because Clauseo's permission profile has deny entries, with no network, and
/// Chrome's own sandbox cannot nest in it. Codex starts a local stdio MCP server as a plain
/// child instead, with no Seatbelt and no permission profile (codex 0.162.1
/// `codex-mcp/src/rmcp_client.rs:1216-1238`, `rmcp-client/src/stdio_server_launcher.rs:262-288`),
/// so the guard, which refuses every tool off agent-browser's `core` profile and every argument
/// naming a file, is what holds the browser to its own folder there. Claude Code starts a stdio
/// MCP server itself too, outside the sandbox it puts Bash commands in, so the same guard holds
/// Claude's browser; its sandboxed shell is refused loopback and the daemon's socket folder (the
/// host's `agent::loads::sandbox`), and a Claude session whose sandbox falls back off gets no
/// entry (the host's `profile_loads::claude`).
///
/// On every OS, Windows included ([`no_browser`]). Claude Code has no sandbox on Windows, so
/// there Claude's shell is not refused loopback; the patched daemon serves no client without the
/// guard's per-session key, and Chrome speaks CDP over a pipe, as for Codex.
pub fn browser_mcp(kind: Option<AgentKind>) -> bool {
    browser_mcp_on(kind, cfg!(windows))
}

/// [`browser_mcp`] as it is on Windows when `windows` is true, and elsewhere when it is false.
pub fn browser_mcp_on(kind: Option<AgentKind>, _windows: bool) -> bool {
    matches!(kind, Some(AgentKind::Codex | AgentKind::Claude))
}

/// Whether an agent of `kind` gets no browser at all, neither the entry nor the shell command,
/// for launch (decision record 0030):
///
/// - Not Codex, on any OS. On Windows Codex was off because its shell's unelevated
///   restricted-token sandbox (`windows.sandbox = "unelevated"`) leaves loopback open (codex
///   0.162.1 `windows-sandbox-rs/src/env.rs:126-170`), and agent-browser's daemon was
///   unauthenticated, so a shell command Codex ran without a prompt could drive the guard's
///   browser past its filter. It is on now: the guard draws a per-session key (#1091,
///   `clauseo_mcp` `browser_guard/env/session_key.rs`) that only its own agent-browser gets,
///   the patched daemon serves no client without it, and Chrome speaks CDP over a pipe, not a
///   port, on Windows.
/// - Not Claude, on any OS: it has the guarded entry on Windows too, for the same key and pipe.
///   Its shell command stays off there ([`shell_browser`]): Claude Code has no sandbox on
///   Windows, so the command ran on deny rules alone, and no deny list is complete: brace
///   expansion (`{upload,}`), globs (`upl?ad`), a variable (`S=eval; agent-browser $S`),
///   `${IFS}`, `cmd //c`, `xargs`, `npx`, `.exe` and letter case each spell a refused command
///   no rule sees (security review round 4 of #1014). The way back is Clauseo's own
///   `agent-browser` shim, after launch.
///
/// So both agents have a browser, through the guarded entry only, on every OS; this is false
/// for every agent. No agent gets the shell command anywhere ([`shell_browser`]).
pub fn no_browser(kind: Option<AgentKind>) -> bool {
    no_browser_on(kind, cfg!(windows))
}

/// [`no_browser`] as it is on Windows when `windows` is true, and elsewhere when it is false.
pub fn no_browser_on(_kind: Option<AgentKind>, _windows: bool) -> bool {
    false
}

/// Whether any agent gets the `agent-browser` shell command on this OS: none, on any OS, for
/// launch (both agents have the guarded `browser` entry instead, [`browser_mcp`]). On macOS
/// and Linux the command would have to run outside Claude Code's sandbox (`excludedCommands`),
/// and Claude Code's exclusion check strips `env` and reworks `VAR=` prefixes before it
/// matches, so `PATH=<a folder Claude writes> agent-browser …` would run Claude's own program
/// unsandboxed; quoting (`-q "upload"`, `e"v"al`, a tab) would
/// run it unsandboxed past the ask rules too. The app copies the command into `<data>/tools`,
/// and renders the browser's skill, only when it is true, so it copies nothing now.
pub fn shell_browser() -> bool {
    shell_browser_on(cfg!(windows))
}

/// [`shell_browser`] as it is on Windows when `windows` is true, and elsewhere when it is false.
pub fn shell_browser_on(_windows: bool) -> bool {
    false
}

/// The case-law connector's reserved server name (R24): a `mcp.servers.case-law` entry in the
/// settings or a policy source is dropped (`clauseo_settings::RESERVED_MCP_SERVERS`).
pub const CASE_LAW: &str = "case-law";

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
/// token the host fills with a native path), `--downloads` the matter's downloads folder
/// (`{matter_downloads}`, where the files the browser saves are delivered),
/// `--fallback-downloads` Clauseo's own downloads folder for the matter
/// (`{matter_fallback_downloads}`, where a file goes when the matter's drive cannot keep the
/// "downloaded from the internet" mark) and
/// `--cdp-port {cdp_port}`, plus `--daemon-port {daemon_port}` on Windows. It derives from
/// `--cwd` every other folder agent-browser needs (screenshots, its run profile, Chrome's
/// download staging, `HOME`, `TMPDIR` or `TEMP`, and the Windows socket folder), and fixes the
/// idle timeout. The entry carries five variables, six on macOS and Linux with the host's
/// socket folder (`{socket_dir}`). Two are the session's egress proxy (ADR 0053): the
/// browser listener's address, `AGENT_BROWSER_PROXY=http://127.0.0.1:{proxy_port}`, which
/// makes the host open the session a proxy in open mode (any public host, minus the unsafe-site
/// block list and every private address), and the file holding its password,
/// `CLAUSEO_PROXY_SECRET_FILE={proxy_secret_file}`, which the guard reads once and deletes, and
/// whose password it gives Chrome over CDP and to no child. The password itself is in no
/// argument and no variable: the Claude Agent SDK puts an entry's variables on a command line.
/// A sidecar path that is no UTF-8 is written lossily: the host's launch would then fail, which
/// is no worse than a missing browser.
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
        "--downloads".to_owned(),
        MATTER_DOWNLOADS.to_owned(),
        "--fallback-downloads".to_owned(),
        MATTER_FALLBACK_DOWNLOADS.to_owned(),
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
        ("AGENT_BROWSER_SESSION".to_owned(), SESSION_ID.to_owned()),
        (
            "AGENT_BROWSER_EXECUTABLE_PATH".to_owned(),
            BROWSER_EXECUTABLE.to_owned(),
        ),
        (
            "AGENT_BROWSER_PROXY".to_owned(),
            format!("http://127.0.0.1:{PROXY_PORT}"),
        ),
        (
            "CLAUSEO_PROXY_SECRET_FILE".to_owned(),
            PROXY_SECRET_FILE.to_owned(),
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

/// The `case-law` entry (R24): `clauseo-mcp connector case-law`, which relays each JSON-RPC
/// line to the session's relay in the host. Its two variables are the relay's address,
/// `CLAUSEO_RELAY=127.0.0.1:{relay_port}`, and the path of the token file,
/// `CLAUSEO_RELAY_TOKEN_FILE={relay_token_file}`; the token itself is never in the entry. The
/// host fills both per session, or drops the entry.
pub fn case_law(paths: &ProfilePaths) -> ProfileServer {
    ProfileServer {
        name: CASE_LAW.to_owned(),
        command: paths.browser.clauseo_mcp.clone(),
        args: vec!["connector".to_owned(), CASE_LAW.to_owned()],
        env: BTreeMap::from([
            (
                "CLAUSEO_RELAY".to_owned(),
                format!("127.0.0.1:{RELAY_PORT}"),
            ),
            (
                "CLAUSEO_RELAY_TOKEN_FILE".to_owned(),
                RELAY_TOKEN_FILE.to_owned(),
            ),
        ]),
    }
}

#[cfg(test)]
mod tests;
