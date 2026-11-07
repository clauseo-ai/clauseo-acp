//! The session's egress proxy through the host (S16, R17's seam): opened once per launch that
//! can get the browser and found its Chrome, closed once after the agent's tree has gone (in the
//! run folder's removal), closed too after a launch that failed once it was opened, and never
//! named in the agent's environment. A proxy that does not open leaves the browser out with
//! `NoProxy`. `egress_wait.rs` has the removal's wait for stragglers.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::{EgressProxy, HostConfig, SessionKey};
use clauseo_protocol::ToolsUnavailableReason;
use serde_json::json;

use crate::profile_rig::{
    CLAUDE, Hook, Wired, agent_lines, agents, browser_server, first_frame_tools, settings,
    template, turn,
};
use crate::support::Dir;

/// The proxy variables an agent's environment must not carry in tier C.
const PROXY_VARS: [&str; 8] = [
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "https_proxy",
    "http_proxy",
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// A proxy that records each call, in order, beside the run folder removals; `open` fails when
/// `refuse` is set. Each close is also sent on `closed`.
pub(crate) struct Recording {
    calls: Arc<Mutex<Vec<String>>>,
    refuse: bool,
    closed: Mutex<Sender<SessionKey>>,
}

impl Recording {
    pub(crate) fn new(refuse: bool) -> (Arc<Self>, Arc<Mutex<Vec<String>>>, Receiver<SessionKey>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (closed, closes) = channel();
        let proxy = Arc::new(Self {
            calls: Arc::clone(&calls),
            refuse,
            closed: Mutex::new(closed),
        });
        (proxy, calls, closes)
    }
}

impl EgressProxy for Recording {
    fn open(
        &self,
        session: SessionKey,
        config: clauseo_sandbox::ProxyConfig,
    ) -> Result<(), clauseo_sandbox::Error> {
        assert!(config.allow.contains("api.anthropic.com"), "{config:?}");
        assert!(config.allow.contains("sci.gov.in"), "{config:?}");
        // Stage C's rule: the host opens a proxy only for a launch that offers the browser, and
        // binds its token-free browser listener; a launch without one opens none (the tests
        // `no_proxy_opens_*` and `the_browser_slot_stays_taken_during_the_wait`).
        assert!(
            config.browser,
            "a proxy opened without its browser listener"
        );
        self.calls
            .lock()
            .unwrap()
            .push(format!("open {}", session.launch));
        if self.refuse {
            return Err(clauseo_sandbox::Error::ProxyStart(std::io::Error::other(
                "no port",
            )));
        }
        Ok(())
    }

    fn port_for(&self, _session: SessionKey) -> Option<u16> {
        Some(8123)
    }

    fn close(&self, session: SessionKey) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("close {}", session.launch));
        let _sent = self.closed.lock().unwrap().send(session);
    }
}

/// The calls so far, launch ids replaced by their order of first appearance, so `open 1`
/// and `close 1` name the same launch.
pub(crate) fn calls(log: &Mutex<Vec<String>>) -> Vec<String> {
    let mut launches: Vec<String> = Vec::new();
    log.lock()
        .unwrap()
        .iter()
        .map(|call| match call.split_once(' ') {
            Some((verb, launch)) => {
                let at = launches
                    .iter()
                    .position(|seen| seen == launch)
                    .unwrap_or_else(|| {
                        launches.push(launch.to_owned());
                        launches.len() - 1
                    });
                format!("{verb} {}", at + 1)
            }
            None => call.clone(),
        })
        .collect()
}

/// `config` with the browser hook, `proxy`, a forced Chrome, the run folder removal recorded in
/// `log`, and an inherited environment without any proxy variable.
pub(crate) fn wired(
    config: HostConfig,
    proxy: Arc<Recording>,
    log: Arc<Mutex<Vec<String>>>,
) -> HostConfig {
    let inherited: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| !PROXY_VARS.iter().any(|var| name == var))
        .collect();
    config
        .with_profile_hook(Box::new(Hook {
            servers: vec![browser_server()],
            ..Hook::default()
        }))
        .with_egress_proxy(proxy)
        .with_browser_executable(Some(PathBuf::from("/chrome")))
        .with_inherited_env(inherited)
        .with_run_removal_hook(Arc::new(move |_run| {
            log.lock().unwrap().push("removal".to_owned());
        }))
}

/// A Claude scenario whose `session/new` carries exactly `servers`.
fn claude(dir: &Dir, servers: &serde_json::Value) {
    claude_then(dir, servers, &[]);
}

/// [`claude`] with `then` played after the prompt's answer.
pub(crate) fn claude_then(dir: &Dir, servers: &serde_json::Value, then: &[serde_json::Value]) {
    let mut lines = agent_lines(
        "claude",
        "session/new",
        &json!({"mcpServers": servers}),
        &[],
    );
    for line in then {
        lines = format!("{lines}\n{line}");
    }
    let scenario = template(dir, "claude", &lines);
    settings(
        dir,
        agents(&[(CLAUDE, &scenario, json!({}))]),
        json!({"tools": {"browser": true, "web_search": false}}),
    );
}

/// One launch: the proxy opens once before the agent starts and stays open while it runs; it
/// closes once, after the run folder's removal, which comes once the agent's tree has exited.
/// The agent's environment carries no proxy variable, and the browser entry goes.
#[test]
fn the_proxy_opens_once_per_launch_and_closes_after_the_tree_has_gone() {
    let dir = Dir::new("egress_lifecycle", &[]);
    claude(&dir, &json!([{"name": "browser"}]));
    let (proxy, log, _closes) = Recording::new(false);
    let launcher = Wired::new(&dir);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), move |config| {
        wired(config, proxy, removals)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let browser = first_frame_tools(&host, thread)
        .into_iter()
        .find(|state| state.tool == clauseo_protocol::ClauseoTool::Browser)
        .expect("the header shows the browser");
    assert_eq!(
        browser.availability,
        clauseo_protocol::ToolAvailability::Available
    );
    assert_eq!(calls(&log), ["open 1"], "open while the agent runs");
    let seen = launcher.seen();
    assert_eq!(seen.len(), 1);
    for var in PROXY_VARS {
        assert!(
            !seen[0].env.contains_key(&OsString::from(var)),
            "the agent got {var}"
        );
    }
    host.shutdown();
    assert_eq!(calls(&log), ["open 1", "removal", "close 1"]);
}

/// A launcher that fails every launch, as an OS that cannot start the program would.
struct Refuses;

impl clauseo_process::Launcher for Refuses {
    fn launch(
        &self,
        spec: &clauseo_process::LaunchSpec,
    ) -> Result<clauseo_process::Launched, clauseo_process::Error> {
        Err(clauseo_process::Error::Spawn {
            program: spec.program().to_path_buf(),
            source: std::io::Error::other("refused"),
        })
    }
}

/// A launch that fails after its proxy opened (the spawn itself failed, so nothing runs): the
/// proxy closes in that launch's removal, before the host stops.
#[test]
fn a_failed_launch_closes_the_proxy_it_opened() {
    let dir = Dir::new("egress_failed_launch", &[]);
    claude(&dir, &json!([{"name": "browser"}]));
    let (proxy, log, closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Arc::new(Refuses), move |config| {
        wired(config, proxy, removals)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    let _turn = host.prompt(thread, "Go.");
    closes
        .recv_timeout(Duration::from_secs(60))
        .expect("the failed launch closed its proxy");
    assert_eq!(calls(&log), ["open 1", "removal", "close 1"]);
    host.shutdown();
    assert_eq!(calls(&log), ["open 1", "removal", "close 1"], "closed once");
}

/// A proxy that does not open leaves the browser out with `NoProxy`, though the proxy would
/// answer a port; the launch goes on, and nothing is closed.
#[test]
fn a_proxy_that_does_not_open_leaves_the_browser_out() {
    let dir = Dir::new("egress_open_fails", &[]);
    claude(&dir, &json!([]));
    let (proxy, log, _closes) = Recording::new(true);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let browser = first_frame_tools(&host, thread)
        .into_iter()
        .find(|state| state.tool == clauseo_protocol::ClauseoTool::Browser)
        .expect("the header shows the browser");
    assert_eq!(
        browser.availability,
        clauseo_protocol::ToolAvailability::Unavailable {
            reason: ToolsUnavailableReason::NoProxy
        }
    );
    host.shutdown();
    assert_eq!(calls(&log), ["open 1", "removal"]);
}

/// With `tools.browser` off no proxy opens at all.
#[test]
fn no_proxy_opens_with_the_browser_off() {
    let dir = Dir::new("egress_browser_off", &[]);
    let scenario = template(
        &dir,
        "claude",
        &agent_lines("claude", "session/new", &json!({"mcpServers": []}), &[]),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &scenario, json!({}))]),
        json!({"tools": {"browser": false, "web_search": false}}),
    );
    let (proxy, log, _closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    host.shutdown();
    assert_eq!(calls(&log), ["removal"]);
}

/// A session whose Chrome is not found opens no proxy: the browser is `BrowserNotInstalled`,
/// and none of the proxy's ports are held.
#[test]
fn no_proxy_opens_without_chrome() {
    let dir = Dir::new("egress_no_chrome", &[]);
    claude(&dir, &json!([]));
    let (proxy, log, _closes) = Recording::new(false);
    let removals = Arc::clone(&log);
    let roots = crate::browser_install::roots(&dir);
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        wired(config, proxy, removals)
            .with_browser_executable(None)
            .with_chrome_roots(roots)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let browser = first_frame_tools(&host, thread)
        .into_iter()
        .find(|state| state.tool == clauseo_protocol::ClauseoTool::Browser)
        .expect("the header shows the browser");
    assert_eq!(
        browser.availability,
        clauseo_protocol::ToolAvailability::Unavailable {
            reason: ToolsUnavailableReason::BrowserNotInstalled
        }
    );
    host.shutdown();
    assert_eq!(calls(&log), ["removal"]);
}
