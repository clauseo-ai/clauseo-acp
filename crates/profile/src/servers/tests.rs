use std::path::{Path, PathBuf};

use clauseo_settings::Locations;

use super::{CASE_LAW, browser_for, browser_mcp_on, case_law, no_browser_on, shell_browser_on};
use crate::{BrowserSidecars, ProfilePaths};

fn paths() -> ProfilePaths {
    let abs = |path: &str| {
        if cfg!(windows) {
            PathBuf::from(format!("C:{path}"))
        } else {
            PathBuf::from(path)
        }
    };
    ProfilePaths::new(
        &Locations::under_root(Path::new("/data")),
        abs("/app/clauseo"),
        abs("/app/stetdoc"),
        BrowserSidecars {
            clauseo_mcp: abs("/app/clauseo-mcp"),
            agent_browser: abs("/app/agent-browser"),
        },
    )
    .unwrap()
}

/// The guard's ports: `--cdp-port` on both, `--daemon-port` on Windows only, and the stream
/// port; the socket folder on macOS and Linux only.
#[test]
fn each_os_gets_its_own_port_and_socket_tokens() {
    let unix = browser_for(&paths(), false);
    let windows = browser_for(&paths(), true);
    let after_cwd = |args: &[String]| args[4..].to_vec();
    assert_eq!(
        after_cwd(&unix.args),
        [
            "{browser_dir}",
            "--downloads",
            "{matter_downloads}",
            "--fallback-downloads",
            "{matter_fallback_downloads}",
            "--cdp-port",
            "{cdp_port}",
            "--",
            "mcp",
            "--tools",
            "core"
        ]
    );
    assert_eq!(
        after_cwd(&windows.args),
        [
            "{browser_dir}",
            "--downloads",
            "{matter_downloads}",
            "--fallback-downloads",
            "{matter_fallback_downloads}",
            "--cdp-port",
            "{cdp_port}",
            "--daemon-port",
            "{daemon_port}",
            "--",
            "mcp",
            "--tools",
            "core"
        ]
    );
    // No stream port: the guard turns agent-browser's stream server off itself.
    assert!(!unix.env.contains_key("AGENT_BROWSER_STREAM_PORT"));
    assert!(!windows.env.contains_key("AGENT_BROWSER_STREAM_PORT"));
    assert_eq!(unix.env["AGENT_BROWSER_SOCKET_DIR"], "{socket_dir}");
    assert!(!windows.env.contains_key("AGENT_BROWSER_SOCKET_DIR"));
    // The session's egress proxy (ADR 0053): its address and its password's file, both
    // variables; neither is ever an argument.
    for entry in [&unix, &windows] {
        assert_eq!(
            entry.env["AGENT_BROWSER_PROXY"],
            "http://127.0.0.1:{proxy_port}"
        );
        assert_eq!(
            entry.env["CLAUSEO_PROXY_SECRET_FILE"],
            "{proxy_secret_file}"
        );
        assert!(
            entry
                .args
                .iter()
                .all(|arg| !arg.contains("{proxy_port}") && !arg.contains("{proxy_secret_file}"))
        );
    }
    assert_eq!(unix.env.len(), 6);
    assert_eq!(windows.env.len(), 5);
}

/// The `case-law` entry: `clauseo-mcp connector case-law` with the relay's address and the
/// token file's path as tokens, and no token anywhere in it.
#[test]
fn the_case_law_entry_names_the_relay_and_the_token_file_only() {
    let entry = case_law(&paths());
    assert_eq!(entry.name, CASE_LAW);
    assert_eq!(entry.command, paths().browser.clauseo_mcp);
    assert_eq!(entry.args, ["connector", "case-law"]);
    let env: Vec<(&str, &str)> = entry
        .env
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    assert_eq!(
        env,
        [
            ("CLAUSEO_RELAY", "127.0.0.1:{relay_port}"),
            ("CLAUSEO_RELAY_TOKEN_FILE", "{relay_token_file}"),
        ]
    );
}

/// Codex gets the entry on every OS, Windows included (the guard's session key, #1091). Claude
/// gets it on macOS and Linux and no browser at all on Windows (Claude's there is a later
/// change). No agent gets the shell command, so the command is copied nowhere. An agent with no
/// kind (a test's) is left as it was.
#[test]
fn codex_has_the_entry_everywhere_and_claude_off_windows() {
    use clauseo_protocol::AgentKind;
    for windows in [false, true] {
        assert!(!browser_mcp_on(None, windows));
        assert!(!no_browser_on(None, windows));
        assert!(!shell_browser_on(windows));
    }
    for windows in [false, true] {
        assert!(browser_mcp_on(Some(AgentKind::Codex), windows));
        assert!(!no_browser_on(Some(AgentKind::Codex), windows));
    }
    assert!(browser_mcp_on(Some(AgentKind::Claude), false));
    assert!(!no_browser_on(Some(AgentKind::Claude), false));
    assert!(!browser_mcp_on(Some(AgentKind::Claude), true));
    assert!(no_browser_on(Some(AgentKind::Claude), true));
}
