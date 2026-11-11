//! The firm's Chrome, Edge and Chromium `RemoteDebuggingAllowed` through the host: false turns
//! the browser off with `BrowserBlockedByPolicy` before any Chrome is picked (a forced one, or
//! Chrome for Testing in the roots), and a place that cannot be read with
//! `BrowserPolicyUnreadable`, until a later launch reads it, unless the firm's Clauseo policy
//! allows Clauseo's browser anyway; a policy that says nothing, or holds a bad value, leaves the
//! browser on.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use clauseo_host::browser::chrome::{ChromeRoots, Platform};
use clauseo_host::{HostConfig, TestProxy};
use clauseo_protocol::{ClauseoTool, MatterId, ThreadId, ToolsUnavailableReason};
use clauseo_settings::chromium_policy::{
    Browser, Found, Place, RemoteDebugging, RemoteDebuggingPolicy, Scope,
};
use serde_json::json;

use crate::profile_rig::{
    BROWSING, Hook, Wired, agent_lines, agents, browser_server, first_frame_tools, settings,
    template, turn,
};
use crate::support::{Dir, Running};
use crate::write;

#[path = "browser_policy_recheck.rs"]
mod recheck;

/// A firm policy that says `found` at Edge's user place.
#[derive(Debug)]
pub(super) struct Says(pub(super) Found);

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

/// A firm policy whose first read fails at Edge's user place, and whose later reads find
/// nothing set.
#[derive(Debug, Default)]
struct FailsOnce(AtomicUsize);

impl RemoteDebuggingPolicy for FailsOnce {
    fn read(&self) -> RemoteDebugging {
        let first = self.0.fetch_add(1, Ordering::SeqCst) == 0;
        let said = if first {
            Found::Unreadable("the registry read failed (5)".to_owned())
        } else {
            Found::Absent
        };
        Says(said).read()
    }
}

/// How the session finds its Chrome.
#[derive(Clone, Copy)]
pub(super) enum Chrome {
    /// The tests' forced Chrome.
    Forced,
    /// Chrome for Testing that Clauseo downloaded, in the roots, and nothing installed.
    ForTesting,
}

/// The browser state one `BROWSING` session shows under `says`, with the firm's Clauseo
/// `policy` (a `policy.json`'s text) if any.
fn reason(
    test: &str,
    says: Found,
    policy: Option<&str>,
    chrome: Chrome,
) -> Option<ToolsUnavailableReason> {
    reasons(test, Arc::new(Says(says)), policy, chrome, 1)
        .pop()
        .expect("one launch")
}

/// A host whose `BROWSING` sessions may have the browser, under `rd_policy` and the firm's
/// Clauseo `policy` (a `policy.json`'s text) if any, with one matter for its threads.
pub(super) struct Rig {
    pub(super) host: Running,
    matter: MatterId,
    /// Kept until the host has stopped.
    _dir: Dir,
}

impl Rig {
    pub(super) fn start(
        test: &str,
        rd_policy: Arc<dyn RemoteDebuggingPolicy>,
        policy: Option<&str>,
        chrome: Chrome,
    ) -> Self {
        let dir = Dir::new(test, &[]);
        let claude = template(
            &dir,
            "claude",
            &agent_lines("claude", "session/new", &json!({}), &[]),
        );
        settings(
            &dir,
            agents(&[(BROWSING, &claude, json!({}))]),
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
                .with_remote_debugging_policy(rd_policy);
            with_chrome(config, chrome, roots)
        });
        let matter = host.create_matter("m", &dir.folder("m", &[]));
        Self {
            host,
            matter,
            _dir: dir,
        }
    }

    /// One more session, in a thread of its own, after a turn: the thread and the browser state
    /// its header shows.
    pub(super) fn launch(&self) -> (ThreadId, Option<ToolsUnavailableReason>) {
        let thread = self.host.create_thread(self.matter, BROWSING);
        turn(&self.host, thread);
        (thread, self.browser(thread))
    }

    /// Why `thread`'s header says it has no browser, on a new subscriber's first frame; `None`
    /// when it has one.
    pub(super) fn browser(&self, thread: ThreadId) -> Option<ToolsUnavailableReason> {
        let tools = first_frame_tools(&self.host, thread);
        let state = tools
            .iter()
            .find(|state| state.tool == ClauseoTool::Browser)
            .expect("the header shows the browser");
        match &state.availability {
            clauseo_protocol::ToolAvailability::Unavailable { reason } => Some(*reason),
            _ => None,
        }
    }

    pub(super) fn shutdown(self) {
        self.host.shutdown();
    }
}

/// The browser state each of `launches` `BROWSING` sessions shows, one after the other, each in
/// a thread of its own, under `rd_policy` and the firm's Clauseo `policy` if any.
fn reasons(
    test: &str,
    rd_policy: Arc<dyn RemoteDebuggingPolicy>,
    policy: Option<&str>,
    chrome: Chrome,
    launches: usize,
) -> Vec<Option<ToolsUnavailableReason>> {
    let rig = Rig::start(test, rd_policy, policy, chrome);
    let found = (0..launches).map(|_| rig.launch().1).collect();
    rig.shutdown();
    found
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
fn a_policy_that_says_nothing_true_or_a_bad_value_does_not_block() {
    let bad = Found::BadValue("the REG_SZ is not an integer".to_owned());
    assert_eq!(
        reason("rd_absent", Found::Absent, None, Chrome::Forced),
        None
    );
    assert_eq!(reason("rd_bad", bad, None, Chrome::Forced), None);
    assert_eq!(reason("rd_on", Found::Allowed, None, Chrome::Forced), None);
}

#[test]
fn a_read_error_blocks_this_launch_and_the_next_launch_reads_again() {
    assert_eq!(
        reasons(
            "rd_unreadable",
            Arc::new(FailsOnce::default()),
            None,
            Chrome::Forced,
            2
        ),
        [Some(ToolsUnavailableReason::BrowserPolicyUnreadable), None]
    );
}

#[test]
fn the_firms_clauseo_policy_allows_the_browser_when_the_policy_was_not_read() {
    let unreadable = Found::Unreadable("the folder was not read (permission denied)".to_owned());
    assert_eq!(
        reason(
            "rd_unreadable_allowed",
            unreadable,
            Some(ALLOW),
            Chrome::Forced
        ),
        None
    );
}
