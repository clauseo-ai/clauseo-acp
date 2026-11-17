//! Codex's guard hook and the conversation's own sessions: the host lists the session's id in
//! the ids file of its guard folder (`<data>/browser/guard/<session>/sessions.json`) once
//! `session/new`, `session/resume` or `session/load` returns, before the first prompt, so the
//! guard answers only the conversation's own sessions; then the launch's self-test runs the
//! hook's command, which must refuse a call from a session no conversation has and allow a
//! harmless call from the conversation's own, or the launch fails. The ids file goes with the
//! guard folder when the run ends.

#[cfg(unix)]
use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;
#[cfg(unix)]
use clauseo_protocol::{Seq, StopReason, StreamId};
#[cfg(unix)]
use serde_json::{Map, Value, json};

#[cfg(unix)]
use crate::codex::run_turn;
use crate::codex::{CODEX, assert_failed_before_prompt, codex_dir, hooks, one_turn};
#[cfg(unix)]
use crate::support::{Dir, Scenario};

/// The fake agent's bundled scenarios.
#[cfg(unix)]
const SCENARIOS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fake_agent/scenarios");

/// Writes `<root>/<name>.sh`, a hook command that reads its call and then runs `body`, as an
/// executable script; returns its path.
#[cfg(unix)]
fn script(dir: &Dir, name: &str, body: &str) -> PathBuf {
    let file = dir.home().join(format!("{name}.sh"));
    let text = format!("#!/bin/sh\n/bin/cat >/dev/null\n{body}\n");
    crate::write::write(&file, text.as_bytes());
    crate::write::make_executable(&file);
    file
}

/// A hook command that copies the session's ids file to `log`, then answers as the guard does
/// for the self-test's two calls: refuses (exit 2, a line on stderr) the call from the session
/// no conversation has, and allows (exit 0) any other.
#[cfg(unix)]
fn copying_guard(dir: &Dir, log: &Path) -> PathBuf {
    let file = dir.home().join("copying-guard.sh");
    let text = format!(
        "#!/bin/sh\ninput=$(/bin/cat)\n/bin/cat '{}'/browser/guard/*/sessions.json > '{}'\n\
         case \"$input\" in *'\"session_id\":\"clauseo-self-test\"'*) echo refused >&2; exit 2;; \
         esac\nexit 0\n",
        dir.data_dir().display(),
        log.display()
    );
    crate::write::write(&file, text.as_bytes());
    crate::write::make_executable(&file);
    file
}

/// Codex's hooks object with `script` as the one `PreToolUse` command, before every tool.
#[cfg(unix)]
fn hooks_running(script: &Path) -> Map<String, Value> {
    let command = format!("'{}'", script.display());
    json!({"PreToolUse": [{"matcher": "*", "hooks": [{"type": "command", "command": command}]}]})
        .as_object()
        .cloned()
        .unwrap()
}

/// The ids the self-test's copy in `log` holds.
#[cfg(unix)]
fn logged_ids(log: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(log).unwrap_or_else(|_| panic!("no copy at {log:?}"));
    let file: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("not JSON: {text:?}"));
    assert_eq!(file["version"], 1, "{text}");
    serde_json::from_value(file["sessions"].clone()).unwrap_or_else(|_| panic!("{text:?}"))
}

/// Every `sessions.json` left under `<data>/browser/guard`.
#[cfg(unix)]
fn ids_files_left(dir: &Dir) -> Vec<PathBuf> {
    crate::guard_record::files_under(&dir.data_dir().join("browser").join("guard"))
        .into_iter()
        .filter(|file| file.ends_with("sessions.json"))
        .collect()
}

/// `codex-hook-trusted`'s `_codex/hooks/list` call and its answer, two scenario lines.
#[cfg(unix)]
fn hooks_list_lines() -> String {
    let trusted = std::fs::read_to_string(format!("{SCENARIOS}/codex-hook-trusted.jsonl")).unwrap();
    let lines: Vec<&str> = trusted
        .lines()
        .filter(|line| line.contains("\"as\":\"hooks\"") || line.contains("\"to\":\"hooks\""))
        .collect();
    assert_eq!(lines.len(), 2, "{trusted}");
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Writes `<data>/scenarios/<name>.jsonl` from `text` and returns it as a scenario.
#[cfg(unix)]
fn written(dir: &Dir, name: &str, text: &str) -> Scenario {
    let file = dir.data().join("scenarios").join(format!("{name}.jsonl"));
    crate::write::write(&file, text.as_bytes());
    Scenario::Written(file)
}

/// `codex-resume` with the hooks list after its `session/set_mode` and before its prompt.
#[cfg(unix)]
fn resume_with_hooks(dir: &Dir) -> Scenario {
    let bundled = std::fs::read_to_string(format!("{SCENARIOS}/codex-resume.jsonl")).unwrap();
    let cut = bundled
        .find("{\"expect\":{\"method\":\"session/prompt\"")
        .expect("codex-resume prompts");
    // The scenario's name is its file's stem.
    let mut text = bundled[..cut].replacen(
        "\"scenario\":\"codex-resume\"",
        "\"scenario\":\"guard-sessions-resume\"",
        1,
    );
    text.push_str(&hooks_list_lines());
    text.push_str(&bundled[cut..]);
    written(dir, "guard-sessions-resume", &text)
}

/// Codex reopened with `session/load` of `sess-1` (its handshake offers `loadSession`, and the
/// host loads before it resumes): no replay, the ask mode, the hooks list, one prompt answered
/// "Loaded.".
#[cfg(unix)]
fn load_with_hooks(dir: &Dir) -> Scenario {
    let handshake =
        std::fs::read_to_string(format!("{SCENARIOS}/common/codex-handshake.jsonl")).unwrap();
    let mut text = json!({"scenario": "guard-sessions-load",
        "description": "Codex reopened with session/load, then the guard's hooks list",
        "on_stdin_close": "exit"})
    .to_string();
    text.push('\n');
    // The handshake's initialize call and answer, which offer `loadSession`.
    for line in handshake.lines().take(2) {
        text.push_str(line);
        text.push('\n');
    }
    let modes = json!({"currentModeId": "agent", "availableModes": [
        {"id": "read-only", "name": "Read-only"},
        {"id": "workspace-write", "name": "Workspace access"},
        {"id": "agent", "name": "Auto review"},
        {"id": "agent-full-access", "name": "Full access"}]});
    let steps = [
        json!({"expect": {"method": "session/load", "as": "load",
            "params": {"sessionId": "sess-1", "cwd": "{{any}}"}}}),
        json!({"respond": {"to": "load", "result": {"modes": modes}}}),
        json!({"expect": {"method": "session/set_mode", "as": "set-mode",
            "params": {"sessionId": "sess-1", "modeId": "workspace-write"}}}),
        json!({"respond": {"to": "set-mode", "result": {}}}),
    ];
    for step in steps {
        text.push_str(&step.to_string());
        text.push('\n');
    }
    text.push_str(&hooks_list_lines());
    for step in [
        json!({"expect": {"method": "session/prompt", "as": "p1",
            "params": {"sessionId": "sess-1"}}}),
        json!({"update": {"session": "sess-1", "update": {
            "sessionUpdate": "agent_message_chunk", "messageId": "m-answer-1",
            "content": {"type": "text", "text": "Loaded."}}}}),
        json!({"respond": {"to": "p1", "result": {"stopReason": "end_turn"}}}),
    ] {
        text.push_str(&step.to_string());
        text.push('\n');
    }
    written(dir, "guard-sessions-load", &text)
}

/// Run 0: `codex-hook-trusted` (session `sess-1`) under the copying guard with the self-test
/// on; asserts the turn ran, the copy holds exactly `sess-1`, and no ids file is left after the
/// shutdown. Returns the thread and the stream's head.
#[cfg(unix)]
fn first_run(dir: &Dir, log: &Path) -> (clauseo_protocol::ThreadId, u64) {
    let hooks = hooks_running(&copying_guard(dir, log));
    let host = dir.start_with(0, |config| {
        config.with_codex_hooks(hooks).with_guard_self_test(true)
    });
    let (thread, events, turn) = one_turn(dir, &host, CODEX);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_ended(turn).unwrap().stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert_eq!(events.turn_text(turn), "Guarded.");
    // The self-test ran after the id went in and before the prompt was sent.
    assert_eq!(logged_ids(log), ["sess-1"]);
    let head = events.head();
    host.shutdown();
    assert_eq!(ids_files_left(dir), Vec::<PathBuf>::new());
    (thread, head)
}

/// Run 1 of `dir` playing `scenario` under the copying guard: its one turn ends with `text`,
/// and the self-test's copy, made in this run, holds exactly `sess-1`.
#[cfg(unix)]
fn reopened_run(
    dir: &Dir,
    log: &Path,
    (thread, head): (clauseo_protocol::ThreadId, u64),
    scenario: Scenario,
    text: &str,
) {
    crate::write::remove_file(log);
    dir.set_agents(&[(CODEX, scenario)]);
    let hooks = hooks_running(&copying_guard(dir, log));
    let host = dir.start_with(1, |config| {
        config.with_codex_hooks(hooks).with_guard_self_test(true)
    });
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_ended(turn).unwrap().stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert_eq!(events.turn_text(turn), text);
    assert_eq!(logged_ids(log), ["sess-1"]);
    host.shutdown();
    assert_eq!(ids_files_left(dir), Vec::<PathBuf>::new());
}

#[cfg(unix)]
#[test]
fn the_session_id_is_in_the_ids_file_before_the_first_prompt() {
    let dir = codex_dir("guard_sessions_new", "codex-hook-trusted");
    let log = dir.home().join("ids.log");
    first_run(&dir, &log);
}

#[cfg(unix)]
#[test]
fn resume_keeps_the_session_id() {
    let dir = codex_dir("guard_sessions_resume", "codex-hook-trusted");
    let log = dir.home().join("ids.log");
    let run = first_run(&dir, &log);
    let scenario = resume_with_hooks(&dir);
    reopened_run(&dir, &log, run, scenario, "Resumed.");
}

#[cfg(unix)]
#[test]
fn load_keeps_the_session_id() {
    let dir = codex_dir("guard_sessions_load", "codex-hook-trusted");
    let log = dir.home().join("ids.log");
    let run = first_run(&dir, &log);
    let scenario = load_with_hooks(&dir);
    reopened_run(&dir, &log, run, scenario, "Loaded.");
}

/// A guard program that is not there fails the self-test, and so the launch.
#[test]
fn a_missing_guard_refuses_the_codex_launch() {
    let dir = codex_dir("guard_sessions_missing", "codex-hook-trusted");
    let missing = dir.home().join("no-such-guard");
    let host = dir.start_with(0, |config| {
        config
            .with_pre_write_guard(missing)
            .with_guard_self_test(true)
    });
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    assert_failed_before_prompt(&events, turn, ErrorCode::AgentGuardSelftestFailed);
    host.shutdown();
}

/// A hook that lets the self-test's call go (exit 0) fails the launch.
#[cfg(unix)]
#[test]
fn a_guard_that_lets_calls_go_refuses_the_codex_launch() {
    let dir = codex_dir("guard_sessions_lets_go", "codex-hook-trusted");
    let hooks = hooks_running(&script(&dir, "lets-go", "exit 0"));
    let host = dir.start_with(0, |config| {
        config.with_codex_hooks(hooks).with_guard_self_test(true)
    });
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    assert_failed_before_prompt(&events, turn, ErrorCode::AgentGuardSelftestFailed);
    host.shutdown();
}

/// A hook that refuses every call, the conversation's own harmless one too, fails the launch.
#[cfg(unix)]
#[test]
fn a_guard_that_refuses_every_call_refuses_the_codex_launch() {
    let dir = codex_dir("guard_sessions_refuses_all", "codex-hook-trusted");
    let hooks = hooks_running(&script(&dir, "refuses-all", "echo refused >&2\nexit 2"));
    let host = dir.start_with(0, |config| {
        config.with_codex_hooks(hooks).with_guard_self_test(true)
    });
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    assert_failed_before_prompt(&events, turn, ErrorCode::AgentGuardSelftestFailed);
    host.shutdown();
}

#[test]
fn codex_guard_untrusted_fails_before_any_prompt() {
    let dir = codex_dir("codex_hook_untrusted", "codex-hook-untrusted");
    let host = dir.start_with(0, |config| config.with_codex_hooks(hooks()));
    let (_, events, turn) = one_turn(&dir, &host, CODEX);
    assert_failed_before_prompt(&events, turn, ErrorCode::AgentLaunchFailed);
    host.shutdown();
}
