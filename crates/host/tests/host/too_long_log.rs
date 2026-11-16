//! A turn the model refuses as too long logs no protocol warning: the AIR `sessionFailure` on the
//! `session_info_update` and on `end_turn` is read, not skipped as unreadable. Before decision
//! 0057 Codex's full context was logged as a `session_failure` warning and nothing else.
//!
//! The log is the process's global subscriber, so this module holds one test: nextest runs it
//! in a process of its own.

use clauseo_host::log_file;
use clauseo_protocol::{StopReason, StreamId};
use clauseo_settings::Locations;

use crate::codex::CODEX;
use crate::limits_host::CLAUDE;
use crate::support::{Dir, Scenario, matter_with_thread};
use crate::write;

#[test]
fn a_too_long_turn_logs_no_protocol_warning() {
    let dir = Dir::new(
        "too_long_log",
        &[
            (CODEX, Scenario::Bundled("codex-context-full")),
            (CLAUDE, Scenario::Bundled("claude-prompt-too-long")),
        ],
    );
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");
    let host = dir.start(0);
    let (matter, codex, _) = matter_with_thread(&dir, &host, "m", &[("lease.txt", "x\n")], CODEX);
    let claude = host.create_thread(matter, CLAUDE);
    for thread in [codex, claude] {
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Read the whole lease.");
        assert_eq!(events.until_turn_ended(turn).stop, StopReason::TooLong);
    }
    host.settle();
    host.shutdown();
    drop(guard);
    let mut log = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        log.push_str(&std::fs::read_to_string(locations.log_dir().join(name)).unwrap_or_default());
    }
    assert!(
        log.contains("clauseo_host::lifecycle: prompt sent"),
        "{log}"
    );
    assert!(
        !log.contains("the agent sent something unreadable"),
        "{log}"
    );
    assert!(!log.contains("session_failure"), "{log}");
}
