//! The host runs `limits`: start-up from the stored limits, live signals, turn ends,
//! `RefreshLimits`, pending questions and the deadlines, seen through `subscribe_limits`. Every
//! test sets a scripted reader ([`Reads`]) through `HostConfig::with_limits_reader`: it records
//! each read and answers only when the test says, so no reader process ever starts.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_host::Subscription;
use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AgentLimitsView, AtLimitBehaviour, Billing,
    CommandOutcome, Finite, Freshness, HostCommand, HostEvent, LimitSource, LimitWindow,
    LimitsView, MeterTone, PlanLabel, Principal, RefreshWhy, StaleReason, StreamId, ThreadId,
    Timestamp, WindowFreshness, WindowKind,
};
use clauseo_store::{Store, StoreOptions};
use futures::{FutureExt, StreamExt};

use crate::codex::CODEX;
use crate::permissions::{KINDS_ANSWERS, answer_from, until_first_request};
use crate::support::{Dir, FAKE, Running, START_MS, Scenario, matter_with_thread, wait};

/// The agent id that makes an agent Claude.
pub(crate) const CLAUDE: &str = "claude-acp";

/// A minute of the host clock.
const MINUTE: Duration = Duration::from_secs(60);

/// A scripted reader: records each read and answers it with the next outcome the test gave for
/// that agent, waiting until there is one (a read the test has not answered stays in flight).
#[derive(Clone)]
pub(crate) struct Reads(Arc<Shared>);

struct Shared {
    calls: Mutex<Vec<AgentKind>>,
    /// Each read's agent as it starts, for [`Reads::until_count`].
    started: (
        async_channel::Sender<AgentKind>,
        async_channel::Receiver<AgentKind>,
    ),
    answers: BTreeMap<
        AgentKind,
        (
            async_channel::Sender<ReadOutcome>,
            async_channel::Receiver<ReadOutcome>,
        ),
    >,
}

impl Reads {
    pub(crate) fn new() -> Self {
        let answers = [AgentKind::Claude, AgentKind::Codex]
            .into_iter()
            .map(|agent| (agent, async_channel::unbounded()))
            .collect();
        Self(Arc::new(Shared {
            calls: Mutex::new(Vec::new()),
            started: async_channel::unbounded(),
            answers,
        }))
    }

    /// How many reads of `agent` started.
    pub(crate) fn count(&self, agent: AgentKind) -> usize {
        let calls = self.0.calls.lock().unwrap();
        calls.iter().filter(|call| **call == agent).count()
    }

    /// Waits until `agent` has had at least `n` reads (the host's agents run on the wall clock).
    pub(crate) fn until_count(&self, agent: AgentKind, n: usize) {
        let started = self.0.started.1.clone();
        while self.count(agent) < n {
            wait("a limits read", started.recv()).unwrap();
        }
    }

    /// Answers `agent`'s read in flight, or its next one, with `outcome`.
    pub(crate) fn answer(&self, agent: AgentKind, outcome: ReadOutcome) {
        let (tx, _) = &self.0.answers[&agent];
        tx.try_send(outcome).unwrap();
    }
}

impl ReadLimits for Reads {
    fn read(&self, agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        self.0.calls.lock().unwrap().push(agent);
        self.0.started.0.try_send(agent).unwrap();
        let answers = self.0.answers.get(&agent).map(|(_, rx)| rx.clone());
        Box::pin(async move {
            let timed_out = ReadOutcome::Failed(ReadFailure::TimedOut);
            match answers {
                Some(rx) => rx.recv().await.unwrap_or(timed_out),
                None => timed_out,
            }
        })
    }
}

/// A limits subscription and every event it delivered, decoded.
pub(crate) struct Feed {
    sub: Subscription,
    events: Vec<HostEvent>,
}

impl Feed {
    pub(crate) fn new(host: &Running) -> Self {
        let sub = wait(
            "subscribe_limits",
            host.handle.subscribe_limits(Principal::LocalDesktop),
        )
        .unwrap();
        Self {
            sub,
            events: Vec::new(),
        }
    }

    /// The next event; fails the test when none comes.
    fn next(&mut self, what: &str) -> HostEvent {
        let item = wait(what, self.sub.next())
            .unwrap_or_else(|| panic!("the limits subscription ended before {what}"));
        let event = item.decode().unwrap();
        self.events.push(event.clone());
        event
    }

    /// The next event, which must be a `LimitsViewChanged`.
    pub(crate) fn view(&mut self, what: &str) -> LimitsView {
        match self.next(what) {
            HostEvent::LimitsViewChanged(view) => view,
            other => panic!("{what}: expected LimitsViewChanged, got {other:?}"),
        }
    }

    /// The next two events: one `AccountLimitsChanged`, then the view.
    pub(crate) fn changed(&mut self, what: &str) -> (AccountLimits, LimitsView) {
        let limits = match self.next(what) {
            HostEvent::AccountLimitsChanged(limits) => limits,
            other => panic!("{what}: expected AccountLimitsChanged, got {other:?}"),
        };
        (limits, self.view(what))
    }

    /// The events already in the channel, without waiting.
    pub(crate) fn ready(&mut self) -> Vec<HostEvent> {
        let mut ready = Vec::new();
        while let Some(Some(item)) = self.sub.next().now_or_never() {
            ready.push(item.decode().unwrap());
        }
        self.events.extend(ready.iter().cloned());
        ready
    }
}

/// The wall time `secs` seconds after run 0's start.
pub(crate) fn at(secs: i64) -> Timestamp {
    Timestamp::new(START_MS + secs * 1000).unwrap()
}

/// A plan account of `agent` with one five-hour window `used` percent full.
pub(crate) fn plan(agent: AgentKind, used: f64, resets_at: Option<Timestamp>) -> AccountLimits {
    AccountLimits {
        agent,
        account: AccountRef("ada@example.com".to_owned()),
        plan: Some(PlanLabel("Max".to_owned())),
        billing: Billing::Plan,
        windows: vec![LimitWindow {
            id: "five_hour".to_owned(),
            kind: WindowKind::Session,
            label: "5-hour limit".to_owned(),
            used_percent: Finite::new(used).unwrap(),
            resets_at,
            duration_mins: Some(300),
            warning: false,
            model: None,
        }],
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Fresh { checked_at: at(0) },
    }
}

/// A good read of `agent`'s plan.
pub(crate) fn usage(agent: AgentKind, used: f64, resets_at: Option<Timestamp>) -> ReadOutcome {
    ReadOutcome::Usage {
        limits: plan(agent, used, resets_at),
        reached: None,
    }
}

/// Keeps `limits` in the store of `dir`, before any host opens it.
fn seed(dir: &Dir, limits: AccountLimits) {
    let data = dir.data().to_owned();
    wait("seeding the store", async move {
        let store = Store::open(StoreOptions::new(data)).await.unwrap();
        store.put_account_limits(limits).await.unwrap();
        store.close().await.unwrap();
    });
}

/// Starts run 0 of `dir` reading through `reads`.
pub(crate) fn start(dir: &Dir, reads: &Reads) -> Running {
    let reader = Arc::new(reads.clone());
    dir.start_with(0, move |config| config.with_limits_reader(reader))
}

/// `agent`'s entry in `view`.
fn entry(view: &LimitsView, agent: AgentKind) -> &AgentLimitsView {
    view.0
        .get(&agent)
        .unwrap_or_else(|| panic!("no {agent:?} in {view:?}"))
}

/// `RefreshLimits`, which answers `Done`.
pub(crate) fn refresh(host: &Running, why: RefreshWhy) {
    let outcome = host.ok(HostCommand::RefreshLimits { agent: None, why });
    assert_eq!(outcome, CommandOutcome::Done);
}

/// Runs one prompt on `thread` to its end.
fn turn(host: &Running, thread: ThreadId) {
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    events.until_turn_ended(turn);
    host.settle();
}

#[test]
fn start_shows_stored_limits_stale_and_reads_each_agent() {
    let dir = Dir::new(
        "start_shows_stored_limits_stale_and_reads_each_agent",
        &[
            (CLAUDE, Scenario::Bundled("limit-warning")),
            (CODEX, Scenario::Bundled("codex-handshake")),
        ],
    );
    let mut stored = plan(AgentKind::Claude, 40.0, Some(at(7200)));
    stored.source = LimitSource::Fresh {
        checked_at: at(-3600),
    };
    seed(&dir, stored);
    let reads = Reads::new();
    let host = start(&dir, &reads);
    host.settle();
    assert_eq!(
        (
            reads.count(AgentKind::Claude),
            reads.count(AgentKind::Codex)
        ),
        (1, 1)
    );

    let mut feed = Feed::new(&host);
    let view = feed.view("the current view");
    let claude = entry(&view, AgentKind::Claude);
    assert_eq!(claude.freshness, Freshness::Stale, "{view:?}");
    assert_eq!(claude.checked_at, Some(at(-3600)));
    assert_eq!(entry(&view, AgentKind::Codex).freshness, Freshness::Loading);

    // A live signal publishes the stored account as the model holds it: aged, its time kept.
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    turn(&host, thread);
    let (limits, _) = feed.changed("the live signal");
    assert_eq!(
        limits.source,
        LimitSource::Stale {
            checked_at: at(-3600),
            reason: StaleReason::Aged,
        }
    );
    assert_eq!(reads.count(AgentKind::Claude), 1, "the start-up read runs");

    reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 45.0, Some(at(7200))),
    );
    let (limits, view) = feed.changed("Claude's read");
    assert_eq!(limits.source, LimitSource::Fresh { checked_at: at(0) });
    assert_eq!(entry(&view, AgentKind::Claude).freshness, Freshness::Fresh);
    host.settle();
    assert_eq!(feed.ready(), [], "nothing more without another read");
    host.shutdown();
}

#[test]
fn live_warning_publishes_the_limits_and_a_near_meter() {
    let dir = Dir::new(
        "live_warning_publishes_the_limits_and_a_near_meter",
        &[(CLAUDE, Scenario::Bundled("limit-warning"))],
    );
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 50.0, None));
    feed.changed("the start-up read");

    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    turn(&host, thread);
    let (limits, view) = feed.changed("the live signal");
    let window = limits.windows.iter().find(|w| w.id == "five_hour").unwrap();
    assert!(
        (window.used_percent.get() - 96.0).abs() < 1e-9,
        "{window:?}"
    );
    assert!(window.warning);
    let meter = entry(&view, AgentKind::Claude).meter.clone().unwrap();
    assert_eq!(meter.tone, MeterTone::Near);
    assert_eq!(
        reads.count(AgentKind::Claude),
        1,
        "no read during or after the turn"
    );
    assert_eq!(feed.ready(), []);
    host.shutdown();
}

#[test]
fn codex_turn_end_reads_at_most_once_a_minute() {
    let dir = Dir::new(
        "codex_turn_end_reads_at_most_once_a_minute",
        &[(CODEX, Scenario::Bundled("codex-handshake"))],
    );
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(AgentKind::Codex, usage(AgentKind::Codex, 10.0, None));
    feed.changed("the start-up read");

    host.clock.advance(MINUTE + Duration::from_secs(1));
    let (matter, first, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    turn(&host, first);
    assert_eq!(
        reads.count(AgentKind::Codex),
        2,
        "a turn a minute after the last read"
    );
    reads.answer(AgentKind::Codex, usage(AgentKind::Codex, 11.0, None));
    feed.changed("the turn's read");

    let second = host.create_thread(matter, CODEX);
    turn(&host, second);
    assert_eq!(reads.count(AgentKind::Codex), 2, "a turn within the minute");
    host.shutdown();
}

#[test]
fn claude_turn_end_reads_once_the_last_read_is_ten_minutes_old() {
    let dir = Dir::new(
        "claude_turn_end_reads_once_the_last_read_is_ten_minutes_old",
        &[(CLAUDE, Scenario::Bundled("prompt-stream"))],
    );
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 10.0, None));
    feed.changed("the start-up read");

    let (matter, first, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    turn(&host, first);
    assert_eq!(reads.count(AgentKind::Claude), 1, "the last read is new");

    host.clock.advance(10 * MINUTE);
    let second = host.create_thread(matter, CLAUDE);
    turn(&host, second);
    assert_eq!(
        reads.count(AgentKind::Claude),
        2,
        "the last read is ten minutes old"
    );
    host.shutdown();
}

#[test]
fn refresh_shown_reads_old_data_and_try_again_reads_unless_in_flight() {
    let dir = Dir::new(
        "refresh_shown_reads_old_data_and_try_again_reads_unless_in_flight",
        &[(CLAUDE, Scenario::Bundled("prompt-stream"))],
    );
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the current view");

    // Past the 10 s window, so the try joins the read in flight and publishes nothing.
    host.clock.advance(Duration::from_secs(11));
    refresh(&host, RefreshWhy::TryAgain);
    assert_eq!(
        reads.count(AgentKind::Claude),
        1,
        "the start-up read is in flight"
    );
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 10.0, None));
    feed.changed("the start-up read");
    host.clock.advance(Duration::from_secs(11));
    refresh(&host, RefreshWhy::TryAgain);
    assert_eq!(
        reads.count(AgentKind::Claude),
        2,
        "try again reads once the start-up read is 10 s old"
    );
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 11.0, None));
    feed.changed("the try-again read");

    refresh(&host, RefreshWhy::Shown);
    host.clock.advance(Duration::from_secs(299));
    refresh(&host, RefreshWhy::Shown);
    assert_eq!(
        reads.count(AgentKind::Claude),
        2,
        "the last read is under 5 minutes old"
    );
    host.clock.advance(Duration::from_secs(1));
    refresh(&host, RefreshWhy::Shown);
    refresh(&host, RefreshWhy::Shown);
    assert_eq!(
        reads.count(AgentKind::Claude),
        3,
        "two shown at 5 minutes: one read"
    );
    host.settle();
    assert_eq!(feed.ready(), [], "a throttled shown publishes nothing");
    host.shutdown();
}

#[test]
fn refresh_waits_for_a_pending_permission() {
    let dir = Dir::new(
        "refresh_waits_for_a_pending_permission",
        &[(CLAUDE, Scenario::Bundled("permission-kinds"))],
    );
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    feed.view("the current view");
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 10.0, None));
    feed.changed("the start-up read");

    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the four clauses.");
    until_first_request(&mut events);
    host.clock.advance(5 * MINUTE);
    refresh(&host, RefreshWhy::Shown);
    refresh(&host, RefreshWhy::TryAgain);
    assert_eq!(
        reads.count(AgentKind::Claude),
        1,
        "no read while a permission is pending"
    );

    answer_from(&host, &mut events, thread, 0, &KINDS_ANSWERS[..1]);
    host.settle();
    assert_eq!(
        reads.count(AgentKind::Claude),
        2,
        "the read starts once it is answered"
    );
    reads.answer(AgentKind::Claude, usage(AgentKind::Claude, 11.0, None));
    feed.changed("the read after the answer");

    answer_from(&host, &mut events, thread, 1, &KINDS_ANSWERS[1..]);
    events.until_turn_ended(turn);
    host.settle();
    assert_eq!(reads.count(AgentKind::Claude), 2);
    host.shutdown();
}

#[test]
fn deadlines_run_only_with_a_subscriber() {
    let dir = Dir::new(
        "deadlines_run_only_with_a_subscriber",
        &[(CLAUDE, Scenario::Bundled("prompt-stream"))],
    );
    let mut stored = plan(AgentKind::Claude, 40.0, Some(at(1800)));
    stored.source = LimitSource::Fresh {
        checked_at: at(-3600),
    };
    seed(&dir, stored);
    let reads = Reads::new();
    let host = start(&dir, &reads);
    host.settle();
    assert_eq!(
        host.clock.armed(),
        0,
        "no limits timer without a subscriber"
    );

    let mut feed = Feed::new(&host);
    feed.view("the current view");
    host.wait_armed(30 * MINUTE);
    reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 40.0, Some(at(1800))),
    );
    feed.changed("the start-up read");
    host.wait_armed(15 * MINUTE);

    host.clock.advance(15 * MINUTE);
    let view = feed.view("the stale mark");
    let row = entry(&view, AgentKind::Claude)
        .windows
        .first()
        .cloned()
        .unwrap();
    assert_eq!(row.freshness, WindowFreshness::Stale, "{view:?}");
    assert_eq!(
        reads.count(AgentKind::Claude),
        1,
        "a stale mark reads nothing"
    );

    host.wait_armed(15 * MINUTE);
    host.clock.advance(15 * MINUTE);
    let view = feed.view("the reset time");
    let row = entry(&view, AgentKind::Claude)
        .windows
        .first()
        .cloned()
        .unwrap();
    assert_eq!(row.freshness, WindowFreshness::ResetPassed, "{view:?}");
    assert_eq!(row.used_percent, None);
    host.settle();
    assert_eq!(reads.count(AgentKind::Claude), 2, "one read at the reset");

    reads.answer(
        AgentKind::Claude,
        usage(AgentKind::Claude, 5.0, Some(at(19_800))),
    );
    feed.changed("the read at the reset");
    host.wait_armed(15 * MINUTE);
    drop(feed);
    let mut settles = 0;
    while host.clock.armed() > 0 && settles < 100 {
        host.settle();
        settles += 1;
    }
    assert_eq!(
        host.clock.armed(),
        0,
        "the last subscriber took the timer with it"
    );
    host.shutdown();
}

#[test]
fn no_claude_or_codex_entry_reads_nothing() {
    let dir = Dir::new(
        "no_claude_or_codex_entry_reads_nothing",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let reads = Reads::new();
    let host = start(&dir, &reads);
    let mut feed = Feed::new(&host);
    assert_eq!(feed.view("the current view"), LimitsView::default());
    refresh(&host, RefreshWhy::Shown);
    refresh(&host, RefreshWhy::TryAgain);
    host.settle();
    assert_eq!(
        (
            reads.count(AgentKind::Claude),
            reads.count(AgentKind::Codex)
        ),
        (0, 0)
    );
    assert_eq!(feed.ready(), []);
    assert_eq!(host.clock.armed(), 0);

    let phone: Principal = serde_json::from_str("\"some_phone\"").unwrap();
    let refused = wait("subscribe_limits", host.handle.subscribe_limits(phone)).unwrap_err();
    assert_eq!(
        refused.code(),
        clauseo_protocol::ErrorCode::TransportUnauthorized
    );
    host.shutdown();
}
