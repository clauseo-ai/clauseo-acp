//! A steer sent into a turn before the lawyer withdraws the conversation's read of another matter
//! never reads that matter: the withdrawal stops the turn at once (`session/cancel`), the steer
//! the agent never answered is not sent, one notice names the matter, and nothing more reaches
//! the old agent (its scenario holds no other prompt).

use std::time::Duration;

use clauseo_protocol::{
    DeliveryRequest, DomainEvent, HostCommand, NoticeKind, SteerOutcome, StopReason, StreamId,
};

use crate::queue::{DURABLE, send, settle, steer_resolved};
use crate::support::{Dir, FAKE, Scenario, text};

/// How long the host waits for a steer's answer.
const STEER_WAIT: Duration = Duration::from_secs(30);

#[test]
fn a_steer_sent_before_a_withdrawal_does_not_read_the_matter_after() {
    let dir = Dir::new(
        "withdraw_steer",
        &[(FAKE, Scenario::Local("withdraw-steer"))],
    );
    let folder = dir.folder("m", &[("a.txt", "a")]);
    let smith = dir.folder("Smith", &[("lease.pdf", "x")]);
    let host = dir.start(0);
    let matter = host.create_matter("m", &folder);
    let other = host.create_matter("Smith", &smith);
    let thread = host.create_thread(matter, FAKE);
    host.ok(HostCommand::AllowMatterRead {
        thread,
        matter: other,
    });
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, "Summarise the lease.");
    host.wait_armed(DURABLE);
    let (turn, message) = send(
        &host,
        thread,
        text("Read Smith's lease"),
        DeliveryRequest::Steer,
    );
    assert_eq!(turn, first, "the steer joins the running turn");
    host.wait_armed(STEER_WAIT);
    host.ok(HostCommand::WithdrawMatterRead {
        thread,
        matter: other,
    });
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Cancelled);
    events.until("the steer's settling", |e| {
        steer_resolved(e, message).is_some()
    });
    settle(&host, &mut events);
    let (_, resolved) = steer_resolved(&events, message).expect("the steer is settled");
    assert!(
        matches!(resolved.outcome, SteerOutcome::NotSent { .. }),
        "{}",
        events.dump()
    );
    let stopped = events.events().filter(|event| {
        matches!(event, DomainEvent::Notice(notice)
            if matches!(&notice.kind, Some(NoticeKind::MatterReadStopped { matter, .. })
                if *matter == other))
    });
    assert_eq!(stopped.count(), 1, "{}", events.dump());
    let started = events
        .events()
        .filter(|event| matches!(event, DomainEvent::TurnStarted(_)))
        .count();
    assert_eq!(started, 1, "nothing ran after the stop: {}", events.dump());
    events.assert_no_mismatch();
    host.shutdown();
}
