//! The limits timer (`actor/limits.rs`) on the turn rig's actor, with a limits model started
//! by hand for Claude and a reader the test answers: a deadline whose wall time passed while
//! its fire never ran (the computer slept) is swept by the next input, and a fire taken out of
//! play (by that sweep, or by the last subscriber leaving) is stale when it runs.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, Finite, HostEvent,
    LimitSource, LimitWindow, LimitsView, PlanLabel, Principal, RefreshWhy, Timestamp,
    WindowFreshness, WindowKind,
};
use futures::channel::oneshot;
use futures::{FutureExt, StreamExt};

use super::rig::Rig;
use crate::actor::input::Input;
use crate::actor::limits::{self, LimitsInput, for_tests};
use crate::handle::Subscription;

/// Ten minutes: the plan's window resets this long after the start.
const RESET: Duration = Duration::from_secs(600);

/// A reader that records each read and answers it when the test says.
#[derive(Clone, Default)]
struct Reads(Arc<Mutex<Vec<Option<oneshot::Sender<ReadOutcome>>>>>);

impl Reads {
    fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    fn answer(&self, n: usize, outcome: ReadOutcome) {
        let sender = self.0.lock().unwrap()[n].take().unwrap();
        sender.send(outcome).unwrap();
    }
}

impl ReadLimits for Reads {
    fn read(
        &self,
        _agent: AgentKind,
    ) -> std::pin::Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        let (tx, rx) = oneshot::channel();
        self.0.lock().unwrap().push(Some(tx));
        Box::pin(rx.map(|answer| answer.unwrap_or(ReadOutcome::Failed(ReadFailure::TimedOut))))
    }
}

/// Claude's plan, 40% used, its window resetting at `resets_at`, checked at `checked_at`.
fn plan(checked_at: Timestamp, resets_at: Timestamp) -> AccountLimits {
    AccountLimits {
        agent: AgentKind::Claude,
        account: AccountRef("ada@example.com".to_owned()),
        plan: Some(PlanLabel("Max".to_owned())),
        billing: Billing::Plan,
        windows: vec![LimitWindow {
            id: "five_hour".to_owned(),
            kind: WindowKind::Session,
            label: "5-hour limit".to_owned(),
            used_percent: Finite::new(40.0).unwrap(),
            resets_at: Some(resets_at),
            duration_mins: Some(300),
            warning: false,
        }],
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Fresh { checked_at },
    }
}

/// The rig's wall time plus `after`.
fn later(rig: &Rig, after: Duration) -> Timestamp {
    let now = rig.actor.now().wall.as_millis();
    Timestamp::new(now + i64::try_from(after.as_millis()).unwrap()).unwrap()
}

/// A limits subscription made on the rig's actor.
fn subscribe(rig: &mut Rig) -> Subscription {
    let (reply, mut answer) = oneshot::channel();
    limits::subscribe(&mut rig.actor, &Principal::LocalDesktop, reply);
    answer.try_recv().unwrap().unwrap().unwrap()
}

/// The events the subscription holds now.
fn ready(sub: &mut Subscription) -> Vec<HostEvent> {
    let mut events = Vec::new();
    while let Some(Some(item)) = sub.next().now_or_never() {
        events.push(item.decode().unwrap());
    }
    events
}

/// Claude's window row in the only view among `events`.
fn row(events: &[HostEvent]) -> WindowFreshness {
    let views: Vec<&LimitsView> = events
        .iter()
        .filter_map(|event| {
            if let HostEvent::LimitsViewChanged(view) = event {
                Some(view)
            } else {
                None
            }
        })
        .collect();
    let [view] = views.as_slice() else {
        panic!("one view: {events:?}");
    };
    view.0[&AgentKind::Claude].windows[0].freshness
}

/// Handles inputs until the limits timer's fire comes, which is held back.
fn hold_the_fire(rig: &mut Rig) {
    rig.hold_next("the limits timer's fire", |_, input| {
        matches!(input, Input::Limits(LimitsInput::Deadline { .. }))
    });
}

#[test]
fn a_deadline_already_past_is_swept_by_the_next_input() {
    let mut rig = Rig::start("limit-timer-late", "");
    let reads = Reads::default();
    let cached = vec![plan(rig.actor.now().wall, later(&rig, RESET))];
    for_tests::start(
        &mut rig.actor,
        Arc::new(reads.clone()),
        cached,
        vec![AgentKind::Claude],
    );
    let mut sub = subscribe(&mut rig);
    assert_eq!(
        row(&ready(&mut sub)),
        WindowFreshness::Stale,
        "a stored read"
    );
    assert!(for_tests::timer(&rig.actor).1, "the reset's timer is armed");

    // The reset time passes and the fire is posted, but has not run (as after a sleep).
    rig.clock.advance(RESET);
    hold_the_fire(&mut rig);
    limits::refresh(&mut rig.actor, Some(AgentKind::Claude), RefreshWhy::Shown);
    assert_eq!(
        row(&ready(&mut sub)),
        WindowFreshness::ResetPassed,
        "the sweep ran before the refresh"
    );

    // The fire the sweep took out of play does nothing when it runs.
    let before = reads.count();
    rig.release_held();
    assert_eq!(ready(&mut sub), []);
    assert_eq!(reads.count(), before);
    drop(sub);
    rig.finish();
}

#[test]
fn a_fire_after_the_last_subscriber_left_is_stale() {
    let mut rig = Rig::start("limit-timer-unwatched", "");
    let reads = Reads::default();
    for_tests::start(
        &mut rig.actor,
        Arc::new(reads.clone()),
        Vec::new(),
        vec![AgentKind::Claude],
    );
    let mut sub = subscribe(&mut rig);
    drop(ready(&mut sub));
    let usage = ReadOutcome::Usage {
        limits: plan(rig.actor.now().wall, later(&rig, RESET)),
        reached: None,
    };
    reads.answer(0, usage);
    let mut events = Vec::new();
    rig.run_until("the start-up read's limits", |_| {
        events.extend(ready(&mut sub));
        !events.is_empty()
    });
    let (generation, armed) = for_tests::timer(&rig.actor);
    assert!(armed, "the reset's timer is armed");

    // The fire is posted, then the last subscriber leaves before it runs.
    rig.clock.advance(RESET);
    hold_the_fire(&mut rig);
    drop(sub);
    rig.run_until("the subscriber gone", |actor| !for_tests::watching(actor));
    assert_ne!(for_tests::timer(&rig.actor), (generation, true));

    rig.release_held();
    rig.settle_commits();
    assert_eq!(reads.count(), 1, "the stale fire started no read");
    rig.finish();
}
