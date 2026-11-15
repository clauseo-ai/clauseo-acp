//! A conversation's read invite into another matter at the interface: `AllowMatterRead` and
//! `WithdrawMatterRead` refuse the thread's own matter and an unknown one, log their events,
//! show on the thread's header (`read_matters`) and keep across a restart.

use clauseo_protocol::{
    CommandOutcome, DomainEvent, ErrorCode, HostCommand, MatterId, StreamHeader, StreamId,
    ThreadId, ViewPatchBody,
};

use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// The thread's header's `read_matters`, from a fresh view's first frame.
fn read_matters(host: &Running, thread: ThreadId) -> Vec<MatterId> {
    let mut view = host.view(StreamId::Thread(thread), None);
    view.until_items(1);
    let first = view.patches().next().expect("a first frame");
    match &first.body {
        ViewPatchBody::Reset { view_window } => match &view_window.header {
            Some(StreamHeader::Thread(header)) => header.read_matters.clone(),
            other => panic!("no thread header: {other:?}"),
        },
        other => panic!("the first frame is not a reset: {other:?}"),
    }
}

fn allow(thread: ThreadId, matter: MatterId) -> HostCommand {
    HostCommand::AllowMatterRead { thread, matter }
}

#[test]
fn an_invite_is_checked_logged_shown_and_kept_across_a_restart() {
    let dir = Dir::new("matter_read", &[(FAKE, Scenario::Bundled("prompt-stream"))]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "a", &[("a.txt", "x\n")], FAKE);
    let (other, _, _) = matter_with_thread(&dir, &host, "b", &[("b.txt", "y\n")], FAKE);
    let (third, _, _) = matter_with_thread(&dir, &host, "c", &[("c.txt", "z\n")], FAKE);
    // Its own matter, or a matter the host does not know: refused.
    host.refused(allow(thread, matter), ErrorCode::CommandInvalid);
    let stranger = MatterId::new(crate::support::foreign_uuid(9)).unwrap();
    host.refused(allow(thread, stranger), ErrorCode::CommandUnknownMatter);
    let unknown = ThreadId::new(crate::support::foreign_uuid(8)).unwrap();
    host.refused(allow(unknown, other), ErrorCode::CommandUnknownThread);

    assert_eq!(host.ok(allow(thread, other)), CommandOutcome::Done);
    assert_eq!(host.ok(allow(thread, third)), CommandOutcome::Done);
    let withdraw = HostCommand::WithdrawMatterRead {
        thread,
        matter: third,
    };
    assert_eq!(host.ok(withdraw), CommandOutcome::Done);
    let mut events = host.events(StreamId::Thread(thread));
    events.until("the withdrawal", |e| {
        e.count(|event| matches!(event, DomainEvent::MatterReadWithdrawn(_))) == 1
    });
    let allowed = events.count(|event| matches!(event, DomainEvent::MatterReadAllowed(_)));
    assert_eq!(allowed, 2, "{}", events.dump());
    host.settle();
    assert_eq!(read_matters(&host, thread), [other]);
    host.shutdown();

    // A restart folds the log again: the invite stands, and can be withdrawn.
    let host = dir.start(1);
    assert_eq!(read_matters(&host, thread), [other]);
    host.ok(HostCommand::WithdrawMatterRead {
        thread,
        matter: other,
    });
    host.settle();
    assert_eq!(read_matters(&host, thread), Vec::<MatterId>::new());
    host.shutdown();
}
