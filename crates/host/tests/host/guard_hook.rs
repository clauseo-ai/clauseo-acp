//! The pre-write guard's hook: with `HostConfig::with_pre_write_guard`, `session/new` and
//! `session/load` carry its `PreToolUse` entry in Claude Code's flag settings, after the
//! profile hook's own entries, and a profile hook cannot remove it; without, no hook goes. Each
//! scenario checks the request; a mismatch makes `fake_agent` exit 3 before the turn can end.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_host::{HostConfig, ProfileHook};
use clauseo_protocol::{AgentId, StopReason, StreamId};
use serde_json::{Map, Value, json};

use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// The program the scenarios expect in the entry; nothing runs it.
const PROGRAM: &str = "/opt/clauseo/clauseo-desktop";

/// A profile hook returning a fixed `_meta` and no variables.
struct Hook(Value);

impl ProfileHook for Hook {
    fn session_meta(&self, _agent: &AgentId) -> Option<Map<String, Value>> {
        self.0.as_object().cloned()
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }
}

/// Starts run `run` with the guard configured and the profile hook's `meta`.
fn start_guarded(dir: &Dir, run: u16, meta: Value) -> Running {
    dir.start_with(run, |config: HostConfig| {
        config
            .with_profile_hook(Box::new(Hook(meta)))
            .with_pre_write_guard(PathBuf::from(PROGRAM))
    })
}

/// Runs one prompt on `thread` to its end: the scenario accepted the session's request.
fn prompt_to_end(host: &Running, thread: clauseo_protocol::ThreadId, text: &str) -> String {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let (turn, _) = host.prompt(thread, text);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    events.turn_text(turn)
}

#[test]
fn the_guard_entry_goes_after_the_hooks_own_on_new_and_load() {
    let dir = Dir::new("guard_hook", &[(FAKE, Scenario::Local("guard-hook-new"))]);
    let firm =
        json!({"matcher": "Read", "hooks": [{"type": "command", "command": "/opt/firm/audit"}]});
    let meta = json!({"claudeCode": {"options": {"settings": {
        "syncClaudeAiPlugins": false,
        "hooks": {"PreToolUse": [firm]},
    }}}});
    let host = start_guarded(&dir, 0, meta);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4\n")], FAKE);
    assert_eq!(
        prompt_to_end(&host, thread, "Summarise clause 4."),
        "First answer."
    );
    host.shutdown();

    dir.set_settings(&[(FAKE, Scenario::Local("guard-hook-load"))], json!({}));
    let meta = json!({"claudeCode": {"options": {"settings": {"hooks": "none"}}}});
    let host = start_guarded(&dir, 1, meta);
    assert_eq!(
        prompt_to_end(&host, thread, "And clause 5?"),
        "Second answer."
    );
    host.shutdown();
}

#[test]
fn without_the_guard_no_hook_goes() {
    let dir = Dir::new(
        "guard_hook_none",
        &[(FAKE, Scenario::Local("guard-hook-none"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4\n")], FAKE);
    assert_eq!(prompt_to_end(&host, thread, "Summarise clause 4."), "Done.");
    host.shutdown();
}
