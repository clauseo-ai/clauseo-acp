//! S27's host side through the host: the slots (the third session on 8 GB, or the fifth on
//! 16 GB, gets no browser; a session's slot is free once its agent ends), the Chrome the launch
//! prep finds filling `{browser_executable}`, and `PageVisited { via: Browser }` from the
//! `browser-visit` scenario.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_host::browser::chrome::{ChromeRoots, Platform};
use clauseo_host::browser::{CONFIG_DIR, CONFIG_FILE};
use clauseo_host::{HostConfig, TestProxy};
use clauseo_protocol::{
    ClauseoTool, DomainEvent, PageVia, StopReason, StreamId, ToolsUnavailableReason,
};
use serde_json::json;

use clauseo_host::McpServer;

use crate::profile_rig::{
    CLAUDE, Hook, Wired, agent_lines, agents, browser_server, filled_browser, first_frame_tools,
    settings, template, turn,
};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread};
use crate::write;

const GB: u64 = 1024 * 1024 * 1024;

/// An agent id whose agent exits after its one turn, so its session ends.
const EXITING: &str = "exiting-agent";

fn browser_hook() -> Hook {
    Hook {
        servers: vec![browser_server()],
        ..Hook::default()
    }
}

/// `count` threads of `CLAUDE` in one matter, each one turn, all alive at once on a machine of
/// `memory` bytes; returns the browser state each thread's header shows.
fn browsers(test: &str, memory: u64, count: usize) -> Vec<Option<ToolsUnavailableReason>> {
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
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        with_browser(config).with_machine_memory(memory)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let threads: Vec<_> = (0..count)
        .map(|_| {
            let thread = host.create_thread(matter, CLAUDE);
            turn(&host, thread);
            thread
        })
        .collect();
    let states = threads
        .iter()
        .map(|thread| browser_reason(&first_frame_tools(&host, *thread)))
        .collect();
    host.shutdown();
    states
}

fn with_browser(config: HostConfig) -> HostConfig {
    config
        .with_profile_hook(Box::new(browser_hook()))
        .with_egress_proxy(Arc::new(TestProxy(8123)))
        .with_browser_executable(Some(PathBuf::from("/chrome")))
}

/// The browser's reason in a header's tools: `None` when it is available.
fn browser_reason(tools: &[clauseo_protocol::ToolState]) -> Option<ToolsUnavailableReason> {
    let state = tools
        .iter()
        .find(|state| state.tool == ClauseoTool::Browser)
        .expect("the header shows the browser");
    match &state.availability {
        clauseo_protocol::ToolAvailability::Available => None,
        clauseo_protocol::ToolAvailability::Unavailable { reason } => Some(*reason),
        other @ clauseo_protocol::ToolAvailability::Unknown { .. } => {
            panic!("an unknown availability: {other:?}")
        }
    }
}

#[test]
fn the_third_session_on_8_gb_gets_no_browser() {
    let too_many = Some(ToolsUnavailableReason::TooManyBrowsers);
    assert_eq!(
        browsers("browser_slots_8gb", 8 * GB, 3),
        [None, None, too_many]
    );
}

#[test]
fn the_fifth_session_on_16_gb_gets_no_browser() {
    let too_many = Some(ToolsUnavailableReason::TooManyBrowsers);
    assert_eq!(
        browsers("browser_slots_16gb", 16 * GB, 5),
        [None, None, None, None, too_many]
    );
}

/// A session holds its slot until it ends: once the first session's agent exits, two more
/// sessions get the browser on 8 GB, and a third does not.
#[test]
fn a_slot_is_free_once_its_session_ends() {
    let dir = Dir::new("browser_slot_freed", &[]);
    let claude = template(
        &dir,
        "claude",
        &agent_lines("claude", "session/new", &json!({}), &[]),
    );
    let exiting_lines = format!(
        "{}\n{}",
        agent_lines("exiting", "session/new", &json!({}), &[]),
        json!({"exit": {"code": 0}})
    );
    let exiting = template(&dir, "exiting", &exiting_lines);
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (EXITING, &exiting, json!({}))]),
        json!({"tools": {"browser": true, "web_search": false}}),
    );
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        with_browser(config).with_machine_memory(8 * GB)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let first = host.create_thread(matter, EXITING);
    let mut events = host.events(StreamId::Thread(first));
    let (first_turn, _) = host.prompt(first, "Go.");
    assert_eq!(
        events.until_turn_ended(first_turn).stop,
        StopReason::EndTurn
    );
    assert_eq!(browser_reason(&first_frame_tools(&host, first)), None);
    events.until("the first agent's session ended", |events| {
        events.count(|event| matches!(event, DomainEvent::AgentSessionDetached(_))) > 0
    });
    host.settle();
    let reasons: Vec<_> = (0..3)
        .map(|_| {
            let thread = host.create_thread(matter, CLAUDE);
            turn(&host, thread);
            browser_reason(&first_frame_tools(&host, thread))
        })
        .collect();
    host.shutdown();
    assert_eq!(
        reasons,
        [None, None, Some(ToolsUnavailableReason::TooManyBrowsers)]
    );
}

/// With no Chrome forced, the launch prep searches the roots it is given: the Chrome there fills
/// `{browser_executable}`, and with none the browser is `BrowserNotInstalled`.
#[test]
fn the_chrome_the_prep_finds_fills_the_entry() {
    let dir = Dir::new("browser_chrome_found", &[]);
    let roots_dir = dir.data().join("chrome-roots");
    let chrome = roots_dir.join("usr").join("bin").join("chromium");
    write::create_dir_all(chrome.parent().unwrap());
    write::write(&chrome, b"");
    let chrome_text = crate::profile_rig::path_text(&chrome);
    let mut entry = filled_browser();
    entry["env"][3]["value"] = json!(chrome_text);
    let claude = template(
        &dir,
        "claude",
        &agent_lines(
            "claude",
            "session/new",
            &json!({"mcpServers": [entry]}),
            &[],
        ),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({}))]),
        json!({"tools": {"browser": true, "web_search": false}}),
    );
    let roots = |root: &Path| ChromeRoots {
        platform: Platform::Linux,
        root: root.to_owned(),
        home: None,
        program_files: None,
        program_files_x86: None,
        local_app_data: None,
        app_data: root.join("app"),
    };
    let launcher = Wired::new(&dir);
    let found = roots(&roots_dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), move |config| {
        config
            .with_profile_hook(Box::new(browser_hook()))
            .with_egress_proxy(Arc::new(TestProxy(8123)))
            .with_chrome_roots(found)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    assert_eq!(browser_reason(&first_frame_tools(&host, thread)), None);
    host.shutdown();
    // The guard needs agent-browser's config file to exist: the launch prep wrote it.
    let config = dir.data_dir().join(CONFIG_DIR).join(CONFIG_FILE);
    assert_eq!(std::fs::read(&config).unwrap(), b"{}\n");

    let empty = roots(&dir.data().join("no-chrome"));
    let generic = template(
        &dir,
        "generic",
        &agent_lines("generic", "session/new", &json!({"mcpServers": []}), &[]),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &generic, json!({}))]),
        json!({"tools": {"browser": true, "web_search": false}}),
    );
    let host = dir.start_launching(1, Arc::<Wired>::clone(&launcher), move |config| {
        config
            .with_profile_hook(Box::new(browser_hook()))
            .with_egress_proxy(Arc::new(TestProxy(8123)))
            .with_chrome_roots(empty)
    });
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    assert_eq!(
        browser_reason(&first_frame_tools(&host, thread)),
        Some(ToolsUnavailableReason::BrowserNotInstalled)
    );
    host.shutdown();
}

/// The browser state one `CLAUDE` session shows with `entry` as the hook's browser entry.
fn reason_with_entry(test: &str, entry: McpServer) -> Option<ToolsUnavailableReason> {
    let dir = Dir::new(test, &[]);
    let generic = template(
        &dir,
        "generic",
        &agent_lines("generic", "session/new", &json!({"mcpServers": []}), &[]),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &generic, json!({}))]),
        json!({"tools": {"browser": true, "web_search": false}}),
    );
    let hook = Hook {
        servers: vec![entry],
        ..Hook::default()
    };
    let host = dir.start_launching(0, Wired::new(&dir), move |config| {
        config
            .with_profile_hook(Box::new(hook))
            .with_egress_proxy(Arc::new(TestProxy(8123)))
            .with_browser_executable(Some(PathBuf::from("/chrome")))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    let reason = browser_reason(&first_frame_tools(&host, thread));
    host.shutdown();
    reason
}

/// A development build has no agent-browser: when the entry's `clauseo-mcp` or agent-browser
/// file is missing, the launch prep leaves the browser out with `BrowserNotInstalled`, and the
/// session starts without it.
#[test]
fn a_missing_sidecar_leaves_the_browser_out() {
    let missing = std::env::temp_dir().join("clauseo-no-such-sidecar");
    let McpServer::Stdio {
        name,
        command,
        args,
        env,
    } = browser_server();
    let no_guard = McpServer::Stdio {
        name: name.clone(),
        command: missing.clone(),
        args: args.clone(),
        env: env.clone(),
    };
    let mut no_agent_browser_args = args;
    no_agent_browser_args[2] = missing.to_str().unwrap().to_owned();
    let no_agent_browser = McpServer::Stdio {
        name,
        command,
        args: no_agent_browser_args,
        env,
    };
    let not_installed = Some(ToolsUnavailableReason::BrowserNotInstalled);
    assert_eq!(
        reason_with_entry("browser_no_guard", no_guard),
        not_installed
    );
    assert_eq!(
        reason_with_entry("browser_no_agent_browser", no_agent_browser),
        not_installed
    );
}

/// The `clauseo-ab-*` folders and lock files in `root`.
#[cfg(unix)]
fn socket_entries(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(root)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("clauseo-ab-"))
        })
        .collect();
    found.sort();
    found
}

/// macOS and Linux: the launch prep makes one socket folder per session (mode 0700, with its
/// lock file beside it) and fills `{socket_dir}` with it; it is there while the agent runs, and
/// goes, lock file and all, once the agent's tree has exited.
#[cfg(unix)]
#[test]
fn the_socket_folder_lives_until_the_agents_tree_has_exited() {
    use std::os::unix::fs::MetadataExt;

    let dir = Dir::new("browser_socket_life", &[]);
    let sockets = dir.data().join("sockets");
    write::create_dir_all(&sockets);
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
    let McpServer::Stdio {
        name,
        command,
        args,
        mut env,
    } = browser_server();
    env.push((
        "AGENT_BROWSER_SOCKET_DIR".to_owned(),
        "{socket_dir}".to_owned(),
    ));
    let hook = Hook {
        servers: vec![McpServer::Stdio {
            name,
            command,
            args,
            env,
        }],
        ..Hook::default()
    };
    let launcher = Wired::new(&dir);
    let root = sockets.clone();
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), move |config| {
        config
            .with_profile_hook(Box::new(hook))
            .with_egress_proxy(Arc::new(TestProxy(8123)))
            .with_browser_executable(Some(PathBuf::from("/chrome")))
            .with_browser_socket_root(root)
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    let thread = host.create_thread(matter, CLAUDE);
    turn(&host, thread);
    assert_eq!(browser_reason(&first_frame_tools(&host, thread)), None);
    // The agent still runs: its folder and lock file are there.
    let live = socket_entries(&sockets);
    assert_eq!(live.len(), 2, "{live:?}");
    let folder = &live[0];
    let meta = std::fs::symlink_metadata(folder).unwrap();
    assert!(meta.is_dir());
    assert_eq!(meta.mode() & 0o7777, 0o700);
    assert_eq!(live[1], PathBuf::from(format!("{}.lock", folder.display())));
    // The shutdown stops the agent and kills its tree; then the claims go.
    host.shutdown();
    assert_eq!(socket_entries(&sockets), Vec::<PathBuf>::new());
}

/// macOS and Linux: socket folders an earlier host left (with their unlocked lock files) are
/// swept when the host starts, before any launch; a name that is not one of the host's stays.
#[cfg(unix)]
#[test]
fn stale_socket_folders_are_swept_at_start() {
    let dir = Dir::new("browser_socket_sweep", &[]);
    let sockets = dir.data().join("sockets");
    let stale = sockets.join("clauseo-ab-0123456789abcdef");
    write::create_dir_all(&stale);
    write::set_mode(&stale, 0o700);
    write::write(&stale.join("s.sock"), b"");
    let lock = sockets.join("clauseo-ab-0123456789abcdef.lock");
    write::write(&lock, b"");
    let other = sockets.join("clauseo-ab-not-ours");
    write::create_dir_all(&other);
    let host = dir.start_with(0, |config| config.with_browser_socket_root(sockets));
    assert!(
        std::fs::symlink_metadata(&stale).is_err(),
        "the stale folder went"
    );
    assert!(
        std::fs::symlink_metadata(&lock).is_err(),
        "its lock file went"
    );
    assert!(other.is_dir(), "a name not of the host's form stays");
    host.shutdown();
}

/// The `PageVisited` events of a thread's log.
fn pages(events: &crate::observe::Events) -> Vec<clauseo_protocol::PageVisited> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PageVisited(page) => Some(page.clone()),
            _ => None,
        })
        .collect()
}

/// `browser-visit`: an open of `https://sci.gov.in/judgments` that completes, a click, and an
/// open that fails. One `PageVisited { via: Browser }` with the URL, the turn and the open's tool
/// call; none for the click or the failed open; and the reopened thread replays it.
#[test]
fn a_completed_open_is_one_page_visited() {
    let dir = Dir::new("browser_visit", &[(FAKE, Scenario::Local("browser-visit"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn_id, _) = host.prompt(thread, "Find the judgment.");
    assert_eq!(events.until_turn_ended(turn_id).stop, StopReason::EndTurn);
    events.assert_no_mismatch();
    let open = events
        .events()
        .find_map(|event| match event {
            DomainEvent::ToolCallStarted(started) if started.acp_id.as_str() == "tc-open" => {
                Some(started.tool_call)
            }
            _ => None,
        })
        .expect("the open is logged");
    let logged = pages(&events);
    assert_eq!(logged.len(), 1, "{}", events.dump());
    let page = &logged[0];
    assert_eq!(page.url, "https://sci.gov.in/judgments");
    assert_eq!(
        page.title.as_deref(),
        Some("Judgments | Supreme Court of India")
    );
    assert_eq!(page.turn, turn_id);
    assert_eq!(page.tool_call, open);
    assert_eq!(page.via, PageVia::Browser);
    // The page comes after its tool call's completed update.
    let completed = events
        .position(|event| {
            matches!(event, DomainEvent::ToolCallUpdated(u) if u.tool_call == open
            && u.patch.status == Some(clauseo_protocol::ToolCallStatus::Completed))
        })
        .unwrap();
    let visited = events
        .position(|event| matches!(event, DomainEvent::PageVisited(_)))
        .unwrap();
    assert!(completed < visited, "{}", events.dump());
    host.shutdown();

    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    assert_eq!(pages(&events), logged, "the reopened thread replays it");
    host.shutdown();
}

/// The config file the host writes is the one the profile's `browser` entry names.
#[test]
fn the_config_file_is_the_one_the_entry_names() {
    let root = std::env::temp_dir().join("clauseo-browser-config-path");
    let locations = clauseo_settings::Locations::under_root(&root);
    let paths = clauseo_profile::ProfilePaths::new(
        &locations,
        root.join("app").join("clauseo"),
        root.join("stetdoc").join("stetdoc"),
        clauseo_profile::BrowserSidecars {
            clauseo_mcp: root.join("app").join("clauseo-mcp"),
            agent_browser: root.join("app").join("agent-browser"),
        },
    )
    .unwrap();
    assert_eq!(
        paths.browser_config(),
        locations.data_dir().join(CONFIG_DIR).join(CONFIG_FILE)
    );
}

/// `browser-late`: an open started in turn 1 and left running; the lawyer cancels, and its
/// completion arrives after the cancel (`Origin::Late`): the thread still holds the open, and no
/// page is written.
#[test]
fn a_late_completed_open_writes_nothing() {
    let dir = Dir::new("browser_late", &[(FAKE, Scenario::Local("browser-late"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Open it.");
    events.until("the open is logged", |events| {
        events.count(|event| matches!(event, DomainEvent::ToolCallStarted(s) if s.acp_id.as_str() == "tc-late")) == 1
    });
    assert_eq!(
        host.ok(clauseo_protocol::HostCommand::Cancel { thread }),
        clauseo_protocol::CommandOutcome::Done
    );
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    let (second, _) = host.prompt(thread, "And then?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert!(
        events.count(|event| matches!(event, DomainEvent::ToolCallStarted(s) if s.acp_id.as_str() == "tc-late")) == 1,
        "{}",
        events.dump()
    );
    assert_eq!(pages(&events), [], "{}", events.dump());
    host.shutdown();
}

/// Plays one scenario per launch, in order: the replay test's first agent, then its relaunch.
pub(crate) struct Sequence {
    pub(crate) inner: clauseo_process::PassthroughLauncher,
    pub(crate) plays: std::sync::Mutex<Vec<Scenario>>,
}

impl clauseo_process::Launcher for Sequence {
    fn launch(
        &self,
        spec: &clauseo_process::LaunchSpec,
    ) -> Result<clauseo_process::Launched, clauseo_process::Error> {
        let scenario = self.plays.lock().unwrap().remove(0);
        let spec = clauseo_process::LaunchSpec::new(
            spec.program().to_path_buf(),
            vec!["--scenario".into(), scenario.arg().into()],
            spec.env().clone(),
            spec.cwd().to_path_buf(),
        )?;
        self.inner.launch(&spec)
    }
}

/// An open left running when its agent exited, completed in the relaunched agent's replay on
/// `session/load` (`Origin::Replay`): the thread still holds the open, and no page is written.
#[test]
fn a_replayed_completion_writes_nothing() {
    let dir = Dir::new(
        "browser_replay",
        &[(FAKE, Scenario::Local("browser-replay-seed"))],
    );
    let launcher = Arc::new(Sequence {
        inner: dir.launcher(),
        plays: std::sync::Mutex::new(vec![
            Scenario::Local("browser-replay-seed"),
            Scenario::Local("browser-replay-load"),
        ]),
    });
    let host = dir.start_launching(0, launcher, |config| config);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Open it.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    events.until("the first agent's session ended", |events| {
        events.count(|event| matches!(event, DomainEvent::AgentSessionDetached(_))) > 0
    });
    let (second, _) = host.prompt(thread, "And then?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    let opened = events.count(|event| {
        matches!(event, DomainEvent::ToolCallStarted(s) if s.acp_id.as_str() == "tc-replayed")
    });
    assert_eq!(opened, 1, "{}", events.dump());
    assert_eq!(pages(&events), [], "{}", events.dump());
    host.shutdown();
}
