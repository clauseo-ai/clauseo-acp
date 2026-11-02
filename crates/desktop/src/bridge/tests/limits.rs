//! The limits on each window's channel (`open_stream` follows them), and the refresh the main
//! window's focus asks for (`on_window_event` in `lib.rs`, then `Bridge::shown`). A scripted
//! reader stands in for the limits readers: it records each read and answers only when the test
//! says, so no reader process starts.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use clauseo_host::ManualClock;
use clauseo_limits::{ReadFailure, ReadLimits, ReadOutcome};
use clauseo_protocol::{AgentKind, LimitsView, Timestamp};
use futures::channel::oneshot;
use tauri::test::{MockRuntime, mock_builder};
use tauri::{Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use super::*;
use crate::test_support::{Messages, Running};

/// The second window's label.
const SECOND: &str = "second";

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

/// Each window's channel gets the current view at open and then each change, and those
/// messages count toward its acks like any other.
#[test]
fn each_window_gets_the_limits_view_then_each_change_and_acks_count_them() {
    let scratch = Scratch::with_limits("limits-windows");
    let (host, reads, _clock) = start(&scratch);
    let bridge = host.bridge();
    // `open` checks each window's first message is the view.
    let main = open(&bridge, MAIN);
    let second = open(&bridge, SECOND);

    // The start-up read answers: no account is signed in, which changes the view.
    reads.answer(ReadOutcome::SignedOut);
    let changed = next_view(&main);
    assert!(changed.0.contains_key(&AgentKind::Claude), "{changed:?}");
    assert_eq!(next_view(&second), changed);

    // Two messages each: the view at open and the change.
    let window = bridge.window(MAIN).unwrap();
    assert!(lock(&window).state().unacked_bytes() > 0);
    let refusal = block_on(bridge.ack(MAIN, Vec::new(), 3)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");
    block_on(bridge.ack(MAIN, Vec::new(), 2)).unwrap();
    assert_eq!(lock(&window).state().unacked_bytes(), 0);
    assert_eq!(main.drain(), Vec::<String>::new());
    assert_eq!(second.drain(), Vec::<String>::new());
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

/// The app's window-event handler, on the mock runtime: only the `main` window's
/// `Focused(true)` dispatches the refresh, and it reads once. Focus lost, or another window's
/// focus, starts no task.
#[test]
fn only_the_main_window_s_focus_dispatches_a_shown_refresh() {
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
    let (main, other) = (window(crate::MAIN_WINDOW), window("other"));
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
        "another window dispatches nothing"
    );
    let task = focus(&main, true).expect("the main window's focus dispatches");
    block_on(task).unwrap();
    // The host starts the read as it takes the command, so the count is final once it answers.
    assert_eq!(reads.count(), 2, "one read for one focus");
}
