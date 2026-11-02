//! A Codex agent (`codex-acp`, played by the `codex-*` scenarios): its ask mode before the first
//! prompt, its launch variables, `session/resume`, the guard's trust check, and its file
//! changes, permissions, limit and sign-in failures through the host. The `.codex` refusal is in
//! `codex_folder.rs`, `ListAgents` in `list_agents.rs` and the plans in `codex_plans.rs`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher};
use clauseo_protocol::{
    DomainEvent, ErrorCode, FileChoice, FileDecision, HostCommand, LimitStopReason, MatterPath,
    PermissionChoice, Seq, StopReason, StreamId, ThreadId, TurnFailed, TurnId,
};
use serde_json::{Map, Value, json};

use crate::observe::Events;
use crate::permissions::answer_from;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// The agent id that makes an agent Codex.
pub(crate) const CODEX: &str = "codex-acp";

/// The text `codex-handshake` answers its one prompt with.
pub(crate) const HANDSHAKE_TEXT: &str = "Codex here. Ready to work on the matter.";

/// A launcher that keeps each spec's environment, then launches it as the tests' own does.
pub(crate) struct Recording {
    inner: PassthroughLauncher,
    pub(crate) envs: Arc<Mutex<Vec<BTreeMap<OsString, OsString>>>>,
}

impl Recording {
    pub(crate) fn new(dir: &Dir) -> Self {
        Self {
            inner: dir.launcher(),
            envs: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Launcher for Recording {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        self.envs.lock().unwrap().push(spec.env().clone());
        self.inner.launch(spec)
    }
}

/// A folder whose `codex-acp` plays bundled `scenario`.
pub(crate) fn codex_dir(test: &str, scenario: &'static str) -> Dir {
    Dir::new(test, &[(CODEX, Scenario::Bundled(scenario))])
}

/// Sends `prompt` on `thread` and reads the thread's events until the turn ends, then drains
/// what a settle brings.
pub(crate) fn run_turn(host: &Running, events: &mut Events, thread: ThreadId) -> TurnId {
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    turn
}

/// `turn`'s `TurnFailed`, when logged.
pub(crate) fn turn_failed(events: &Events, turn: TurnId) -> Option<TurnFailed> {
    events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.clone()),
        _ => None,
    })
}

/// Opens a thread of `agent` in a fresh matter and runs one turn; returns its events and turn.
fn one_turn(dir: &Dir, host: &Running, agent: &str) -> (ThreadId, Events, TurnId) {
    let files = [
        ("brief.txt", "Clause 4 draft\n"),
        ("old.txt", "Old notes\n"),
    ];
    let (_, thread, _) = matter_with_thread(dir, host, "m", &files, agent);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(host, &mut events, thread);
    (thread, events, turn)
}

/// Asserts `turn` failed with `code` before any prompt went: no text, no mismatch.
fn assert_failed_before_prompt(events: &Events, turn: TurnId, code: ErrorCode) -> TurnFailed {
    events.assert_no_mismatch();
    let failed = turn_failed(events, turn)
        .unwrap_or_else(|| panic!("no TurnFailed of {turn}: {}", events.dump()));
    assert_eq!(failed.code, code, "{}", events.dump());
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::Failed);
    assert_eq!(events.turn_text(turn), "", "the agent answered a prompt");
    failed
}

/// The last `ModeChanged`'s mode.
fn last_mode(events: &Events) -> Option<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ModeChanged(changed) => Some(changed.mode_id.to_string()),
            _ => None,
        })
        .last()
}

#[test]
fn codex_handshake_streams_in_workspace_write() {
    let dir = codex_dir("codex_handshake", "codex-handshake");
    let host = dir.start(0);
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), HANDSHAKE_TEXT);
    assert_eq!(
        last_mode(&events).as_deref(),
        Some("workspace-write"),
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// What a settings `env` sets for each agent: the four names the host forces on Codex, then
/// the four it drops for Codex (in mixed case).
fn greedy_env() -> Value {
    json!({
        "INITIAL_AGENT_MODE": "agent-full-access",
        "CODEX_HOME": "/elsewhere",
        "CODEX_CONFIG": r#"{"features":{"apps":true},"model":"m"}"#,
        "DISABLE_MCP_CONFIG_FILTERING": "false",
        "codex_path": "/elsewhere/codex",
        "App_Server_Logs": "/elsewhere/logs",
        "DEFAULT_AUTH_REQUEST": "api-key",
        "model_provider": "other",
    })
}

/// Rewrites `settings.json` with `env` on every agent.
fn set_env_on_every_agent(dir: &Dir, env: &Value) {
    let mut settings: Value = serde_json::from_str(&dir.settings_json()).unwrap();
    for (_, agent) in settings["agents"].as_object_mut().unwrap() {
        agent["env"] = env.clone();
    }
    let text = serde_json::to_string_pretty(&settings).unwrap();
    crate::write::write(&dir.data().join("settings.json"), text.as_bytes());
}

/// The value of `name` in `env`, as text.
fn var(env: &BTreeMap<OsString, OsString>, name: &str) -> Option<String> {
    env.get(&OsString::from(name))
        .map(|value| value.to_string_lossy().into_owned())
}

#[test]
fn codex_launch_variables_beat_the_settings_env() {
    let dir = Dir::new(
        "codex_launch_env",
        &[
            (CODEX, Scenario::Bundled("codex-handshake")),
            (FAKE, Scenario::Bundled("prompt-stream")),
        ],
    );
    set_env_on_every_agent(&dir, &greedy_env());
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| config);
    let (matter, codex_thread, _) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(codex_thread));
    let turn = run_turn(&host, &mut events, codex_thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    let fake_thread = host.create_thread(matter, FAKE);
    let mut events = host.events(StreamId::Thread(fake_thread));
    let turn = run_turn(&host, &mut events, fake_thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    host.shutdown();

    let envs = envs.lock().unwrap();
    let [codex, fake] = envs.as_slice() else {
        panic!("two launches, not {}", envs.len());
    };
    let home = dir.data().join("agents").join(CODEX).join("home");
    assert_eq!(
        var(codex, "INITIAL_AGENT_MODE").as_deref(),
        Some("workspace-write")
    );
    assert_eq!(var(codex, "CODEX_HOME"), Some(home.display().to_string()));
    assert_eq!(
        var(codex, "DISABLE_MCP_CONFIG_FILTERING").as_deref(),
        Some("true")
    );
    let config: Value = serde_json::from_str(&var(codex, "CODEX_CONFIG").unwrap()).unwrap();
    assert_eq!(
        config,
        json!({
            "model": "m",
            "features": {"apps": false, "plugins": false, "skill_mcp_dependency_install": false},
            "cloud": {"skills": {"enabled": false}},
            "skills": {"bundled": {"enabled": false}, "include_instructions": false},
        })
    );
    for dropped in [
        "CODEX_PATH",
        "APP_SERVER_LOGS",
        "DEFAULT_AUTH_REQUEST",
        "MODEL_PROVIDER",
    ] {
        let found = codex.keys().find(|name| name.eq_ignore_ascii_case(dropped));
        assert_eq!(found, None, "{dropped} reached Codex");
    }
    assert!(std::fs::symlink_metadata(&home).unwrap().is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&home).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{mode:o}");
    }

    // Any other agent keeps what the settings give it.
    for (name, value) in greedy_env().as_object().unwrap() {
        assert_eq!(var(fake, name).as_deref(), value.as_str(), "{name}");
    }
}

#[test]
fn codex_in_mode_agent_is_switched_before_the_first_prompt() {
    let dir = codex_dir("codex_mode_agent", "codex-mode-agent");
    let host = dir.start(0);
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), "In workspace-write.");
    assert_eq!(last_mode(&events).as_deref(), Some("workspace-write"));
    host.shutdown();
}

#[test]
fn codex_without_workspace_write_fails_before_any_prompt() {
    let dir = codex_dir("codex_no_workspace_write", "codex-no-workspace-write");
    let host = dir.start(0);
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    assert_failed_before_prompt(&events, turn, ErrorCode::AgentProtocolError);
    host.shutdown();
}

/// The `Notice` texts logged.
fn notices(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.text.clone()),
            _ => None,
        })
        .collect()
}

/// The sessions of the `AgentSessionAttached`s logged.
fn attached(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::AgentSessionAttached(a) => Some(a.session.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

/// Run 0 plays `codex-handshake`; run 1 plays `codex-resume`, which accepts only
/// `session/resume` of `sess-1` (a `session/new` or `session/load` is a mismatch).
#[test]
fn codex_reopens_with_session_resume() {
    let dir = codex_dir("codex_resume", "codex-handshake");
    let host = dir.start(0);
    let (thread, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    let head = events.head();
    host.shutdown();

    dir.set_agents(&[(CODEX, Scenario::Bundled("codex-resume"))]);
    let host = dir.start(1);
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let second = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(second).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(second), "Resumed.");
    assert_eq!(attached(&events), ["sess-1"], "{}", events.dump());
    assert_eq!(last_mode(&events).as_deref(), Some("workspace-write"));
    assert_eq!(notices(&events), Vec::<String>::new(), "{}", events.dump());
    // Nothing replayed: run 1 logged no text but the second turn's.
    let text = events.count(|e| matches!(e, DomainEvent::TextAppended(t) if t.turn != second));
    assert_eq!(text, 0, "{}", events.dump());
    host.shutdown();
}

/// An agent offering neither `session/load` nor `session/resume` gets `session/new` on reopen.
#[test]
fn an_agent_with_neither_load_nor_resume_gets_session_new() {
    let dir = Dir::new(
        "neither_load_nor_resume",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (thread, events, turn) = one_turn(&dir, &host, FAKE);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    let head = events.head();
    host.shutdown();

    dir.set_agents(&[(FAKE, Scenario::Local("new-without-load"))]);
    let host = dir.start(1);
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let second = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(second).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(second), "New session.");
    assert_eq!(attached(&events), ["sess-2"], "{}", events.dump());
    assert_eq!(
        notices(&events).len(),
        1,
        "context not restored: {}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn codex_file_changes_undo() {
    let dir = codex_dir("codex_files_undo", "codex-files");
    let host = dir.start(0);
    let files = [
        ("brief.txt", "Clause 4 draft\n"),
        ("old.txt", "Old notes\n"),
    ];
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &files, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    let ended = events.turn_ended(turn).unwrap();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(ended.checkpoint_after.is_some());
    let mut changed: Vec<String> = events
        .files_changed(turn)
        .unwrap_or_else(|| panic!("no FilesChanged: {}", events.dump()))
        .files
        .iter()
        .map(|f| f.path().as_str().to_owned())
        .collect();
    changed.sort();
    assert_eq!(changed, ["brief.txt", "notes/a.txt", "old.txt"]);

    let paths = ["brief.txt", "notes/a.txt", "old.txt"].map(|p| MatterPath::new(p).unwrap());
    let decisions = paths.iter().map(|path| FileDecision {
        path: path.clone(),
        choice: FileChoice::Undo,
    });
    host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: decisions.collect(),
    });
    events.until("StepUndone", |e| {
        e.position(|event| matches!(event, DomainEvent::StepUndone(_)))
            .is_some()
    });
    let read = |path: &str| std::fs::read_to_string(folder.join(path)).ok();
    assert_eq!(read("brief.txt").as_deref(), Some("Clause 4 draft\n"));
    assert_eq!(read("old.txt").as_deref(), Some("Old notes\n"));
    assert!(
        !folder.join("notes/a.txt").exists(),
        "the undo left notes/a.txt"
    );
    host.shutdown();
}

#[test]
fn codex_permissions_allow_then_decline() {
    let dir = codex_dir("codex_permission", "codex-permission");
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the brief and list the folder.");
    answer_from(&host, &mut events, thread, 0, &["allow_once", "decline"]);
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let resolved: Vec<String> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionResolved(r) => match &r.choice {
                PermissionChoice::Selected { option_id, .. } => Some(option_id.to_string()),
                other => Some(format!("{other:?}")),
            },
            _ => None,
        })
        .collect();
    assert_eq!(resolved, ["allow_once", "decline"], "{}", events.dump());
    host.shutdown();
}

#[test]
fn codex_air_limit_ends_the_turn_at_the_limit() {
    let dir = codex_dir("codex_limit_air", "codex-limit-air");
    let host = dir.start(0);
    let (_, mut events, turn) = one_turn(&dir, &host, CODEX);
    let is_stop = |e: &DomainEvent| matches!(e, DomainEvent::TurnStoppedAtLimit(_));
    events.until("TurnStoppedAtLimit", |e| e.position(is_stop).is_some());
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::AtLimit);
    let stop_at = events.position(is_stop).unwrap();
    let Some(DomainEvent::TurnStoppedAtLimit(stop)) = events.seen.get(stop_at).map(|e| &e.event)
    else {
        unreachable!()
    };
    assert_eq!(
        (stop.turn, stop.reason),
        (turn, LimitStopReason::PlanWindow)
    );
    // No reset time: the end commit holds `TurnEnded { AtLimit }` alone, and the stop follows
    // in a commit of its own once the read is done (the harness's reader fails at once, so it
    // has no window or reset time).
    assert_eq!((&stop.window, stop.resets_at), (&None, None));
    let ended_at = events
        .position(|e| matches!(e, DomainEvent::TurnEnded(_)))
        .unwrap();
    assert_eq!(stop_at, ended_at + 1, "{}", events.dump());
    assert_ne!(
        events.batch[ended_at], events.batch[stop_at],
        "the stop rode in the end commit"
    );
    host.settle();
    events.drain_ready();
    assert_eq!(events.count(is_stop), 1, "{}", events.dump());
    host.shutdown();
}

#[test]
fn codex_not_signed_in_fails_the_turn_with_auth_required() {
    let dir = codex_dir("codex_auth_required", "codex-auth-required");
    let host = dir.start(0);
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    assert_failed_before_prompt(&events, turn, ErrorCode::AgentAuthRequired);
    host.shutdown();
}

/// A small hooks object, as the codex hooks seam would give.
fn hooks() -> Map<String, Value> {
    json!({"PreToolUse": [{"hooks": [{"type": "command", "command": "/bin/guard"}]}]})
        .as_object()
        .cloned()
        .unwrap()
}

#[test]
fn codex_guard_trusted_runs_the_turn() {
    let dir = codex_dir("codex_hook_trusted", "codex-hook-trusted");
    let host = dir.start_with(0, |config| config.with_codex_hooks(hooks()));
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), "Guarded.");
    // The hooks went through CODEX_CONFIG only: the home's config.toml has no hooks table, or
    // the guard's session-flags trust key would not match.
    let written = std::fs::read_to_string(
        dir.data_dir()
            .join("agents")
            .join(CODEX)
            .join("home")
            .join("config.toml"),
    )
    .unwrap();
    assert!(written.contains("features.plugins = false"), "{written}");
    assert!(!written.contains("hooks"), "{written}");
    host.shutdown();
}

/// With only the guard program set (as the desktop app sets it), the codex hooks seam gives
/// `codex_hook_config`'s real value: `CODEX_CONFIG.hooks.PreToolUse` holds exactly one group, the
/// guard's, and the session start passes the trust check with it.
#[test]
fn codex_launch_carries_the_real_guard_hook_and_passes_the_trust_check() {
    const PROGRAM: &str = "/opt/clauseo/clauseo-desktop";
    let dir = codex_dir("codex_real_guard", "codex-hook-trusted");
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| {
        config.with_pre_write_guard(std::path::PathBuf::from(PROGRAM))
    });
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), "Guarded.");
    host.shutdown();

    let envs = envs.lock().unwrap();
    let [codex] = envs.as_slice() else {
        panic!("one launch, not {}", envs.len());
    };
    let config: Value = serde_json::from_str(&var(codex, "CODEX_CONFIG").unwrap()).unwrap();
    let expected = clauseo_host::codex_hook_config(std::path::Path::new(PROGRAM)).unwrap();
    assert_eq!(config["hooks"], expected);
    let groups = config["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(groups[0]["matcher"], "apply_patch|Write|Edit|Bash");
    assert_eq!(
        groups[0]["hooks"][0]["command"],
        format!("'{PROGRAM}' --pre-write-guard")
    );
}

#[test]
fn codex_guard_untrusted_fails_before_any_prompt() {
    let dir = codex_dir("codex_hook_untrusted", "codex-hook-untrusted");
    let host = dir.start_with(0, |config| config.with_codex_hooks(hooks()));
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    assert_failed_before_prompt(&events, turn, ErrorCode::AgentLaunchFailed);
    host.shutdown();
}

/// Without a hooks object the trust check is skipped: `codex-handshake` never expects
/// `_codex/hooks/list`, so a call to it would be a mismatch.
#[test]
fn codex_without_hooks_skips_the_trust_check() {
    let dir = codex_dir("codex_no_hooks", "codex-handshake");
    let host = dir.start(0);
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), HANDSHAKE_TEXT);
    host.shutdown();
}
