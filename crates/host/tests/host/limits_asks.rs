//! A question pending in a Claude thread holds limits reads back, and each way it goes away
//! lets the held read start: an answer (in `limits_host.rs`), a turn's end, a cancel, the
//! agent's exit; an elicitation holds one too, and so does an accepted URL elicitation until
//! the agent says it is complete. Also: a second `subscribe_limits` subscriber gets the
//! current view alone. Reads go through the scripted [`Reads`].

use std::time::Duration;

use clauseo_protocol::{
    AgentKind, CommandOutcome, DomainEvent, ElicitationId, ElicitationResponse, HostCommand,
    RawPayload, RefreshWhy, StreamId, ThreadId, TurnId,
};

use crate::limits_host::{CLAUDE, Feed, Reads, refresh, start, usage};
use crate::observe::Events;
use crate::permissions::answer_from;
use crate::support::{Dir, Running, Scenario, matter_with_thread};

/// The cancel fallback's wait for the prompt's answer.
const CANCEL_WAIT: Duration = Duration::from_secs(5);

/// A Claude host playing `scenario`, its start-up read answered, with one prompt sent on a
/// thread; returns once the agent asked its question and a `TryAgain` read nothing.
struct Asked {
    _dir: Dir,
    host: Running,
    reads: Reads,
    thread: ThreadId,
    turn: TurnId,
    events: Events,
}

fn asked(test: &str, scenario: Scenario) -> Asked {
    let dir = Dir::new(test, &[(CLAUDE, scenario)]);
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 10.0, None));
    feed.changed("the start-up read");
    drop(feed);
    // The question comes later than the start-up read: a try again within 10 s of it starts none.
    host.clock.advance(Duration::from_secs(11));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the clause.");
    events.until("a question", |e| {
        e.position(|event| {
            matches!(
                event,
                DomainEvent::PermissionRequested(_) | DomainEvent::ElicitationRequested(_)
            )
        })
        .is_some()
    });
    refresh(&host, RefreshWhy::TryAgain);
    assert_eq!(
        reads.count(AgentKind::Claude),
        1,
        "no read while the question is pending"
    );
    Asked {
        _dir: dir,
        host,
        reads,
        thread,
        turn,
        events,
    }
}

impl Asked {
    /// The held read has started, and only it.
    fn assert_read_started(&self, after: &str) {
        self.host.settle();
        assert_eq!(
            self.reads.count(AgentKind::Claude),
            2,
            "the held read starts after {after}"
        );
    }

    /// The thread's first elicitation.
    fn elicitation(&self) -> ElicitationId {
        self.events
            .events()
            .find_map(|event| match event {
                DomainEvent::ElicitationRequested(requested) => Some(requested.elicitation),
                _ => None,
            })
            .unwrap()
    }

    /// Answers the second permission request, then the third the agent asks once it has that
    /// answer; the first stays pending. The count sits between the two: the third request
    /// proves the actor took the second answer, and the agent ends its turn or exits only
    /// after the third answer, so nothing else can have released a read by then.
    fn answer_second_and_third_permissions(&mut self) {
        answer_from(
            &self.host,
            &mut self.events,
            self.thread,
            1,
            &["allow-once"],
        );
        self.events.until("permission request 3", |e| {
            e.permission_requests().len() > 2
        });
        self.host.settle();
        assert_eq!(
            self.reads.count(AgentKind::Claude),
            1,
            "no read while the first request is pending"
        );
        answer_from(
            &self.host,
            &mut self.events,
            self.thread,
            2,
            &["allow-once"],
        );
    }

    fn answer_elicitation(&self, values: &str) {
        let values: RawPayload = serde_json::from_str(values).unwrap();
        let outcome = self.host.ok(HostCommand::AnswerElicitation {
            thread: self.thread,
            elicitation: self.elicitation(),
            response: ElicitationResponse::Accept { values },
        });
        assert_eq!(outcome, CommandOutcome::Done);
    }
}

#[test]
fn a_turn_that_ends_with_its_permission_pending_lets_the_read_start() {
    let mut asked = asked(
        "a_turn_that_ends_with_its_permission_pending_lets_the_read_start",
        Scenario::Local("permission-then-end"),
    );
    asked.answer_second_and_third_permissions();
    asked.events.until_turn_ended(asked.turn);
    asked.assert_read_started("the turn's end");
    asked.host.shutdown();
}

#[test]
fn a_cancel_lets_the_read_start_before_the_turn_ends() {
    let mut asked = asked(
        "a_cancel_lets_the_read_start_before_the_turn_ends",
        Scenario::Local("permission-cancel-unanswered"),
    );
    let outcome = asked.host.ok(HostCommand::Cancel {
        thread: asked.thread,
    });
    assert_eq!(outcome, CommandOutcome::Done);
    asked.host.wait_armed(CANCEL_WAIT);
    asked.assert_read_started("the cancel");
    asked.events.drain_ready();
    assert!(
        asked.events.turn_ended(asked.turn).is_none(),
        "the prompt is not answered, so the turn has not ended: {}",
        asked.events.dump()
    );
    asked.host.clock.advance(CANCEL_WAIT);
    asked.events.until_turn_ended(asked.turn);
    asked.host.shutdown();
}

#[test]
fn an_agent_that_exits_with_its_permission_pending_lets_the_read_start() {
    let mut asked = asked(
        "an_agent_that_exits_with_its_permission_pending_lets_the_read_start",
        Scenario::Local("permission-then-exit"),
    );
    asked.answer_second_and_third_permissions();
    asked.events.until_turn_ended(asked.turn);
    asked.assert_read_started("the agent's exit");
    asked.host.shutdown();
}

#[test]
fn an_elicitation_holds_the_read_until_it_is_answered() {
    let mut asked = asked(
        "an_elicitation_holds_the_read_until_it_is_answered",
        Scenario::Bundled("claude-elicitation-form"),
    );
    asked.answer_elicitation(r#"{"party":"Tenant"}"#);
    asked.assert_read_started("the answer");
    asked.events.until_turn_ended(asked.turn);
    asked.host.shutdown();
}

#[test]
fn an_accepted_url_elicitation_holds_the_read_until_it_is_complete() {
    let mut asked = asked(
        "an_accepted_url_elicitation_holds_the_read_until_it_is_complete",
        Scenario::Local("elicitation-url-held"),
    );
    asked.answer_elicitation("{}");
    refresh(&asked.host, RefreshWhy::TryAgain);
    asked.host.settle();
    assert_eq!(
        asked.reads.count(AgentKind::Claude),
        1,
        "no read while the lawyer finishes in the browser"
    );
    let outcome = asked.host.ok(HostCommand::Cancel {
        thread: asked.thread,
    });
    assert_eq!(outcome, CommandOutcome::Done);
    asked.assert_read_started("the cancel");
    asked.events.until_turn_ended(asked.turn);
    asked.host.shutdown();
}

#[test]
fn a_second_subscriber_gets_the_current_view_alone() {
    let dir = Dir::new(
        "a_second_subscriber_gets_the_current_view_alone",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut first = Feed::new(&host);
    first.view("the first subscriber's view");
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 10.0, None));
    let (_, current) = first.changed("the start-up read");

    let mut second = Feed::new(&host);
    assert_eq!(second.view("the second subscriber's view"), current);
    host.settle();
    assert_eq!(first.ready(), [], "the first subscriber gets nothing more");
    assert_eq!(
        second.ready(),
        [],
        "the second subscriber gets the view once"
    );
    host.shutdown();
}
