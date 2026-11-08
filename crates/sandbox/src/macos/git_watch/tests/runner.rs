//! The runner's order and its guards: the kill goes before any rescan, a stuck check ends the
//! session by the watchdog, a folder watched only before and after, and the notes folder's
//! watch ending the session it shares with the matter's.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::super::aside::Aside;
use super::super::run::{BUDGET, Busy, Flow, Mode, Msg, Runner, Session, start_with};
use super::super::{Launch, Watch, flags};
use super::fake::{
    FakeFs, FakeSession, MOVED_IN, MemStore, Notices, SET_ASIDE, assert_moved, event, watch,
};
use crate::{Clock, Denial, DenialSink, SystemClock};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Waits up to 5 s for `done`.
fn within(what: &str, done: impl Fn() -> bool) {
    let clock = SystemClock::new();
    while !done() {
        assert!(clock.now() < Duration::from_secs(5), "{what} within 5 s");
        async_io::block_on(async_io::Timer::after(ms(20)));
    }
}

/// A session that notes how many folders the fake had listed when its kill came.
struct Counting {
    fs: FakeFs,
    at_kill: Mutex<Option<u64>>,
}

impl Session for Counting {
    fn gone(&self) -> bool {
        self.at_kill.lock().unwrap().is_some()
    }

    fn end(&self) {
        self.at_kill
            .lock()
            .unwrap()
            .get_or_insert(self.fs.listings());
    }
}

#[test]
fn a_find_kills_before_the_batchs_rescan_lists_anything() {
    let fs = FakeFs::new();
    for n in 0..50 {
        fs.dir(&format!("/m/d{n}"));
    }
    let (watch, _notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO);
    let session = Arc::new(Counting {
        fs: fs.clone(),
        at_kill: Mutex::new(None),
    });
    let attach = Msg::Attach(Arc::clone(&session) as Arc<dyn Session>);
    let _ = runner.input(Some(attach), Duration::ZERO);
    fs.dir("/m/foo");
    fs.repo("/m/foo/.git");
    let before = fs.listings();
    // The rescan flag comes first in the batch; the find after it ends the input.
    let batch = vec![
        event("/m", flags::MUST_SCAN_SUB_DIRS),
        event("/m/foo", MOVED_IN),
    ];
    assert_eq!(runner.input(Some(Msg::Events(batch)), ms(100)), Flow::Done);
    let at_kill = session.at_kill.lock().unwrap().unwrap();
    // The find's own check lists its parent and the folder moved in; a full scan lists 52+.
    assert!(
        at_kill - before <= 3,
        "{} listings before the kill",
        at_kill - before
    );
    assert_eq!(
        fs.listings(),
        at_kill,
        "and none after it, before the final scan"
    );
}

#[test]
fn nothing_more_is_checked_while_the_session_is_ending() {
    let fs = FakeFs::new();
    let (watch, _notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO);
    let session = FakeSession::default();
    let _ = runner.input(Some(Msg::Attach(Arc::new(session.clone()))), Duration::ZERO);
    fs.repo("/m/.git");
    let found = Msg::Events(vec![event("/m/.git", MOVED_IN)]);
    assert_eq!(runner.input(Some(found), ms(100)), Flow::Go);
    let lists = fs.listings();
    let rescan = Msg::Events(vec![event("/m", flags::MUST_SCAN_SUB_DIRS)]);
    assert_eq!(runner.input(Some(rescan), ms(200)), Flow::Go);
    assert_eq!(runner.input(None, Duration::from_secs(31)), Flow::Done);
    assert_eq!(fs.listings(), lists, "no rescan or 30 s pass while it ends");
    assert!(
        session.ends.load(Ordering::SeqCst) >= 2,
        "the kill went again"
    );
}

/// A clock the test sets.
#[derive(Default)]
struct Manual(AtomicU64);

impl Clock for Manual {
    fn now(&self) -> Duration {
        ms(self.0.load(Ordering::SeqCst))
    }
}

#[test]
fn the_watchdog_ends_a_session_whose_check_runs_past_its_budget() {
    let clock = Arc::new(Manual::default());
    let busy = Busy::new(Arc::clone(&clock) as Arc<dyn Clock>);
    let session = FakeSession::default();
    busy.attach(&(Arc::new(session.clone()) as Arc<dyn Session>));
    assert!(!busy.look(BUDGET), "no check running");
    busy.start();
    clock.0.store(1_999, Ordering::SeqCst);
    assert!(!busy.look(BUDGET), "within its budget");
    assert!(!busy.stop());
    busy.start();
    clock.0.store(1_999 + 2_000, Ordering::SeqCst);
    assert!(busy.look(BUDGET));
    assert_eq!(session.ends.load(Ordering::SeqCst), 1);
    assert!(!busy.look(BUDGET), "once");
    assert!(busy.stop(), "the runner finds it ended");
    // A session attached after the watchdog gave up is ended at once.
    let late = FakeSession::default();
    busy.attach(&(Arc::new(late.clone()) as Arc<dyn Session>));
    assert_eq!(late.ends.load(Ordering::SeqCst), 1);
}

#[test]
fn a_runner_whose_check_overran_ends_the_session_and_tells_the_lawyer() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let clock = Arc::new(Manual::default());
    let busy = Busy::new(Arc::clone(&clock) as Arc<dyn Clock>);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO).with_busy(Arc::clone(&busy));
    let session = FakeSession {
        dies: true,
        ..FakeSession::default()
    };
    let _ = runner.input(Some(Msg::Attach(Arc::new(session))), Duration::ZERO);
    fs.repo("/m/.git");
    // The watchdog gave up on the check this input runs.
    busy.start();
    clock.0.store(5_000, Ordering::SeqCst);
    assert!(busy.look(BUDGET));
    assert_eq!(runner.input(None, ms(100)), Flow::Done);
    assert_eq!(notices.taken(), [Denial::GitWatchLost]);
    assert_eq!(runner.finish().stuck().len(), 0);
    assert!(!fs.exists("/m/.git"), "the final scan ran");
}

#[test]
fn a_listing_that_blocks_is_ended_by_the_hosts_watchdog() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let (tx, rx) = std::sync::mpsc::channel::<Sender<Msg>>();
    let source = move |_: &[PathBuf], events: Sender<Msg>| -> Result<(), String> {
        tx.send(events).unwrap();
        Ok(())
    };
    let store = MemStore::default();
    let watching = start_with(watch, "w".to_owned(), store, None, Mode::Live, source).unwrap();
    let session = FakeSession {
        dies: true,
        ..FakeSession::default()
    };
    watching.attach(session.clone());
    let events = rx.recv().unwrap();
    // A folder moved in whose listing hangs, as an autofs mount behind a link would.
    fs.dir("/m/slow");
    let open = fs.block_listing("/m/slow");
    events
        .send(Msg::Events(vec![event("/m/slow", MOVED_IN)]))
        .unwrap();
    let clock = SystemClock::new();
    within("the watchdog's kill", || {
        session.ends.load(Ordering::SeqCst) > 0
    });
    let took = clock.now();
    assert!(
        took >= BUDGET.saturating_sub(ms(200)),
        "ended after {took:?}"
    );
    assert!(
        !open.load(Ordering::SeqCst),
        "while the listing still hangs"
    );
    // The listing returns; the runner sees the session ended and runs the final scan.
    fs.dir("/m/other");
    fs.repo("/m/other/.git");
    open.store(true, Ordering::SeqCst);
    within("the final scan", || !fs.exists("/m/other/.git"));
    within("the notices", || notices.count() >= 2);
    let got = notices.taken();
    assert_eq!(got[0], Denial::GitWatchLost, "{got:?}");
    assert!(matches!(got[1], Denial::GitSetAside { .. }), "{got:?}");
}

#[test]
fn a_folder_watched_after_only_starts_no_stream_and_sets_aside_at_the_end() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let called = Arc::new(AtomicBool::new(false));
    let calls = Arc::clone(&called);
    let source = move |_: &[PathBuf], _: Sender<Msg>| -> Result<(), String> {
        calls.store(true, Ordering::SeqCst);
        Err("never called".to_owned())
    };
    let canary = Some(PathBuf::from(format!("{SET_ASIDE}/watch-canary")));
    let store = MemStore::default();
    let watching = start_with(
        watch,
        "a".to_owned(),
        store,
        canary,
        Mode::AfterOnly,
        source,
    );
    let watching = watching.unwrap();
    assert!(!called.load(Ordering::SeqCst), "no stream");
    let session = FakeSession::default();
    watching.attach(session.clone());
    fs.repo("/m/.git");
    async_io::block_on(async_io::Timer::after(ms(300)));
    assert_eq!(
        session.ends.load(Ordering::SeqCst),
        0,
        "nothing ends the run"
    );
    assert!(fs.exists("/m/.git"));
    assert_eq!(fs.touched(), Vec::<PathBuf>::new(), "no canary");
    session.gone.store(true, Ordering::SeqCst);
    within("the final scan", || !fs.exists("/m/.git"));
    within("its notice", || notices.count() > 0);
    assert_moved(&notices, ".git");
}

/// The streams the fake sources started: each one's folder and sender.
type Streams = Arc<Mutex<Vec<(PathBuf, Sender<Msg>)>>>;

/// A watch of `/notes` for session `session`, empty launch.
fn notes_watch(fs: &FakeFs, notices: &Arc<Notices>) -> Watch<FakeFs> {
    let aside = Aside::new(
        PathBuf::from("/notes"),
        PathBuf::from(SET_ASIDE),
        "clauseo-1-0-n-notes".to_owned(),
        Arc::clone(notices) as Arc<dyn DenialSink>,
    );
    Watch::new(
        fs.clone(),
        PathBuf::from("/notes"),
        aside,
        &Launch::default(),
    )
}

#[test]
fn a_find_in_the_notes_folder_ends_the_session_and_is_set_aside() {
    let fs = FakeFs::new();
    fs.dir("/notes");
    fs.file("/notes/brief.md");
    let (matter, matter_notices) = watch(&fs, &[]);
    let notes_notices = Arc::new(Notices::default());
    let notes = notes_watch(&fs, &notes_notices);
    let senders = Arc::new(Mutex::new(Vec::new()));
    let source = |into: &Streams| {
        let into = Arc::clone(into);
        move |roots: &[PathBuf], events: Sender<Msg>| -> Result<(), String> {
            into.lock().unwrap().push((roots[0].clone(), events));
            Ok(())
        }
    };
    let (matter_store, notes_store) = (MemStore::default(), MemStore::default());
    let one = start_with(
        matter,
        "m".to_owned(),
        matter_store,
        None,
        Mode::Live,
        source(&senders),
    );
    let two = start_with(
        notes,
        "n".to_owned(),
        notes_store,
        None,
        Mode::Live,
        source(&senders),
    );
    let watching = one.unwrap().and(two.unwrap());
    let session = FakeSession {
        dies: true,
        ..FakeSession::default()
    };
    watching.attach(session.clone());
    let streams = senders.lock().unwrap().clone();
    let roots: Vec<&Path> = streams.iter().map(|(root, _)| root.as_path()).collect();
    assert_eq!(
        roots,
        [Path::new("/m"), Path::new("/notes")],
        "a stream each"
    );
    // A repository renamed into the notes folder under another name.
    fs.dir("/notes/foo");
    fs.repo("/notes/foo/.git");
    streams[1]
        .1
        .send(Msg::Events(vec![event("/notes/foo", MOVED_IN)]))
        .unwrap();
    within("the session's end", || {
        session.ends.load(Ordering::SeqCst) > 0
    });
    within("the set-aside", || !fs.exists("/notes/foo/.git"));
    within("its notice", || notes_notices.count() > 0);
    assert_moved(&notes_notices, "foo/.git");
    assert_eq!(matter_notices.count(), 0, "nothing in the matter");
}
