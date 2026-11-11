//! Codex's `browser` entry (`clauseo_profile::servers::browser_mcp`): no proxy, filled per
//! session on each OS.

use std::path::{Path, PathBuf};

use clauseo_acp::McpServer;
use clauseo_protocol::ToolsUnavailableReason;

use super::browser::{BrowserInputs, Claims, asks_for_proxy, hook_servers, no_browser, offered};
use super::profile_tests::{browser_server, claims, inputs, reason};

/// Codex's `browser` entry exactly as `clauseo_profile::servers::browser` builds it on Windows
/// (`windows`) or on macOS and Linux: no proxy.
fn codex_server(windows: bool) -> McpServer {
    let mut args = vec![
        "browser-guard",
        "--agent-browser",
        "/app/agent-browser",
        "--cwd",
        "{browser_dir}",
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
    env.push(("AGENT_BROWSER_STREAM_PORT", "{stream_port}"));
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

/// Codex's entry needs no proxy: with none set and none opened it goes, filled per session, on
/// Windows (the daemon port, no socket folder) and on macOS and Linux (the socket folder), and
/// carries no proxy variable. Every `AGENT_BROWSER_*` value reaches the guard through the entry,
/// since Codex starts it with codex-acp's `HOME`, not Clauseo's.
#[test]
fn codex_s_entry_goes_without_a_proxy_on_each_os() {
    let dir = PathBuf::from("/data/run/t-1/browser");
    let chrome = PathBuf::from("/chrome/msedge.exe");
    for windows in [false, true] {
        let claims = Claims {
            daemon_port: windows.then_some(61_102),
            socket_dir: (!windows).then(|| Path::new("/tmp/clauseo-ab-0123456789abcdef")),
            ..claims()
        };
        let no_proxy = BrowserInputs {
            proxy_set: false,
            port: None,
            claims: Some(claims),
            ..inputs(&dir, Some(&chrome))
        };
        let server = codex_server(windows);
        assert!(!asks_for_proxy(&server));
        let (servers, state) = hook_servers(vec![server], &no_proxy);
        assert_eq!(reason(state.clone()), None, "windows {windows}: {state:?}");
        let McpServer::Stdio { args, env, .. } = &servers[0];
        let mut want_args = vec![
            "browser-guard",
            "--agent-browser",
            "/app/agent-browser",
            "--cwd",
            "/data/run/t-1/browser",
            "--cdp-port",
            "61101",
        ];
        if windows {
            want_args.extend(["--daemon-port", "61102"]);
        }
        want_args.extend(["--", "mcp", "--tools", "core"]);
        assert_eq!(args, &want_args);
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
        want_env.push(("AGENT_BROWSER_STREAM_PORT", "61100"));
        let got: Vec<(&str, &str)> = env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        assert_eq!(got, want_env, "windows {windows}");
    }
    // The proxy-asking entry still needs one.
    assert!(asks_for_proxy(&browser_server()));
    let no_proxy = BrowserInputs {
        proxy_set: false,
        port: None,
        ..inputs(&dir, Some(&chrome))
    };
    let (_, state) = hook_servers(vec![browser_server()], &no_proxy);
    assert_eq!(reason(state), Some(ToolsUnavailableReason::NoProxy));
}

/// Codex on Windows has no browser (`no_browser`): its entry is dropped before the fill, so the
/// session sends none and its header holds no browser state at all, which the app shows as
/// nothing to act on (no install advice, no switch). Claude has no browser on any OS; Codex
/// elsewhere is untouched. Runs on every OS: the OS is a parameter.
#[test]
fn codex_on_windows_gets_no_entry_and_no_browser_state() {
    use clauseo_protocol::AgentKind;
    let dir = PathBuf::from("/data/browser/run/t-1");
    let chrome = PathBuf::from("/chrome");
    let ready = inputs(&dir, Some(&chrome));
    for windows in [false, true] {
        assert!(no_browser(Some(AgentKind::Claude), windows));
        assert!(!no_browser(None, windows));
        let none_here = no_browser(Some(AgentKind::Codex), windows);
        assert_eq!(none_here, windows);
        let servers = offered(vec![codex_server(false)], none_here);
        let (sent, state) = hook_servers(servers, &ready);
        if windows {
            assert_eq!(sent, Vec::<McpServer>::new());
            assert_eq!(state, None);
        } else {
            assert_eq!(sent.len(), 1);
            let state = state.expect("a browser state");
            assert_eq!(
                state.availability,
                clauseo_protocol::ToolAvailability::Available
            );
        }
    }
}
