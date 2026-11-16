//! A Claude launch's own environment: it carries every variable the launch sets
//! (`agent::loads::CLAUDE_LAUNCH_VARS`), recorded from the spec the fake `claude-acp` agent is
//! launched with.

use std::path::Path;
use std::sync::Arc;

use clauseo_protocol::{StopReason, StreamId, ThreadId, TurnId};

use crate::codex::{Recording, run_turn};
use crate::observe::Events;
use crate::support::{Dir, Running, Scenario};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// Imports `folder` as a matter with a Claude thread and runs one prompt.
fn turn_in(host: &Running, folder: &Path) -> (ThreadId, Events, TurnId) {
    let matter = host.create_matter("m", folder);
    let thread = host.create_thread(matter, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(host, &mut events, thread);
    (thread, events, turn)
}

/// A Claude launch's own environment carries every variable the launch sets
/// (`agent::loads::CLAUDE_LAUNCH_VARS`).
#[test]
fn a_claude_launch_sets_the_launch_variables_in_its_own_environment() {
    let dir = Dir::new(
        "claude_launch_vars",
        &[(CLAUDE, Scenario::Local("claude-mode-new"))],
    );
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(0, Arc::new(launcher), |config| config);
    let (_, events, turn) = turn_in(&host, &folder);
    assert_eq!(
        events.turn_ended(turn).unwrap().stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    host.shutdown();
    let envs = envs.lock().unwrap();
    let env = envs.first().expect("a launch");
    for (name, value) in [
        ("CLAUDE_CODE_DISABLE_CRON", "1"),
        ("CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS", "1"),
        ("CLAUDE_CODE_HARBOR_KITE", "0"),
        ("CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL", "1"),
    ] {
        let named: Vec<_> = env
            .iter()
            .filter(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
            .collect();
        assert_eq!(named.len(), 1, "{name}: {named:?}");
        assert_eq!(named[0].1, value, "{name}");
    }
}
