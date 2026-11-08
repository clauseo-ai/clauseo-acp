//! The firm's Chrome and Edge `RemoteDebuggingAllowed` through the host: false turns the browser
//! off with `BrowserBlockedByPolicy` before any Chrome is picked (a forced one, or Chrome for
//! Testing in the roots), unless the firm's Clauseo policy allows Clauseo's browser anyway; a
//! policy that says nothing, or cannot be read, leaves the browser on.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_host::browser::chrome::{ChromeRoots, Platform};
use clauseo_host::{HostConfig, TestProxy};
use clauseo_protocol::{ClauseoTool, ToolsUnavailableReason};
use clauseo_settings::chromium_policy::{
    Browser, Found, Place, RemoteDebugging, RemoteDebuggingPolicy, Scope,
};
use serde_json::json;

use crate::profile_rig::{
    CLAUDE, Hook, Wired, agent_lines, agents, browser_server, first_frame_tools, settings,
    template, turn,
};
use crate::support::Dir;
use crate::write;

/// A firm policy that says `found` at Edge's user place.
#[derive(Debug)]
struct Says(Found);

impl RemoteDebuggingPolicy for Says {
    fn read(&self) -> RemoteDebugging {
        let place = Place {
            browser: Browser::Edge,
            scope: Scope::User,
        };
        RemoteDebugging {
            found: vec![(place, self.0.clone())],
        }
    }
}

/// How the session finds its Chrome.
#[derive(Clone, Copy)]
enum Chrome {
    /// The tests' forced Chrome.
    Forced,
    /// Chrome for Testing that Clauseo downloaded, in the roots, and nothing installed.
    ForTesting,
}

/// The browser state one `CLAUDE` session shows under `says`, with the firm's Clauseo
/// `policy` (a `policy.json`'s text) if any.
fn reason(
    test: &str,
    says: Found,
    policy: Option<&str>,
    chrome: Chrome,
) -> Option<ToolsUnavailableReason> {
    let dir = Dir::new(test, &[]);
    let claude = template(
        &dir,
        "claude",
        &agent_lines("claude", "session/new", &json!({}), &[]),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({}))]),
        json!({"tools": {"browser": true, "web_search": false}}),
    );
    if let Some(policy) = policy {
        dir.set_policy(policy);
    }
    let app = dir.data().join("app");
    let cft = app
        .join("browsers")
        .join("chrome-131.0.6778.85")
        .join("chrome-win64")
        .join("chrome.exe");
    write::create_dir_all(cft.parent().unwrap());
    write::write(&cft, b"");
    let roots = ChromeRoots {
        platform: Platform::WindowsX64,
        root: dir.data().join("no-root"),
        home: None,
        program_files: None,
        program_files_x86: None,
        local_app_data: None,
        app_data: app,
    };
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), move |config| {
        let config = config
            .with_profile_hook(Box::new(Hook {
                servers: vec![browser_server()],
                ..Hook::default()
            }))
            .with_egress_proxy(Arc::new(TestProxy(8123)))
            .with_remote_debugging_policy(Arc::new(Says(says)));
        with_chrome(config, chrome, roots)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let tools = first_frame_tools(&host, thread);
    host.shutdown();
    let state = tools
        .iter()
        .find(|state| state.tool == ClauseoTool::Browser)
        .expect("the header shows the browser");
    match &state.availability {
        clauseo_protocol::ToolAvailability::Unavailable { reason } => Some(*reason),
        _ => None,
    }
}

fn with_chrome(config: HostConfig, chrome: Chrome, roots: ChromeRoots) -> HostConfig {
    match chrome {
        Chrome::Forced => config.with_browser_executable(Some(PathBuf::from("/chrome"))),
        Chrome::ForTesting => config.with_chrome_roots(roots),
    }
}

const ALLOW: &str = r#"{"browser": {"allow_when_remote_debugging_disabled": true}}"#;

#[test]
fn remote_debugging_off_blocks_a_forced_chrome() {
    assert_eq!(
        reason("rd_off_forced", Found::Disallowed, None, Chrome::Forced),
        Some(ToolsUnavailableReason::BrowserBlockedByPolicy)
    );
}

#[test]
fn remote_debugging_off_blocks_chrome_for_testing() {
    assert_eq!(
        reason("rd_off_cft", Found::Disallowed, None, Chrome::ForTesting),
        Some(ToolsUnavailableReason::BrowserBlockedByPolicy)
    );
}

#[test]
fn the_firms_clauseo_policy_allows_the_browser_anyway() {
    assert_eq!(
        reason(
            "rd_off_allowed",
            Found::Disallowed,
            Some(ALLOW),
            Chrome::Forced
        ),
        None
    );
}

#[test]
fn a_policy_that_says_nothing_or_cannot_be_read_does_not_block() {
    let unreadable = Found::Unreadable("the registry read failed (5)".to_owned());
    assert_eq!(
        reason("rd_absent", Found::Absent, None, Chrome::Forced),
        None
    );
    assert_eq!(
        reason("rd_unreadable", unreadable, None, Chrome::Forced),
        None
    );
    assert_eq!(reason("rd_on", Found::Allowed, None, Chrome::Forced), None);
}
