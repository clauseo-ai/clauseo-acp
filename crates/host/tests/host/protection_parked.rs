//! With no sandbox wired the Safe workspace is parked for launch (decision record 0051, Rohan
//! 2026-11-09): the host is a plain ACP client. `sandbox.required` refuses neither
//! `CreateThread` nor a launch, and every row carries no protection.

use clauseo_protocol::{AgentId, HostCommand, StopReason, StreamId};

use crate::protection::dir;
use crate::support::{FAKE, Scenario};

fn required(dir: &crate::support::Dir) {
    dir.set_settings(
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
        serde_json::json!({"sandbox": {"required": true, "allow_unprotected_auto": false}}),
    );
}

#[test]
fn with_no_sandbox_wired_sandbox_required_refuses_no_create_thread_and_rows_carry_none() {
    let dir = dir("protection_parked_create");
    required(&dir);
    let host = dir.start(0);
    let matter = host.create_matter("Smith", &dir.folder("smith", &[]));
    let rows = host.matters();
    assert_ne!(rows.len(), 0);
    for row in rows {
        assert_eq!(row.protection, None, "{row:?}");
    }
    let _created = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(FAKE).unwrap(),
    });
    host.shutdown();
}

#[test]
fn with_no_sandbox_wired_sandbox_required_refuses_no_launch() {
    let dir = dir("protection_parked_turn");
    let host = dir.start(0);
    let matter = host.create_matter("Smith", &dir.folder("smith", &[]));
    let thread = host.create_thread(matter, FAKE);
    host.shutdown();
    required(&dir);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    let ended = events.until_turn_ended(turn);
    assert_ne!(ended.stop, StopReason::Failed, "{}", events.dump());
    host.shutdown();
}
