//! A turn the agent cancels right after the lawyer refused a permission ends `NotAllowed`
//! (private #361: Codex's only reject option, `cancel`, ends the turn). A Stop, before or after
//! a refusal, ends it `Cancelled`, and so does an agent's cancel after a later allow: only the
//! lawyer's last answer in the turn counts. The queue holds after `NotAllowed` as after `Cancelled`
//! (`queue.rs`).

use clauseo_protocol::{
    CommandOutcome, DomainEvent, HostCommand, PermissionChoice, PermissionKind, StopReason,
    StreamId,
};

use crate::codex::{CODEX, codex_dir};
use crate::permissions::{answer_from, until_first_request};
use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

#[test]
fn codex_deny_that_ends_the_turn_is_not_allowed() {
    let dir = codex_dir("codex_deny_not_allowed", "codex-deny-ends-turn");
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Write ok to a file outside the matter.");
    answer_from(&host, &mut events, thread, 0, &["cancel"]);
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::NotAllowed, "{}", events.dump());
    let resolved: Vec<PermissionChoice> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionResolved(r) => Some(r.choice.clone()),
            _ => None,
        })
        .collect();
    assert!(
        matches!(
            resolved.as_slice(),
            [PermissionChoice::Selected { option_id, kind: PermissionKind::RejectOnce }]
                if option_id.as_str() == "cancel"
        ),
        "{}",
        events.dump()
    );
    host.shutdown();
}

#[test]
fn stop_after_a_deny_is_cancelled() {
    let dir = Dir::new(
        "stop_after_a_deny_is_cancelled",
        &[(FAKE, Scenario::Bundled("permission-deny-then-stop"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the brief.");
    until_first_request(&mut events);
    answer_from(&host, &mut events, thread, 0, &["reject-once"]);
    assert_eq!(
        host.ok(HostCommand::Cancel { thread }),
        CommandOutcome::Done
    );
    let ended = events.until_turn_ended(turn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::Cancelled, "{}", events.dump());
    host.shutdown();
}

#[test]
fn only_the_last_answer_counts() {
    let dir = Dir::new(
        "only_the_last_answer_counts",
        &[(FAKE, Scenario::Bundled("permission-last-answer"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    // A refusal, then an allow: the agent's own cancel is a plain cancel.
    let (first, _) = host.prompt(thread, "Edit the brief, then read the notes.");
    answer_from(
        &host,
        &mut events,
        thread,
        0,
        &["reject-always", "allow-once"],
    );
    assert_eq!(
        events.until_turn_ended(first).stop,
        StopReason::Cancelled,
        "{}",
        events.dump()
    );
    // An allow, then a refusal (`reject_always` counts as `reject_once` does).
    let (second, _) = host.prompt(thread, "Read the notes, then edit the brief.");
    answer_from(
        &host,
        &mut events,
        thread,
        2,
        &["allow-once", "reject-always"],
    );
    assert_eq!(
        events.until_turn_ended(second).stop,
        StopReason::NotAllowed,
        "{}",
        events.dump()
    );
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    host.shutdown();
}
