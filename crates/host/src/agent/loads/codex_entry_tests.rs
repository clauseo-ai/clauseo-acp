//! Codex's `browser` entry (`clauseo_profile::servers::browser_mcp`): the session's proxy and
//! its password (ADR 0053), filled per session on each OS.

use std::path::{Path, PathBuf};

use clauseo_acp::McpServer;
use clauseo_protocol::ToolsUnavailableReason;

use super::browser::{BrowserInputs, Claims, asks_for_proxy, hook_servers, no_browser, offered};
use super::profile_tests::{PASSWORD, SECRET_FILE, claims, inputs, reason};

/// Codex's `browser` entry exactly as `clauseo_profile::servers::browser` builds it on Windows
/// (`windows`) or on macOS and Linux: the proxy's address and password as variables.
fn codex_server(windows: bool) -> McpServer {
    let mut args = vec![
        "browser-guard",
        "--agent-browser",
        "/app/agent-browser",
        "--cwd",
        "{browser_dir}",
        "--downloads",
        "{matter_downloads}",
        "--fallback-downloads",
        "{matter_fallback_downloads}",
        "--cdp-port",
        "{cdp_port}",
    ];
    if windows {
        args.extend(["--daemon-port", "{daemon_port}"]);
    }
    args.extend(["--", "mcp", "--tools", "core"]);
    let mut env = vec![
        ("AGENT_BROWSER_CONFIG", "/data/browser/agent-browser.json"),
        ("AGENT_BROWSER_EXECUTABLE_PATH", "{browser_executable}"),
        ("AGENT_BROWSER_SESSION", "{session_id}"),
    ];
    if !windows {
        env.push(("AGENT_BROWSER_SOCKET_DIR", "{socket_dir}"));
    }
    env.push(("AGENT_BROWSER_PROXY", "http://127.0.0.1:{proxy_port}"));
    env.push(("CLAUSEO_PROXY_SECRET_FILE", "{proxy_secret_file}"));
    McpServer::Stdio {
        name: "browser".to_owned(),
        command: PathBuf::from("/app/clauseo-mcp"),
        args: args.into_iter().map(str::to_owned).collect(),
        env: env
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
    }
}

/// Codex's entry asks for the session's proxy (ADR 0053): with one opened it goes, filled per
/// session, on Windows (the daemon port, no socket folder) and on macOS and Linux (the socket
/// folder), with the proxy's address and its browser listener's password as variables, the
/// password in no argument. Every `AGENT_BROWSER_*` value reaches the guard through the entry,
/// since Codex starts it with codex-acp's `HOME`, not Clauseo's. With no proxy set, or one that
/// did not open, it is left out (`NoProxy`, or `BlockListNotLoaded` when that was why).
#[test]
fn codex_s_entry_goes_with_the_session_s_proxy_on_each_os() {
    let dir = PathBuf::from("/data/run/t-1/browser");
    let chrome = PathBuf::from("/chrome/msedge.exe");
    for windows in [false, true] {
        let claims = Claims {
            daemon_port: windows.then_some(61_102),
            socket_dir: (!windows).then(|| Path::new("/tmp/clauseo-ab-0123456789abcdef")),
            ..claims()
        };
        let opened = BrowserInputs {
            claims: Some(claims),
            ..inputs(&dir, Some(&chrome))
        };
        let server = codex_server(windows);
        assert!(asks_for_proxy(&server));
        for unopened in [
            BrowserInputs {
                proxy_set: false,
                port: None,
                secret_file: None,
                ..opened
            },
            BrowserInputs {
                port: None,
                secret_file: None,
                ..opened
            },
        ] {
            let (sent, state) = hook_servers(vec![server.clone()], &unopened);
            assert_eq!(sent, Vec::<McpServer>::new());
            assert_eq!(reason(state), Some(ToolsUnavailableReason::NoProxy));
        }
        // A proxy that refused to start with no block list loaded says so.
        let no_list = BrowserInputs {
            port: None,
            secret_file: None,
            proxy_refused: Some(ToolsUnavailableReason::BlockListNotLoaded),
            ..opened
        };
        let (sent, state) = hook_servers(vec![server.clone()], &no_list);
        assert_eq!(sent, Vec::<McpServer>::new());
        let want = Some(ToolsUnavailableReason::BlockListNotLoaded);
        assert_eq!(reason(state), want);
        let (servers, state) = hook_servers(vec![server], &opened);
        assert_eq!(reason(state.clone()), None, "windows {windows}: {state:?}");
        let McpServer::Stdio { args, env, .. } = &servers[0] else {
            panic!("expected a stdio server");
        };
        let mut want_args = vec![
            "browser-guard",
            "--agent-browser",
            "/app/agent-browser",
            "--cwd",
            "/data/run/t-1/browser",
            "--downloads",
            "/matters/smith/downloads",
            "--fallback-downloads",
            "/data/agent-state/m1/downloads",
            "--cdp-port",
            "61101",
        ];
        if windows {
            want_args.extend(["--daemon-port", "61102"]);
        }
        want_args.extend(["--", "mcp", "--tools", "core"]);
        assert_eq!(args, &want_args);
        // The password is in no argument and no variable: only its file's path is.
        assert!(args.iter().all(|arg| !arg.contains(PASSWORD)));
        assert!(env.iter().all(|(_, value)| !value.contains(PASSWORD)));
        let mut want_env = vec![
            ("AGENT_BROWSER_CONFIG", "/data/browser/agent-browser.json"),
            ("AGENT_BROWSER_EXECUTABLE_PATH", "/chrome/msedge.exe"),
            ("AGENT_BROWSER_SESSION", "t-1"),
        ];
        if !windows {
            want_env.push((
                "AGENT_BROWSER_SOCKET_DIR",
                "/tmp/clauseo-ab-0123456789abcdef",
            ));
        }
        want_env.push(("AGENT_BROWSER_PROXY", "http://127.0.0.1:8123"));
        want_env.push(("CLAUSEO_PROXY_SECRET_FILE", SECRET_FILE));
        let got: Vec<(&str, &str)> = env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        assert_eq!(got, want_env, "windows {windows}");
    }
}

/// Codex has the browser on every OS, Windows included (`no_browser`, the guard's session key
/// from #1091): its entry goes out and its header holds the browser's state. Claude has no
/// browser on Windows yet and the entry elsewhere. Runs on every OS: the OS is a parameter.
#[test]
fn codex_gets_the_entry_and_its_browser_state_on_every_os() {
    use clauseo_protocol::AgentKind;
    let dir = PathBuf::from("/data/browser/run/t-1");
    let chrome = PathBuf::from("/chrome");
    let ready = inputs(&dir, Some(&chrome));
    for windows in [false, true] {
        assert_eq!(no_browser(Some(AgentKind::Claude), windows), windows);
        assert!(!no_browser(None, windows));
        let none_here = no_browser(Some(AgentKind::Codex), windows);
        assert!(!none_here, "windows {windows}");
        let servers = offered(vec![codex_server(false)], none_here);
        let (sent, state) = hook_servers(servers, &ready);
        assert_eq!(sent.len(), 1, "windows {windows}");
        let state = state.expect("a browser state");
        assert_eq!(
            state.availability,
            clauseo_protocol::ToolAvailability::Available
        );
    }
}

#[test]
fn the_fallback_folder_is_the_notes_folders_sibling_the_desktop_reveals_in() {
    let data = PathBuf::from(crate::testing::abs("/data"));
    let data = data.as_path();
    let matter = crate::pipeline::tests::support::id(7);
    let notes = crate::agent::matter_state_dir(data, matter).join("notes");
    let folder = super::browser::fallback_downloads(&notes);
    assert_eq!(
        folder,
        super::browser::matter_fallback_downloads(data, matter)
    );
    assert_eq!(
        folder,
        data.join("agent-state")
            .join(matter.to_string())
            .join("downloads")
    );
}
