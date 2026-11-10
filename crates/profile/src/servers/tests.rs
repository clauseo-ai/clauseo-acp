use std::path::{Path, PathBuf};

use clauseo_settings::Locations;

use super::{CASE_LAW, browser_for, browser_mcp_on, case_law, no_browser_on};
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
    assert_eq!(unix.env["AGENT_BROWSER_STREAM_PORT"], "{stream_port}");
    assert_eq!(windows.env["AGENT_BROWSER_STREAM_PORT"], "{stream_port}");
    assert_eq!(unix.env["AGENT_BROWSER_SOCKET_DIR"], "{socket_dir}");
    assert!(!windows.env.contains_key("AGENT_BROWSER_SOCKET_DIR"));
    // No proxy: Chrome goes direct, and the host opens no egress proxy for the entry.
    for entry in [&unix, &windows] {
        assert!(!entry.env.contains_key("AGENT_BROWSER_PROXY"));
        assert!(
            entry
                .args
                .iter()
                .chain(entry.env.values())
                .all(|text| !text.contains("{proxy_port}"))
        );
    }
    assert_eq!(unix.env.len(), 5);
    assert_eq!(windows.env.len(), 4);
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

/// Codex gets the entry on macOS and Linux and no browser at all on Windows, where its part points
/// the lawyer to a Claude conversation with no install advice and no tool named; Claude never gets
/// the entry and always may have its shell command.
#[test]
fn codex_has_the_entry_off_windows_and_no_browser_on_windows() {
    use crate::instructions::{BrowserPart, browser_part};
    use clauseo_protocol::AgentKind;
    for windows in [false, true] {
        assert!(!browser_mcp_on(Some(AgentKind::Claude), windows));
        assert!(!no_browser_on(Some(AgentKind::Claude), windows));
        assert!(!browser_mcp_on(None, windows));
    }
    assert!(browser_mcp_on(Some(AgentKind::Codex), false));
    assert!(!no_browser_on(Some(AgentKind::Codex), false));
    assert!(!browser_mcp_on(Some(AgentKind::Codex), true));
    assert!(no_browser_on(Some(AgentKind::Codex), true));
    let claude_only = browser_part(BrowserPart::ClaudeOnly);
    assert!(claude_only.contains("You have no browser in this session"));
    assert!(
        claude_only.contains("a Claude conversation can"),
        "{claude_only}"
    );
    for word in ["Chrome", "install", "MCP", "tool", "skill", "agent-browser"] {
        assert!(!claude_only.contains(word), "{word}: {claude_only}");
    }
    assert!(claude_only.split_whitespace().count() <= 40);
}
