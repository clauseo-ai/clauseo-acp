//! The chooser opens at once (Windows, a Documents folder on a share that is down): the
//! pre-check runs as the app starts and its answer is kept for the run, and the drives and the
//! first listing land after the session has opened, each under its own bound. Every probe of
//! the fake machine here takes 2 s.

use super::*;

/// Waits `wait` of real time, as the fake machine's reads take it: the bounds under test are
/// real `recv_timeout`s, as in the rest of the chooser's tests.
fn sleep(wait: Duration) {
    let (_held, never) = mpsc::channel::<()>();
    let _timed_out = never.recv_timeout(wait);
}

/// A little under `wait`, for a timing that lands about then.
fn about(wait: Duration) -> Duration {
    wait.saturating_sub(Duration::from_millis(100))
}

/// How long each fake probe takes.
const SLOW: Duration = Duration::from_secs(2);

/// Known folders: Documents on `\\192.0.2.1`, whose probe takes [`SLOW`] and then answers as
/// `answers` says. Counts the reads of the paths and the probes.
struct Known {
    answers: bool,
    reads: AtomicUsize,
    probes: AtomicUsize,
}

impl KnownFolders for Known {
    fn paths(&self) -> Vec<String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        vec![r"\\192.0.2.1\docs".to_owned(), HOME.to_owned()]
    }
    fn remote_drive(&self, _letter: char) -> bool {
        false
    }
    fn answers(&self, _path: &str) -> bool {
        self.probes.fetch_add(1, Ordering::SeqCst);
        if !self.answers {
            sleep(SLOW);
        }
        self.answers
    }
}

/// A machine whose drives and folders each take [`SLOW`] to read: drive C, home with `Work`.
struct Slow;

impl Machine for Slow {
    fn drives(&self) -> Drives {
        sleep(SLOW);
        Drives {
            local: vec!['C'],
            desktop: Some(format!(r"{HOME}\Desktop")),
            ..Drives::default()
        }
    }
    fn read(&self, _folder: &str) -> std::io::Result<Vec<Entry>> {
        sleep(SLOW);
        Ok(vec![dir("Work")])
    }
    fn entry(&self, _path: &str) -> Option<Entry> {
        Some(dir(""))
    }
}

fn slow_chooser(answers: bool) -> (Chooser, Arc<Known>) {
    let known = Arc::new(Known {
        answers,
        reads: AtomicUsize::new(0),
        probes: AtomicUsize::new(0),
    });
    let folders: Arc<dyn KnownFolders> = Arc::clone(&known) as Arc<dyn KnownFolders>;
    (Chooser::with(Arc::new(Slow), Some(folders)), known)
}

/// What `pick_folder_or_choose` does on Windows with no setting: the route, then a session.
fn click(chooser: &Chooser) -> Option<ChooserStart> {
    let now = crate::open::now();
    let route = route(false, true, None, || chooser.would_stall());
    (route == Route::Chooser)
        .then(|| chooser.open(Some(Path::new(HOME)), None, &[], now))
        .flatten()
}

/// The pre-check started with the app has its answer by the click: the session opens in well
/// under a second, before any probe ends; the start points land when the drives' bound ends
/// (1.5 s, before the 2 s read: no drive, home still), and the listing after its own read.
#[test]
fn the_chooser_opens_before_any_probe_ends_and_each_answer_lands_later() {
    let (chooser, _known) = slow_chooser(false);
    let app_start = crate::open::now();
    chooser.warm();
    // The lawyer reads the first screen; the pre-check's bound ends meanwhile.
    sleep(PROBE_WAIT + Duration::from_millis(100));

    let clicked = crate::open::now();
    let start = click(&chooser).expect("the chooser opens");
    let opened = clicked.elapsed();
    assert!(opened < Duration::from_millis(200), "{opened:?}");
    // No probe has ended: the pre-check's own 2 s probe is still running too.
    assert!(app_start.elapsed() < SLOW, "{:?}", app_start.elapsed());

    let starts = chooser.starts(start.chooser, crate::open::now()).unwrap();
    let landed = clicked.elapsed();
    assert!(landed >= about(DRIVES_WAIT) && landed < SLOW, "{landed:?}");
    let kinds: Vec<StartKind> = starts.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [StartKind::Home]);

    let listed = crate::open::now();
    let home = chooser
        .list(start.chooser, Go::Start { key: 0 }, crate::open::now())
        .unwrap();
    let took = listed.elapsed();
    assert!(took >= about(SLOW), "{took:?}");
    assert!(
        matches!(home.state, ListState::Folders { .. } | ListState::NoAnswer),
        "{:?}",
        home.state
    );
}

/// A click before the pre-check has its answer waits only for the rest of its bound, counted
/// from the app's start, not a bound of its own.
#[test]
fn a_click_before_the_answer_waits_only_for_the_rest_of_the_bound() {
    let (chooser, known) = slow_chooser(false);
    let app_start = crate::open::now();
    chooser.warm();
    sleep(Duration::from_millis(500));
    let clicked = crate::open::now();
    assert!(chooser.would_stall());
    let waited = clicked.elapsed();
    let total = app_start.elapsed();
    assert!(waited < Duration::from_millis(1300), "{waited:?}");
    assert!(total < PROBE_WAIT + HAND_IN, "{total:?}");
    assert_eq!(known.probes.load(Ordering::SeqCst), 1);
}

/// "Stalls" is kept for the run: a second click reads no known folder and probes nothing. An
/// "answers" is checked again on the next click, which takes no time while the server answers.
#[test]
fn the_pre_check_answer_is_used_again_on_the_next_click() {
    let (dead, known) = slow_chooser(false);
    assert!(dead.would_stall());
    let again = crate::open::now();
    assert!(dead.would_stall());
    assert!(again.elapsed() < Duration::from_millis(50));
    assert_eq!(known.reads.load(Ordering::SeqCst), 1);
    assert_eq!(known.probes.load(Ordering::SeqCst), 1);

    let (live, known) = slow_chooser(true);
    live.warm();
    assert!(!live.would_stall());
    let reads = known.reads.load(Ordering::SeqCst);
    let again = crate::open::now();
    assert!(!live.would_stall());
    assert!(again.elapsed() < Duration::from_millis(200));
    assert_eq!(known.reads.load(Ordering::SeqCst), reads + 1);
}

/// The app starts the pre-check only where a pick would ask it: on Windows, with no setting and
/// no fixed e2e folder.
#[test]
fn the_app_starts_the_pre_check_only_where_a_pick_asks_it() {
    assert!(asks_precheck(false, true, None));
    assert!(asks_precheck(false, true, Some("other")));
    assert!(!asks_precheck(false, true, Some("native")));
    assert!(!asks_precheck(false, true, Some("app")));
    assert!(!asks_precheck(false, false, None));
    assert!(!asks_precheck(true, true, None));
}
