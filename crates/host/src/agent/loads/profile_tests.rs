//! The launch profile's host half (R21, R22): the browser entry and the instructions' tokens.
//! Claude's keys, the login and the web-search state are in `profile_keys_tests.rs`.

use std::path::{Path, PathBuf};

use clauseo_acp::McpServer;
use clauseo_protocol::{ClauseoTool, ToolAvailability, ToolState, ToolsUnavailableReason};

use super::McpControl;
use super::browser::{BrowserInputs, Claims, Switch, hook_servers};
use super::profile::{Folders, fill_instructions};
use super::profile_keys_tests::folders;

pub(super) const ON: Switch = Switch {
    on: true,
    locked: false,
};
pub(super) const OFF: Switch = Switch {
    on: false,
    locked: false,
};
pub(super) const LOCKED_OFF: Switch = Switch {
    on: false,
    locked: true,
};

/// A `browser` entry asking for the proxy (`{proxy_port}`), as the Safe workspace's would.
pub(super) fn browser_server() -> McpServer {
    McpServer::Stdio {
        name: "browser".to_owned(),
        command: PathBuf::from("/app/clauseo-mcp"),
        args: [
            "browser-guard",
            "--agent-browser",
            "/app/agent-browser",
            "--cwd",
            "{browser_dir}",
            "--cdp-port",
            "{cdp_port}",
            "--",
            "mcp",
            "--tools",
            "core",
        ]
        .map(str::to_owned)
        .to_vec(),
        env: vec![
            (
                "AGENT_BROWSER_CONFIG".to_owned(),
                "/data/browser/agent-browser.json".to_owned(),
            ),
            (
                "AGENT_BROWSER_PROXY".to_owned(),
                "http://127.0.0.1:{proxy_port}".to_owned(),
            ),
            (
                "AGENT_BROWSER_SESSION".to_owned(),
                "{session_id}".to_owned(),
            ),
            (
                "AGENT_BROWSER_EXECUTABLE_PATH".to_owned(),
                "{browser_executable}".to_owned(),
            ),
            (
                "AGENT_BROWSER_STREAM_PORT".to_owned(),
                "{stream_port}".to_owned(),
            ),
            (
                "AGENT_BROWSER_SOCKET_DIR".to_owned(),
                "{socket_dir}".to_owned(),
            ),
        ],
    }
}

/// A session's claims as the tests fill them: macOS and Linux's, with no daemon port.
pub(super) fn claims() -> Claims<'static> {
    Claims {
        name: "t-1",
        cdp_port: 61_101,
        stream_port: 61_100,
        daemon_port: None,
        socket_dir: Some(Path::new("/tmp/clauseo-ab-0123456789abcdef")),
    }
}

fn other_server() -> McpServer {
    McpServer::Stdio {
        name: "firm-docs".to_owned(),
        command: PathBuf::from("/opt/firm-docs"),
        args: vec!["{session_id}".to_owned()],
        env: Vec::new(),
    }
}

pub(super) fn inputs<'a>(browser_dir: &'a Path, chrome: Option<&'a Path>) -> BrowserInputs<'a> {
    BrowserInputs {
        switch: ON,
        control: McpControl::Clauseo,
        slot_free: true,
        proxy_set: true,
        port: Some(8123),
        blocked_by_policy: None,
        executable: chrome,
        browser_dir,
        claims: Some(claims()),
    }
}

pub(super) fn reason(state: Option<ToolState>) -> Option<ToolsUnavailableReason> {
    match state?.availability {
        ToolAvailability::Unavailable { reason } => Some(reason),
        ToolAvailability::Available | ToolAvailability::Unknown { .. } => None,
    }
}

#[test]
fn the_browser_entry_is_filled_per_session() {
    let dir = PathBuf::from("/data/run/t-1/browser");
    let chrome = PathBuf::from("/Applications/Chrome");
    let (servers, state) = hook_servers(
        vec![browser_server(), other_server()],
        &inputs(&dir, Some(&chrome)),
    );
    assert_eq!(
        state,
        Some(ToolState {
            tool: ClauseoTool::Browser,
            availability: ToolAvailability::Available,
        })
    );
    let McpServer::Stdio { args, env, .. } = &servers[0] else {
        panic!("expected a stdio server");
    };
    assert_eq!(args[4], dir.to_str().unwrap());
    assert_eq!(
        env.iter()
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>(),
        [
            "/data/browser/agent-browser.json",
            "http://127.0.0.1:8123",
            "t-1",
            "/Applications/Chrome",
            "61100",
            "/tmp/clauseo-ab-0123456789abcdef",
        ]
    );
    assert_eq!(args[5..7], ["--cdp-port", "61101"]);
    // Another hook server goes as it came: only the browser's tokens are the host's.
    assert_eq!(servers[1], other_server());
}

#[test]
fn a_folder_named_like_a_token_stays_literal() {
    let dir = PathBuf::from("/data/{session_id}/{proxy_port}/browser");
    let chrome = PathBuf::from("/chrome/{browser_dir}");
    let (servers, _) = hook_servers(vec![browser_server()], &inputs(&dir, Some(&chrome)));
    let McpServer::Stdio { args, env, .. } = &servers[0] else {
        panic!("expected a stdio server");
    };
    assert_eq!(args[4], "/data/{session_id}/{proxy_port}/browser");
    assert_eq!(env[3].1, "/chrome/{browser_dir}");
    let folders = Folders {
        work: "/w/{matter_notes}",
        notes: "/n/{work_folder}",
        ..folders()
    };
    assert_eq!(
        fill_instructions("{work_folder} and {matter_notes}", folders).unwrap(),
        "/w/{matter_notes} and /n/{work_folder}"
    );
}

#[test]
fn each_reason_drops_the_browser_in_order() {
    let dir = PathBuf::from("/b");
    let chrome = PathBuf::from("/chrome");
    let base = inputs(&dir, Some(&chrome));
    let cases = [
        (
            BrowserInputs {
                switch: OFF,
                ..base
            },
            ToolsUnavailableReason::Off,
        ),
        (
            BrowserInputs {
                switch: LOCKED_OFF,
                control: McpControl::Managed,
                ..base
            },
            ToolsUnavailableReason::LockedOff,
        ),
        (
            BrowserInputs {
                control: McpControl::Managed,
                port: None,
                ..base
            },
            ToolsUnavailableReason::ManagedByFirm,
        ),
        (
            BrowserInputs {
                slot_free: false,
                port: None,
                ..base
            },
            ToolsUnavailableReason::TooManyBrowsers,
        ),
        (
            BrowserInputs {
                proxy_set: false,
                port: None,
                executable: None,
                ..base
            },
            ToolsUnavailableReason::NoProxy,
        ),
        (
            BrowserInputs {
                blocked_by_policy: Some(ToolsUnavailableReason::BrowserBlockedByPolicy),
                port: None,
                executable: None,
                claims: None,
                ..base
            },
            ToolsUnavailableReason::BrowserBlockedByPolicy,
        ),
        (
            BrowserInputs {
                blocked_by_policy: Some(ToolsUnavailableReason::BrowserPolicyUnreadable),
                port: None,
                executable: None,
                claims: None,
                ..base
            },
            ToolsUnavailableReason::BrowserPolicyUnreadable,
        ),
        (
            BrowserInputs {
                port: None,
                executable: None,
                ..base
            },
            ToolsUnavailableReason::BrowserNotInstalled,
        ),
        (
            BrowserInputs {
                port: None,
                claims: None,
                ..base
            },
            ToolsUnavailableReason::BrowserNotInstalled,
        ),
        (
            BrowserInputs { port: None, ..base },
            ToolsUnavailableReason::NoProxy,
        ),
    ];
    for (case, expected) in cases {
        let (servers, state) = hook_servers(vec![browser_server()], &case);
        assert_eq!(servers.len(), 0, "{expected:?}");
        assert_eq!(reason(state), Some(expected));
    }
}

/// The Windows entry's `{daemon_port}` fills from the daemon claim; a token the session's claims
/// cannot fill (`{daemon_port}` with no daemon claim, `{socket_dir}` with no folder) drops the
/// entry with `BrowserNotInstalled`.
#[test]
fn the_port_tokens_fill_and_an_unfillable_one_drops_the_entry() {
    let dir = PathBuf::from("/b");
    let chrome = PathBuf::from("/chrome");
    let windows_entry = || {
        let McpServer::Stdio {
            name,
            command,
            mut args,
            mut env,
        } = browser_server()
        else {
            panic!("expected a stdio server");
        };
        args.splice(
            7..7,
            ["--daemon-port".to_owned(), "{daemon_port}".to_owned()],
        );
        env.retain(|(key, _)| key != "AGENT_BROWSER_SOCKET_DIR");
        McpServer::Stdio {
            name,
            command,
            args,
            env,
        }
    };
    let windows_claims = Claims {
        name: "Qx7_drawn-name00",
        daemon_port: Some(61_102),
        socket_dir: None,
        ..claims()
    };
    let windows = BrowserInputs {
        claims: Some(windows_claims),
        ..inputs(&dir, Some(&chrome))
    };
    let (servers, state) = hook_servers(vec![windows_entry()], &windows);
    assert_eq!(reason(state), None);
    let McpServer::Stdio { args, env, .. } = &servers[0] else {
        panic!("expected a stdio server");
    };
    assert_eq!(
        args[5..9],
        ["--cdp-port", "61101", "--daemon-port", "61102"]
    );
    assert!(env.contains(&(
        "AGENT_BROWSER_SESSION".to_owned(),
        "Qx7_drawn-name00".to_owned()
    )));

    // The Windows entry with no daemon claim, and the Unix entry with no socket folder.
    let no_daemon = BrowserInputs {
        claims: Some(Claims {
            daemon_port: None,
            ..windows_claims
        }),
        ..windows
    };
    let no_folder = BrowserInputs {
        claims: Some(Claims {
            socket_dir: None,
            ..claims()
        }),
        ..inputs(&dir, Some(&chrome))
    };
    for (entry, case) in [(windows_entry(), no_daemon), (browser_server(), no_folder)] {
        let (servers, state) = hook_servers(vec![entry], &case);
        assert_eq!(servers.len(), 0);
        assert_eq!(
            reason(state),
            Some(ToolsUnavailableReason::BrowserNotInstalled)
        );
    }
}

#[test]
fn managed_mcp_sends_no_hook_server_at_all() {
    let dir = PathBuf::from("/b");
    let base = inputs(&dir, None);
    let managed = BrowserInputs {
        control: McpControl::Managed,
        ..base
    };
    let (servers, _) = hook_servers(vec![other_server(), browser_server()], &managed);
    assert_eq!(servers.len(), 0);
    let (servers, state) = hook_servers(vec![other_server()], &base);
    assert_eq!(servers, [other_server()]);
    assert_eq!(state, None, "no browser offered, no browser state");
}
