//! A Claude agent that refuses `session/new` (claude-agent-acp 0.89.1's `cwd` check, captured on
//! a real run): the launch fails with `matter.folder_permission`, and the log file names the
//! request's method, the error's code, its message's length and its data's keys, then the
//! start-phase lines of the agent's stderr and its last line, and holds neither the message, the
//! data's value nor any other stderr line (decision record 0047).
//!
//! The log is the process's global subscriber, so this module holds one test: nextest runs it
//! in a process of its own.

use clauseo_host::log_file;
use clauseo_protocol::{DomainEvent, ErrorCode, StreamId};
use clauseo_settings::Locations;

use crate::support::{Dir, Scenario, matter_with_thread};
use crate::write;

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

fn read_logs(dir: &std::path::Path) -> String {
    ["host.2.log", "host.1.log", "host.log"]
        .iter()
        .map(|name| std::fs::read_to_string(dir.join(name)).unwrap_or_default())
        .collect()
}

#[test]
fn a_refused_start_logs_its_method_error_shape_and_start_phases() {
    let dir = Dir::new(
        "start_refused_log",
        &[(CLAUDE, Scenario::Local("claude-new-refused-cwd"))],
    );
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Summarise clause 4.");
    events.until_turn_ended(turn);
    let failed = events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
        _ => None,
    });
    assert_eq!(
        failed,
        Some(ErrorCode::MatterFolderPermission),
        "{}",
        events.dump()
    );
    host.shutdown();
    drop(guard);

    let text = read_logs(&locations.log_dir());
    let wanted = [
        "the agent did not start: it answered a startup request with an error",
        "method=session/new",
        "code=-32602",
        "message_len=",
        "data=keys [cwd]",
        "phase=validate-cwd durationMs=1 totalMs=1 | the start ended",
    ];
    for want in wanted {
        assert!(text.contains(want), "no {want:?} in the log:\n{text}");
    }
    for never in ["Quarrington", "Zanzibar", "Invalid params", "/Users/lawyer"] {
        assert!(!text.contains(never), "{never:?} in the log:\n{text}");
    }
}
