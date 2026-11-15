//! acp: the scanner that reads the start of a `session/update` line over the 8 MiB line cap
//! (crates/acp/src/line/oversize.rs), called directly. `acp_stdout` cannot reach it: its
//! inputs are at most 64 KiB, and the reader scans a line only once it is past the cap.
//!
//! Items driven: `clauseo_acp::fuzzing::skipped_update`, the `#[doc(hidden)]` door to the
//! scanner, and the marker line it builds.
//!
//! Input: the start of the line, as the reader holds it.
//!
//! Invariants: no panic, and a read in linear time (a hang shows as a libFuzzer timeout). When
//! the start reads as skippable: each copied id is at most `MAX_ID_BYTES` of UTF-8 with no
//! control character; the status is one of ACP's four; the marker is one JSON object naming
//! `_clauseo/skipped_update` whose params repeat what was read, `null` for what was not.
#![no_main]

use clauseo_acp::fuzzing::{MAX_ID_BYTES, skipped_update};
use libfuzzer_sys::fuzz_target;
use serde_json::{Value, json};

const STATUSES: [&str; 4] = ["pending", "in_progress", "completed", "failed"];

fn check_id(id: Option<&String>) {
    if let Some(id) = id {
        assert!(id.len() <= MAX_ID_BYTES, "an id of {} bytes", id.len());
        assert!(
            id.bytes().all(|byte| byte >= 0x20),
            "a control character in {id:?}"
        );
    }
}

fuzz_target!(|head: &[u8]| {
    let Some(read) = skipped_update(head) else {
        return;
    };
    check_id(read.session.as_ref());
    check_id(read.tool_call.as_ref());
    if let Some(status) = read.status {
        assert!(STATUSES.contains(&status), "{status}");
    }
    let marker: Value = serde_json::from_str(&read.marker).expect("the marker is JSON");
    let kind = read
        .tool_call_update
        .map(|tool| if tool { "tool_call_update" } else { "other" });
    let expected = json!({
        "jsonrpc": "2.0",
        "method": "_clauseo/skipped_update",
        "params": {
            "bytes": 9 * 1024 * 1024,
            "sessionId": read.session,
            "kind": kind,
            "toolCallId": read.tool_call,
            "status": read.status,
        },
    });
    assert_eq!(marker, expected);
});
