//! The log line of a Codex MCP server that did not start: its name and why, none of Codex's
//! error text (which can hold a path or a command). The log is the process's global
//! subscriber, so this module holds one test: nextest runs it in a process of its own.

use clauseo_host::log_file;
use clauseo_protocol::StreamId;
use clauseo_settings::Locations;

use crate::support::{Dir, Scenario, matter_with_thread};
use crate::write;

#[test]
fn a_codex_mcp_server_that_did_not_start_is_logged_by_name_and_reason_only() {
    let dir = Dir::new(
        "codex_mcp_startup_log",
        &[("codex-acp", Scenario::Local("codex-mcp-startup-failed"))],
    );
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[], "codex-acp");
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read the papers.");
    events.until_turn_ended(turn);
    host.shutdown();

    guard.flush();
    let mut text = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        text.push_str(&std::fs::read_to_string(locations.log_dir().join(name)).unwrap_or_default());
    }
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("an MCP server failed to start"))
        .collect();
    assert_eq!(lines.len(), 3, "{text}");
    // The values only: the field names may carry the formatter's colour codes.
    assert!(lines[0].contains(r#""fails""#), "{lines:?}");
    assert!(!lines[0].contains("timed_out"), "{lines:?}");
    assert!(lines[1].contains(r#""silent""#), "{lines:?}");
    assert!(lines[1].contains("timed_out"), "{lines:?}");
    assert!(lines[2].contains(r#""silent""#), "{lines:?}");
    for needle in [
        "handshaking",
        "config.toml",
        "startup_timeout_sec",
        "forwarded",
    ] {
        assert!(!text.contains(needle), "the log holds {needle:?}:\n{text}");
    }
}
