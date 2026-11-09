//! The pairing fails closed when it loses track: a file ID still in its window pushed out of the
//! bounded list (a flood of folders leaving), too many folders held, or any watch's overflow
//! lose it for 10 s; a pairing watch that can't be armed or breaks loses it for the session.
//! While lost, every fresh folder gone and unpaired counts, those held when it was lost too.
//! In the watch's loop, what pairs late is counted on the next record or idle tick, and ends
//! the session at [`GONE_LIMIT`].

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::super::pairs::{PAIR_MAX, PAIR_WINDOW, Paired, Pairs};
use super::super::{
    Changes, GONE_LIMIT, Guard, Layout, Next, PAIRING_LOST, Pulse, Session, Source, Tell, feed,
    lose_pairing, pairing_started, run_pairing, watch,
};
use super::Log;
use super::pairs::{NOW, checked, left_temp, matter};
use crate::windows::denial_pipe::Record;
use crate::windows::runner::stderr::{Detail, RunnerKind};
use crate::windows::set_aside::{Failed, Hooks, Live};

const MS: Duration = Duration::from_millis(1);
const SECOND: Duration = Duration::from_secs(1);

fn x() -> Vec<PathBuf> {
    vec![PathBuf::from("x")]
}

fn folders(from: u64, count: u64) -> Vec<(PathBuf, u64)> {
    (from..from + count)
        .map(|id| (PathBuf::from(format!("f{id}")), id))
        .collect()
}

/// A flood of folders leaving pushes out a file ID still in its window: the pairing is lost
/// for [`PAIR_WINDOW`], and a gone folder checked then counts, unpaired.
#[test]
fn a_file_id_pushed_out_within_its_window_loses_the_pairing_for_the_window() {
    let pairs = Arc::new(Pairs::default());
    left_temp(&pairs, NOW);
    pairs.note(
        NOW + SECOND,
        Path::new("temp"),
        &folders(1000, PAIR_MAX as u64),
    );
    assert!(!pairs.paired(NOW + SECOND, 7, &Path::new("matter").join("x")));
    let until = NOW + SECOND + PAIR_WINDOW;
    assert!(pairs.lost(NOW + SECOND) && pairs.lost(until));
    assert!(!pairs.lost(until + MS));
    let (mut live, fresh) = (
        Live {
            unpaired: vec![PathBuf::from("y")],
            ..Live::default()
        },
        vec![(PathBuf::from("y"), 8)],
    );
    super::super::pairs::promote(&mut live, &fresh, Some(&matter(&pairs)), until);
    assert_eq!(live.gone, [PathBuf::from("y")]);
    assert_eq!(
        checked(&pairs, until + MS),
        Vec::<PathBuf>::new(),
        "held again"
    );

    // File IDs past their window go without losing anything.
    let pairs = Arc::new(Pairs::default());
    left_temp(&pairs, NOW);
    let later = NOW + PAIR_WINDOW + MS;
    pairs.note(later, Path::new("temp"), &folders(1000, PAIR_MAX as u64));
    assert!(!pairs.lost(later));
}

/// A folder held when the pairing loses track counts then, unpaired; so does one held past
/// [`PAIR_MAX`] others.
#[test]
fn folders_held_when_the_pairing_loses_track_count() {
    let pairs = Arc::new(Pairs::default());
    assert_eq!(checked(&pairs, NOW), Vec::<PathBuf>::new());
    pairs.lose(NOW + 3 * SECOND);
    assert_eq!(matter(&pairs).take(NOW + 3 * SECOND), x());

    let pairs = Arc::new(Pairs::default());
    for id in 0..PAIR_MAX as u64 {
        pairs.hold(
            NOW,
            Path::new("matter"),
            Path::new(&format!("h{id}")),
            10_000 + id,
        );
    }
    assert!(!pairs.lost(NOW));
    assert_eq!(checked(&pairs, NOW).len(), PAIR_MAX + 1, "all held count");
    assert!(pairs.lost(NOW));
}

/// A guard whose fresh folders are all gone by their check.
struct AllGone;

impl Guard for AllGone {
    fn changed(&self, changes: &Changes, _hooks: &impl Hooks) -> Live {
        Live {
            unpaired: changes.fresh_paths(),
            gone: changes.moved_folders.clone(),
            ..Live::default()
        }
    }
    fn rescan(&self, _hooks: &impl Hooks) -> Live {
        Live::default()
    }
    fn retry(&self, _failed: &[Failed], _hooks: &impl Hooks) -> Live {
        Live::default()
    }
}

/// A session that shares `paired` and reads `clock`.
struct Pairing<'a> {
    log: &'a Log,
    paired: Option<Paired>,
    clock: &'a Cell<Duration>,
    pulse: Pulse,
}

impl Session for Pairing<'_> {
    fn end(&self) -> bool {
        self.log.push("end");
        true
    }
    fn say(&self, _kind: RunnerKind, _detail: Detail) {}
    fn record(&self, record: &Record) {
        if let Record::Notice { text } = record {
            self.log.push(text.clone());
        }
    }
    fn pause(&self, _wait: Duration) -> bool {
        true
    }
    fn settle(&self, _wait: Duration) {}
    fn now(&self) -> Duration {
        self.clock.get()
    }
    fn pulse(&self) -> &Pulse {
        &self.pulse
    }
    fn paired(&self) -> Option<&Paired> {
        self.paired.as_ref()
    }
}

/// Steps run in turn, each giving what the watch reads next (and doing what other watches do
/// meanwhile), then the stop.
struct Steps<'a>(RefCell<VecDeque<Box<dyn FnOnce() -> Next + 'a>>>);

impl Source for &Steps<'_> {
    fn next(&mut self) -> Next {
        let step = self.0.borrow_mut().pop_front();
        step.map_or(Next::Stopped, |step| step())
    }
    fn layout(&self) -> Layout {
        Layout::Extended
    }
}

fn fresh(name: &str, id: u64) -> Next {
    Next::Changed(Changes {
        added: vec![PathBuf::from(name)],
        fresh_folders: vec![(PathBuf::from(name), id)],
        ..Changes::default()
    })
}

/// Runs the watch over `steps` with `pairs` and `clock`; whether it ended the session.
fn ended(
    steps: Vec<Box<dyn FnOnce() -> Next + '_>>,
    pairs: &Arc<Pairs>,
    clock: &Cell<Duration>,
) -> bool {
    let log = Log::default();
    let session = Pairing {
        log: &log,
        paired: Some(matter(pairs)),
        clock,
        pulse: Pulse::default(),
    };
    let steps = Steps(RefCell::new(steps.into()));
    watch(&steps, &AllGone, &session);
    log.lines().contains(&"end".to_owned())
}

/// In the loop: forged folders moved in from temp whose leaving records the temp watch reads
/// seconds after their checks are counted on the matter watch's idle ticks, and end the session
/// at [`GONE_LIMIT`]; as many `mkdir`s gone, held past the window, end nothing.
#[test]
fn folders_paired_after_their_checks_end_the_session_on_idle_ticks() {
    let (pairs, clock) = (Arc::new(Pairs::default()), Cell::new(NOW));
    let mut steps: Vec<Box<dyn FnOnce() -> Next + '_>> = Vec::new();
    for id in 1..=u64::from(GONE_LIMIT) {
        steps.push(Box::new(move || fresh(&format!("m{id}"), id)));
        let (pairs, clock) = (Arc::clone(&pairs), &clock);
        steps.push(Box::new(move || {
            clock.set(clock.get() + 5 * SECOND);
            pairs.note(clock.get(), Path::new("temp"), &[(PathBuf::from("t"), id)]);
            Next::Idle
        }));
    }
    assert!(ended(steps, &pairs, &clock));

    let (pairs, clock) = (Arc::new(Pairs::default()), Cell::new(NOW));
    let mut steps: Vec<Box<dyn FnOnce() -> Next + '_>> = Vec::new();
    for id in 1..=u64::from(GONE_LIMIT) + 1 {
        steps.push(Box::new(move || fresh(&format!("tmp{id}"), 50 + id)));
        let clock = &clock;
        steps.push(Box::new(move || {
            clock.set(clock.get() + PAIR_WINDOW + MS);
            Next::Idle
        }));
    }
    assert!(!ended(steps, &pairs, &clock));
}

/// In the loop: a matter overflow loses the pairing, so the `mkdir`s gone after it count and
/// end the session; one renamed in counts as it always did.
#[test]
fn a_matter_overflow_loses_the_pairing_in_the_loop() {
    let (pairs, clock) = (Arc::new(Pairs::default()), Cell::new(NOW));
    let mut steps: Vec<Box<dyn FnOnce() -> Next + '_>> = vec![Box::new(|| Next::Overflow)];
    for id in 1..=u64::from(GONE_LIMIT) {
        steps.push(Box::new(move || fresh(&format!("m{id}"), id)));
    }
    assert!(ended(steps, &pairs, &clock));
    assert!(pairs.lost(NOW + PAIR_WINDOW));

    let (pairs, clock) = (Arc::new(Pairs::default()), Cell::new(NOW));
    let renamed = || {
        Next::Changed(Changes {
            added: vec![PathBuf::from("r")],
            moved_folders: vec![PathBuf::from("r")],
            ..Changes::default()
        })
    };
    let steps: Vec<Box<dyn FnOnce() -> Next + '_>> =
        vec![Box::new(renamed), Box::new(renamed), Box::new(renamed)];
    assert!(ended(steps, &pairs, &clock));
}

/// Reports given in turn, then the stop, as read with the extended records, or the plain ones
/// with `Script::plain`.
struct Script(VecDeque<Next>, Layout);

impl Script {
    fn new(reports: Vec<Next>) -> Script {
        Script(reports.into(), Layout::Extended)
    }
    fn plain(reports: Vec<Next>) -> Script {
        Script(reports.into(), Layout::Plain)
    }
}

impl Source for Script {
    fn next(&mut self) -> Next {
        self.0.pop_front().unwrap_or(Next::Stopped)
    }
    fn layout(&self) -> Layout {
        self.1
    }
}

/// The notices a pairing watch sent on the denial pipe, by their words.
/// The notices on the denial pipe, and the lines on stderr.
#[derive(Default)]
struct Told(RefCell<Vec<String>>, RefCell<Vec<String>>);

impl Tell for Told {
    fn record(&self, record: &Record) {
        if let Record::Notice { text } = record {
            self.0.borrow_mut().push(text.clone());
        }
    }

    fn say(&self, _kind: RunnerKind, detail: Detail) {
        self.1.borrow_mut().push(detail.text().to_owned());
    }
}

impl Told {
    fn said(&self) -> Vec<String> {
        self.0.borrow().clone()
    }

    fn on_stderr(&self) -> Vec<String> {
        self.1.borrow().clone()
    }
}

fn temp(pairs: &Arc<Pairs>) -> Paired {
    Paired {
        pairs: Arc::clone(pairs),
        root: PathBuf::from("temp"),
    }
}

/// A pairing watch (state, temp) notes the folders that left; its overflow loses the pairing
/// for [`PAIR_WINDOW`].
#[test]
fn a_pairing_watchs_overflow_loses_the_pairing_for_the_window() {
    let pairs = Arc::new(Pairs::default());
    let left = |name: &str, id: u64| {
        Next::Changed(Changes {
            left: vec![(PathBuf::from(name), id)],
            ..Changes::default()
        })
    };
    let reports = vec![left("a", 1), Next::Idle, Next::Overflow, left("b", 2)];
    let told = Told::default();
    feed(Ok(Script::new(reports)), &temp(&pairs), || NOW, &told);
    assert_eq!(
        told.said(),
        Vec::<String>::new(),
        "lost for the window only"
    );
    let made = Path::new("matter").join("x");
    assert!(pairs.paired(NOW, 1, &made) && pairs.paired(NOW, 2, &made));
    assert!(pairs.lost(NOW + PAIR_WINDOW) && !pairs.lost(NOW + PAIR_WINDOW + MS));
    assert_eq!(checked(&pairs, NOW + PAIR_WINDOW), x());
}

/// A state or temp watch that breaks, or couldn't be armed, loses the pairing for the session:
/// what was held counts, and so does every fresh folder gone after, however late.
#[test]
fn a_pairing_watch_that_breaks_or_cant_be_armed_loses_the_pairing_for_the_session() {
    let pairs = Arc::new(Pairs::default());
    assert_eq!(checked(&pairs, NOW), Vec::<PathBuf>::new());
    let reports = vec![
        Next::Broken(io::Error::from_raw_os_error(6)),
        Next::Changed(Changes {
            left: vec![(PathBuf::from("c"), 3)],
            ..Changes::default()
        }),
    ];
    let told = Told::default();
    feed(Ok(Script::new(reports)), &temp(&pairs), || NOW, &told);
    assert_eq!(told.said(), [PAIRING_LOST], "broke");
    assert!(
        !pairs.paired(NOW, 3, &Path::new("matter").join("x")),
        "nothing after it broke"
    );
    assert_eq!(matter(&pairs).take(NOW), x());
    assert!(pairs.lost(NOW + 1000 * PAIR_WINDOW));

    let pairs = Arc::new(Pairs::default());
    let armed: io::Result<Script> = Err(io::Error::from_raw_os_error(5));
    let told = Told::default();
    feed(armed, &temp(&pairs), || NOW, &told);
    assert_eq!(told.said(), [PAIRING_LOST], "couldn't be armed");
    assert!(pairs.lost(NOW + 1000 * PAIR_WINDOW));
    assert_eq!(checked(&pairs, NOW + 1000 * PAIR_WINDOW), x());
}

/// A state or temp watch armed for the plain records (a network share: no attributes or file
/// IDs) notes nothing, so it loses the pairing for the session, as one that couldn't be armed.
#[test]
fn a_pairing_watch_armed_for_the_plain_records_loses_the_pairing_for_the_session() {
    let pairs = Arc::new(Pairs::default());
    let left = Next::Changed(Changes {
        left: vec![(PathBuf::from("d"), 4)],
        ..Changes::default()
    });
    let told = Told::default();
    feed(Ok(Script::plain(vec![left])), &temp(&pairs), || NOW, &told);
    assert_eq!(told.said(), [PAIRING_LOST], "plain records");
    assert!(
        !pairs.paired(NOW, 4, &Path::new("matter").join("x")),
        "nothing noted"
    );
    assert!(pairs.lost(NOW + 1000 * PAIR_WINDOW));
    assert_eq!(checked(&pairs, NOW + 1000 * PAIR_WINDOW), x());
}

/// The lawyer hears of the pairing lost for the session once, however many pairing watches lose
/// it and by whichever path: the state and temp watches share one pairing.
#[test]
fn the_pairing_lost_for_the_session_is_told_once_a_session() {
    let pairs = Arc::new(Pairs::default());
    let told = Told::default();
    let state = Paired {
        pairs: Arc::clone(&pairs),
        root: PathBuf::from("state"),
    };
    let broken = vec![Next::Broken(io::Error::from_raw_os_error(6))];
    feed(Ok(Script::new(broken)), &state, || NOW, &told);
    let armed: io::Result<Script> = Err(io::Error::from_raw_os_error(5));
    feed(armed, &temp(&pairs), || NOW, &told);
    feed(Ok(Script::plain(Vec::new())), &temp(&pairs), || NOW, &told);
    // A panicked watch, or one whose thread couldn't start (`outer/watch.rs`).
    run_pairing(&pairs, &told, || panic!("a pairing watch panics"));
    let thread: io::Result<()> = Err(io::Error::from_raw_os_error(8));
    assert_eq!(pairing_started(thread, &pairs, &told), None);
    lose_pairing(&pairs, &told);
    assert_eq!(told.said(), [PAIRING_LOST]);
    assert_eq!(
        told.on_stderr(),
        [format!("clauseo-sandbox-run: {PAIRING_LOST}")]
    );
    assert!(pairs.lost(NOW + 1000 * PAIR_WINDOW));
}

/// A pairing watch that panics, or whose thread couldn't start, loses the pairing on its own and
/// tells the lawyer on both ways; one that runs through or starts loses nothing.
#[test]
fn a_panicked_or_unstarted_pairing_watch_loses_the_pairing_and_says_so_on_both_ways() {
    let both = |told: &Told| {
        assert_eq!(told.said(), [PAIRING_LOST]);
        assert_eq!(
            told.on_stderr(),
            [format!("clauseo-sandbox-run: {PAIRING_LOST}")]
        );
    };
    let (pairs, told) = (Pairs::default(), Told::default());
    run_pairing(&pairs, &told, || {});
    assert_eq!(pairing_started(Ok(7), &pairs, &told), Some(7));
    assert!(!pairs.lost(NOW) && told.said().is_empty() && told.on_stderr().is_empty());
    run_pairing(&pairs, &told, || panic!("a pairing watch panics"));
    assert!(pairs.lost(NOW + 1000 * PAIR_WINDOW));
    both(&told);
    let (pairs, told) = (Pairs::default(), Told::default());
    let thread: io::Result<()> = Err(io::Error::from_raw_os_error(8));
    assert_eq!(pairing_started(thread, &pairs, &told), None);
    assert!(pairs.lost(NOW + 1000 * PAIR_WINDOW));
    both(&told);
}

/// The notice's words, as the lawyer reads them.
#[test]
fn the_pairing_lost_notice_says_what_happens_next_in_plain_words() {
    assert_eq!(
        PAIRING_LOST,
        "Clauseo can't follow folders the AI tool moves in this session, so it will stop the \
         session if the AI tool keeps moving or removing new folders in the matter before Clauseo \
         can check them."
    );
}
