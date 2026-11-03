//! The limits on `main`'s and the settings window's channels (`open_stream` follows them there
//! alone), the refresh either window's focus asks for (`on_window_event` in `lib.rs`, then
//! `Bridge::shown`), and the settings window's refreshes, throttled. A scripted reader stands in
//! for the limits readers: it records each read and answers only when the test says, so no
//! reader process starts.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use clauseo_host::{Host, ManualClock};
use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, Finite, LimitSource,
    LimitWindow, LimitsView, PlanLabel, Timestamp, WindowKind,
};
use futures::FutureExt as _;
use futures::StreamExt as _;
use futures::channel::oneshot;
use tauri::test::{MockRuntime, mock_builder};
use tauri::{Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use super::*;
use crate::bridge::throttle::REFRESH_GAP;
use crate::host_start::{HostCell, stop};
use crate::test_support::{Messages, Running, collector};

/// How long a test waits for the host to start a read. A bound, not a sleep.
const READ_WAIT: Duration = Duration::from_secs(20);

/// The wait a `Shown` refresh has the host keep since the last read.
const SHOWN_AFTER: Duration = Duration::from_mins(5);

/// A scripted Claude reader: each read is recorded and waits until [`Reads::answer`].
struct Reads {
    started: Mutex<Sender<()>>,
    starts: Mutex<Receiver<()>>,
    count: Mutex<usize>,
    pending: Mutex<VecDeque<oneshot::Sender<ReadOutcome>>>,
}

impl Reads {
    fn new() -> Arc<Self> {
        let (started, starts) = channel();
        Arc::new(Self {
            started: Mutex::new(started),
            starts: Mutex::new(starts),
            count: Mutex::new(0),
            pending: Mutex::new(VecDeque::new()),
        })
    }

    /// How many reads started.
    fn count(&self) -> usize {
        *lock(&self.count)
    }

    /// Waits until at least `n` reads started.
    fn until_count(&self, n: usize) {
        while self.count() < n {
            lock(&self.starts)
                .recv_timeout(READ_WAIT)
                .unwrap_or_else(|err| panic!("no limits read: {err}"));
        }
    }

    /// Answers the oldest read in flight with `outcome`.
    fn answer(&self, outcome: ReadOutcome) {
        let read = lock(&self.pending).pop_front().expect("no read in flight");
        read.send(outcome).unwrap();
    }
}

impl ReadLimits for Reads {
    fn read(&self, agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        assert_eq!(agent, AgentKind::Claude, "the settings name only Claude");
        let (answer, answered) = oneshot::channel();
        lock(&self.pending).push_back(answer);
        *lock(&self.count) += 1;
        lock(&self.started).send(()).unwrap();
        Box::pin(async move {
            answered
                .await
                .unwrap_or(ReadOutcome::Failed(ReadFailure::TimedOut))
        })
    }
}

/// A host whose settings name Claude, reading through a new [`Reads`] on a manual clock, once
/// its start-up read has started.
fn start(scratch: &Scratch) -> (Running, Arc<Reads>, ManualClock) {
    let reads = Reads::new();
    let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
    let host = scratch.start_reading(Arc::<Reads>::clone(&reads), clock.clone());
    reads.until_count(1);
    (host, reads, clock)
}

/// The view a limits message carries.
fn view(event: HostEvent) -> LimitsView {
    let HostEvent::LimitsViewChanged(view) = event else {
        panic!("not limits_view_changed: {event:?}");
    };
    view
}

/// The next limits message of `messages`, which must be a view.
fn next_view(messages: &Messages) -> LimitsView {
    view(messages.next_limits())
}

/// `main`'s channel and the settings window's each get the current view at open and then each
/// change, and those messages count toward each window's acks like any other. A window of any
/// other label follows no limits (U12 opened them for Settings: 0027's amendment, 0028).
#[test]
fn main_and_settings_get_the_limits_view_then_each_change_and_acks_count_them() {
    let scratch = Scratch::with_limits("limits-windows");
    let (host, reads, _clock) = start(&scratch);
    let bridge = host.bridge();
    let (channel, main) = collector();
    block_on(bridge.open_stream(MAIN, channel)).unwrap();
    let (channel, settings) = collector();
    block_on(bridge.open_stream(SETTINGS, channel)).unwrap();
    let (channel, other) = collector();
    block_on(bridge.open_stream("other", channel)).unwrap();
    // Each window following the limits: the view first.
    for (label, messages) in [(MAIN, &main), (SETTINGS, &settings)] {
        let first = messages.next_limits();
        assert!(
            matches!(first, HostEvent::LimitsViewChanged(_)),
            "{label}'s first message is not the view: {first:?}"
        );
        let view_json = serde_json::to_string(&first).unwrap();
        assert!(pump::AppEvents::of(label).carries(Feed::Limits, &view_json));
    }
    let other_window = bridge.window("other").unwrap();
    assert_eq!(
        lock(&other_window).state().feeds().restart_id(Feed::Limits),
        None,
        "a window other than main and settings follows the limits"
    );

    // The start-up read answers: no account is signed in, which changes the view.
    reads.answer(ReadOutcome::SignedOut);
    for (label, messages) in [(MAIN, &main), (SETTINGS, &settings)] {
        let changed = next_view(messages);
        assert!(
            changed.0.contains_key(&AgentKind::Claude),
            "{label}: {changed:?}"
        );
        // Two messages on its channel: the view at open and the change.
        assert_eq!(messages.limits_received(), 2, "{label}");
        let window = bridge.window(label).unwrap();
        assert!(lock(&window).state().unacked_bytes() > 0, "{label}");
        let refusal = block_on(bridge.ack(label, Vec::new(), 3)).unwrap_err();
        assert_eq!(
            refusal.code(),
            ErrorCode::CommandInvalid,
            "{label}: {refusal:?}"
        );
        block_on(bridge.ack(label, Vec::new(), 2)).unwrap();
        assert_eq!(lock(&window).state().unacked_bytes(), 0, "{label}");
        assert_eq!(messages.drain(), Vec::<String>::new(), "{label}");
    }
    // `main` and settings got the change, so the other window would have by now.
    assert_eq!(other.drain(), Vec::<String>::new());
    assert_eq!(other.limits_received(), 0, "the other window got limits");
}

/// The watchdog, with the view unacknowledged for 5 s, renews the limits subscription: the new
/// one starts with the current view, and later changes follow on it.
#[test]
fn a_watchdog_fire_renews_the_limits_with_the_current_view() {
    let scratch = Scratch::with_limits("limits-watchdog");
    let (host, reads, _clock) = start(&scratch);
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    let window = bridge.window(MAIN).unwrap();
    let before = lock(&window).state().feeds().restart_id(Feed::Limits);
    assert!(before.is_some());

    pump::tick_at(&window, window::WATCHDOG * 2);
    let renewed = next_view(&messages);
    assert!(renewed.0.contains_key(&AgentKind::Claude), "{renewed:?}");
    assert_ne!(
        lock(&window).state().feeds().restart_id(Feed::Limits),
        before
    );

    reads.answer(ReadOutcome::SignedOut);
    assert_ne!(next_view(&messages), renewed);
    assert_eq!(messages.limits_received(), 3);
}

/// A window's focus refresh is `RefreshLimits { agent: None, why: Shown }`: the host reads
/// again only once the last read is 5 minutes old, so a focus just after one reads nothing.
#[test]
fn a_focus_refresh_reads_limits_older_than_five_minutes() {
    let scratch = Scratch::with_limits("limits-focus");
    let (host, reads, clock) = start(&scratch);
    let bridge = host.bridge();
    // The start-up read ends before the refreshes: its view change shows it did.
    let messages = open(&bridge, MAIN);
    reads.answer(ReadOutcome::SignedOut);
    let _changed = next_view(&messages);

    // The host starts a read as it takes the command, so the count is final once it answers.
    assert_eq!(block_on(bridge.shown()).unwrap(), CommandOutcome::Done);
    assert_eq!(reads.count(), 1, "the last read is under 5 minutes old");
    clock.advance(SHOWN_AFTER);
    assert_eq!(block_on(bridge.shown()).unwrap(), CommandOutcome::Done);
    assert_eq!(
        reads.count(),
        2,
        "a focus 5 minutes after the last read reads"
    );
}

/// The app's window-event handler, on the mock runtime: the `main` and settings windows'
/// `Focused(true)` each dispatch the refresh, which reads once. Focus lost, or another window's
/// focus, starts no task.
#[test]
fn main_and_settings_focus_dispatch_a_shown_refresh_and_no_other_window_s() {
    let scratch = Scratch::with_limits("limits-focus-event");
    let (host, reads, clock) = start(&scratch);
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    reads.answer(ReadOutcome::SignedOut);
    let _changed = next_view(&messages);
    let app = mock_builder()
        .build(crate::context::<MockRuntime>())
        .unwrap();
    app.manage(bridge);
    let window = |label: &str| {
        WebviewWindowBuilder::new(&app, label, WebviewUrl::default())
            .build()
            .unwrap()
    };
    let (main, settings, other) = (
        window(crate::MAIN_WINDOW),
        window(SETTINGS),
        window("other"),
    );
    let focus = |window: &WebviewWindow<MockRuntime>, focused: bool| {
        crate::on_window_event(&window.as_ref().window(), &WindowEvent::Focused(focused))
    };
    clock.advance(SHOWN_AFTER);

    assert!(
        focus(&main, false).is_none(),
        "focus lost dispatches nothing"
    );
    assert!(
        focus(&other, true).is_none(),
        "another window's focus dispatches nothing"
    );
    let task = focus(&main, true).expect("the main window's focus dispatches");
    block_on(task).unwrap();
    // The host starts the read as it takes the command, so the count is final once it answers.
    assert_eq!(reads.count(), 2, "one read for one focus");
    // That read ends (its view, read later, shows it did) and is 5 minutes old by the next focus.
    reads.answer(ReadOutcome::SignedOut);
    let _changed = next_view(&messages);
    clock.advance(SHOWN_AFTER);
    let task = focus(&settings, true).expect("the settings window's focus dispatches");
    block_on(task).unwrap();
    assert_eq!(reads.count(), 3, "one read for the settings window's focus");
}

/// A focus before the host has started: the handler returns at once with a task that waits on
/// the host's cell, and once the host is in it, that task dispatches its one refresh, which
/// reads (the start-up read is answered and 5 minutes old by then).
#[test]
fn a_focus_before_the_host_starts_refreshes_once_it_has() {
    let scratch = Scratch::with_limits("limits-focus-before-start");
    let reads = Reads::new();
    let clock = ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
    let (cell, start) = HostCell::new();
    let app = mock_builder()
        .build(crate::context::<MockRuntime>())
        .unwrap();
    app.manage(Bridge::new(Arc::clone(&cell)));
    let main = WebviewWindowBuilder::new(&app, crate::MAIN_WINDOW, WebviewUrl::default())
        .build()
        .unwrap();
    let focused = WindowEvent::Focused(true);
    let mut task = crate::on_window_event(&main.as_ref().window(), &focused)
        .expect("the main window's focus dispatches");
    assert!(
        (&mut task).now_or_never().is_none(),
        "the refresh ran with no host"
    );

    let config = scratch
        .config(Vec::new())
        .with_limits_reader(Arc::<Reads>::clone(&reads))
        .with_clock(Arc::new(clock.clone()));
    let (host, join) = Host::start(config).unwrap();
    let mut views = block_on(host.subscribe_limits(PRINCIPAL)).unwrap();
    let _first = block_on(views.next()).unwrap();
    reads.until_count(1);
    // The start-up read ends (its view change shows it did), then 5 minutes pass.
    reads.answer(ReadOutcome::SignedOut);
    let _changed = block_on(views.next()).unwrap();
    clock.advance(SHOWN_AFTER);
    assert_eq!(reads.count(), 1);
    start.finish(Ok((host, join)));

    block_on(task).unwrap();
    // The host starts the read as it takes the command, so the count is final once it answers.
    assert_eq!(reads.count(), 2, "one read for the one focus");
    drop(views);
    drop(app);
    stop(&cell);
}

/// `used` as a window's share.
fn usage_percent(used: usize) -> Finite {
    Finite::new(f64::from(u32::try_from(used).unwrap())).unwrap()
}

/// A good read of a Claude plan whose one window is `used` percent full.
fn usage(used: usize) -> ReadOutcome {
    let used = usage_percent(used);
    let window = LimitWindow {
        id: "five_hour".to_owned(),
        kind: WindowKind::Session,
        label: "5-hour limit".to_owned(),
        used_percent: used,
        resets_at: None,
        duration_mins: Some(300),
        warning: false,
        model: None,
    };
    let limits = AccountLimits {
        agent: AgentKind::Claude,
        account: AccountRef("ada@example.com".to_owned()),
        plan: Some(PlanLabel("Max".to_owned())),
        billing: Billing::Plan,
        windows: vec![window],
        at_limit: AtLimitBehaviour::Pauses,
        source: LimitSource::Fresh {
            checked_at: Timestamp::new(1_790_000_000_000).unwrap(),
        },
    };
    ReadOutcome::Usage {
        limits,
        reached: None,
    }
}

/// A `TryAgain` refresh for `agent`, as Plan and usage's button sends it.
fn try_again(agent: Option<AgentKind>) -> CommandEnvelope {
    envelope(HostCommand::RefreshLimits {
        agent,
        why: RefreshWhy::TryAgain,
    })
}

/// The settings window's refreshes, at most one per agent per 10 s (0028's 2026-11-03
/// amendment): a second for Claude within 10 s answers `Done` and reads nothing, one for every
/// agent (another key) passes, and one 10 s after the first passes. `main`'s are not throttled.
/// Time is given, never slept.
#[test]
fn the_settings_window_refreshes_an_agent_at_most_once_in_ten_seconds() {
    let scratch = Scratch::with_limits("limits-throttle");
    let (host, reads, _clock) = start(&scratch);
    let bridge = host.bridge();
    let messages = open(&bridge, MAIN);
    // Each read ends (the host publishes its account's share) before the next refresh: the
    // host starts no read while one is in flight. Read n answers n% used.
    let answer = || {
        let used = reads.count();
        reads.answer(usage(used));
        let wanted = usage_percent(used);
        while !matches!(messages.next_limits(), HostEvent::AccountLimitsChanged(read)
            if read.windows[0].used_percent == wanted)
        {}
    };
    answer();
    let start = pump::now();
    let at = |secs| start + Duration::from_secs(secs);
    let claude = Some(AgentKind::Claude);
    let dispatch = |label, agent, now| block_on(bridge.dispatch_at(label, try_again(agent), now));

    // The host starts a read as it takes the command, so the count is final once it answers.
    assert_eq!(
        dispatch(SETTINGS, claude, at(0)).unwrap(),
        CommandOutcome::Done
    );
    assert_eq!(reads.count(), 2);
    answer();
    assert_eq!(
        dispatch(SETTINGS, claude, at(9)).unwrap(),
        CommandOutcome::Done
    );
    assert_eq!(
        reads.count(),
        2,
        "a second refresh of Claude within 10 s read"
    );
    assert_eq!(
        dispatch(SETTINGS, None, at(9)).unwrap(),
        CommandOutcome::Done
    );
    assert_eq!(reads.count(), 3, "a refresh of every agent is its own key");
    answer();
    assert_eq!(
        dispatch(SETTINGS, claude, at(10)).unwrap(),
        CommandOutcome::Done
    );
    assert_eq!(
        reads.count(),
        4,
        "a refresh of Claude 10 s after the last did not read"
    );
    for count in [5, 6] {
        answer();
        assert_eq!(
            dispatch(MAIN, claude, at(10)).unwrap(),
            CommandOutcome::Done
        );
        assert_eq!(reads.count(), count, "main's refresh was throttled");
    }
}

/// The throttle keeps each agent apart: Codex and every agent pass while Claude is held back,
/// and Claude passes again at exactly 10 s, which restarts its 10 s.
#[test]
fn the_refresh_throttle_keeps_each_agent_apart() {
    let throttle = throttle::RefreshThrottle::default();
    let start = pump::now();
    let claude = Some(AgentKind::Claude);
    assert!(throttle.pass(claude, start));
    assert!(!throttle.pass(claude, start + Duration::from_millis(9_999)));
    assert!(throttle.pass(Some(AgentKind::Codex), start + Duration::from_secs(1)));
    assert!(throttle.pass(None, start + Duration::from_secs(1)));
    assert!(throttle.pass(claude, start + REFRESH_GAP));
    assert!(!throttle.pass(claude, start + REFRESH_GAP + Duration::from_secs(9)));
}
