//! Which tries hold the start and which run in the background (`skills::stetdoc::start`): a
//! stetdoc not seen to answer yet gets none at the start and 20 s, 20 s, then 30 s in the
//! background; one the marker names gets 3 s at the start and, when that times out or does not
//! start, 20 s then 30 s in the background. As in `stetdoc_start.rs`, no outcome hangs on the
//! wall clock: a slow stetdoc is the stub held by `stub-hang`, and [`Clock`] ends the waits the
//! test names at once and never ends the others. Its waits record their limits, and a counting
//! launcher counts the runs, so a test sees which limits a start used and how many runs it made.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use clauseo_process::{
    Error, LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions, Timer,
};
use clauseo_profile::skills::stetdoc::start::{
    self, FIRST_LIMIT, LONG_LIMIT, Later, Limits, RECHECK_LIMIT, Start, State, Tries, TryEnd,
    cached_skill, seen,
};
use futures::FutureExt as _;
use futures::executor::block_on;

use super::stetdoc_start::{skill_0_9_3, slow, stetdoc_in};
use super::{Scratch, profile_paths, write};

/// Limits that all differ, so [`Clock`] tells the tries apart: 3 s at the start for a stetdoc
/// seen before; in the background 19 s for one not seen yet, 20 s for the retry, 30 s last.
const APART: Limits = Limits {
    seen: FIRST_LIMIT,
    unseen: Duration::from_secs(19),
    retry: LONG_LIMIT,
    recheck: RECHECK_LIMIT,
};

/// A timer the test controls. A wait whose limit [`Self::fires`] names ends at once (that limit
/// has passed); every other wait never ends. The first wait for the retry's limit, which the
/// retry's deadline begins before it starts stetdoc, takes the stub's `stub-hang` away when
/// [`Self::release`] names it, so that try answers. Every wait's limit is recorded.
#[cfg(test)]
struct Clock {
    fires: fn(Duration) -> bool,
    release: Mutex<Option<(Duration, PathBuf)>>,
    waits: Mutex<Vec<Duration>>,
}

#[cfg(test)]
impl Clock {
    fn new(fires: fn(Duration) -> bool) -> Self {
        Self {
            fires,
            release: Mutex::new(None),
            waits: Mutex::new(Vec::new()),
        }
    }

    /// Takes `<root>/bin/stub-hang` away when the first wait for `limit` begins.
    fn releasing(self, limit: Duration, root: &Scratch) -> Self {
        *self.release.lock().unwrap() = Some((limit, root.join("bin").join("stub-hang")));
        self
    }

    fn waits(&self) -> Vec<Duration> {
        self.waits.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl Timer for Clock {
    fn after(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        self.waits.lock().unwrap().push(duration);
        let mut release = self.release.lock().unwrap();
        if release
            .as_ref()
            .is_some_and(|(limit, _)| *limit == duration)
            && let Some((_, hang)) = release.take()
        {
            write::remove_file(&hang).unwrap();
        }
        let fires = (self.fires)(duration);
        async move {
            if !fires {
                std::future::pending::<()>().await;
            }
        }
    }
}

/// The stub's launcher, counting the runs it starts or tries to.
#[cfg(test)]
struct Counted {
    inner: PassthroughLauncher,
    runs: AtomicUsize,
}

#[cfg(test)]
impl Counted {
    fn new() -> Self {
        Self {
            inner: PassthroughLauncher::new(SpawnOptions::new()),
            runs: AtomicUsize::new(0),
        }
    }

    fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
impl Launcher for Counted {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, Error> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        self.inner.launch(spec)
    }
}

/// The marker of the profile under `root`.
#[cfg(test)]
fn marker(root: &Scratch) -> PathBuf {
    profile_paths(root).stetdoc_seen()
}

/// [`start::start`] of the stub in `root` with `limits`, the marker of the profile under `root`.
#[cfg(test)]
fn start_in(
    root: &Scratch,
    launcher: &Counted,
    clock: &Clock,
    program: &Path,
    cached: &Path,
    limits: Limits,
) -> Start {
    let seen = marker(root);
    block_on(start::start(
        launcher, clock, program, cached, &seen, limits,
    ))
}

/// [`start::recheck`] of the stub in `root` from `now`, within `later`'s limits.
#[cfg(test)]
fn later_in(
    root: &Scratch,
    launcher: &Counted,
    clock: &Clock,
    program: &Path,
    cached: &Path,
    now: &State,
    later: &[Duration],
) -> Option<State> {
    let seen = marker(root);
    let later = Later {
        limits: later,
        held: Tries::new().begin(),
    };
    block_on(start::recheck(
        launcher, clock, program, cached, &seen, now, later,
    ))
    .state
}

/// The background tries' limits for a stetdoc not seen yet.
#[cfg(test)]
fn unseen_tries(limits: Limits) -> Vec<Duration> {
    vec![limits.unseen, limits.retry, limits.recheck]
}

/// The background tries' limits for a stetdoc seen before whose try at the start timed out or
/// did not start.
#[cfg(test)]
fn seen_tries(limits: Limits) -> Vec<Duration> {
    vec![limits.retry, limits.recheck]
}

/// A stetdoc not seen yet holds nothing at the start: no run, no wait, on at once with no skill
/// (or the cached one), the background's three tries to follow. Once it answered there, the
/// marker names it and the next start tries it within 3 s. A marker for another build of it
/// (another size) does not count.
#[test]
fn an_unseen_stetdoc_is_not_run_at_the_start_and_once_it_answered_gets_3_s() {
    let root = Scratch::new("first-unseen-then-seen");
    let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
    assert!(!seen::is_seen(&marker(&root), &program));
    let (launcher, clock) = (Counted::new(), Clock::new(|_| false));
    let started = start_in(&root, &launcher, &clock, &program, &cached, Limits::APP);
    assert_eq!(
        started,
        Start {
            state: State::On(cached_skill(&cached)),
            later: unseen_tries(Limits::APP)
        }
    );
    assert_eq!((launcher.runs(), clock.waits()), (0, Vec::new()));

    let next = later_in(
        &root,
        &launcher,
        &clock,
        &program,
        &cached,
        &started.state,
        &started.later,
    );
    assert_eq!(next, Some(State::On(Some(skill_0_9_3()))));
    assert!(seen::is_seen(&marker(&root), &program));

    let clock = Clock::new(|_| false);
    let started = start_in(
        &root,
        &Counted::new(),
        &clock,
        &program,
        &cached,
        Limits::APP,
    );
    assert_eq!(started.state, State::On(Some(skill_0_9_3())));
    assert_eq!(started.later, Vec::<Duration>::new());
    assert_eq!(clock.waits().first(), Some(&FIRST_LIMIT));

    let held = std::fs::read_to_string(marker(&root)).unwrap();
    let size = std::fs::metadata(&program).unwrap().len();
    let other = held.replacen(&format!("\n{size}\n"), &format!("\n{}\n", size + 1), 1);
    assert_ne!(other, held);
    write::write(&marker(&root), other.as_bytes()).unwrap();
    assert!(!seen::is_seen(&marker(&root), &program));
    let launcher = Counted::new();
    let started = start_in(
        &root,
        &launcher,
        &Clock::new(|_| false),
        &program,
        &cached,
        Limits::APP,
    );
    assert_eq!(started.later, unseen_tries(Limits::APP));
    assert_eq!(launcher.runs(), 0);
}

/// Not seen yet and slower than its first background limit once (as while the OS scans a new
/// program), then fast: the start is not held (no run), and the background's second try
/// answers, so the agents started from then on get the skill, and the marker names it.
#[test]
fn an_unseen_stetdoc_slow_once_holds_nothing_and_gets_its_skill_in_the_background() {
    let root = Scratch::new("first-unseen-slow-once");
    let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
    slow(&root);
    let clock = Clock::new(|limit| limit == APART.unseen).releasing(APART.retry, &root);
    let launcher = Counted::new();
    let started = start_in(&root, &launcher, &clock, &program, &cached, APART);
    assert_eq!(started.state, State::On(None));
    assert_eq!(
        (launcher.runs(), clock.waits()),
        (0, Vec::new()),
        "the start holds nothing"
    );

    let next = later_in(
        &root,
        &launcher,
        &clock,
        &program,
        &cached,
        &started.state,
        &started.later,
    );
    assert_eq!(next, Some(State::On(Some(skill_0_9_3()))));
    // The held run, then the second try's `--version` and `skills get core`.
    assert_eq!(launcher.runs(), 3);
    assert_eq!(clock.waits().first(), Some(&APART.unseen));
    assert!(!clock.waits().contains(&APART.recheck), "no third try");
    assert!(seen::is_seen(&marker(&root), &program));
}

/// Seen before, but past its 3 s at the start (Defender scanning it again after a signature
/// update): the start waits for that one try alone, every wait there 3 s; stetdoc stays on at
/// once, and the background's 20 s retry answers and turns the skill on.
#[test]
fn a_seen_stetdoc_that_times_out_holds_the_start_3_s_and_the_retry_turns_it_on() {
    let root = Scratch::new("first-seen-timeout");
    let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
    seen::record(&marker(&root), &program);
    slow(&root);
    let clock = Clock::new(|limit| limit == FIRST_LIMIT).releasing(APART.retry, &root);
    let launcher = Counted::new();
    let started = start_in(&root, &launcher, &clock, &program, &cached, APART);
    assert_eq!(
        started,
        Start {
            state: State::On(None),
            later: seen_tries(APART)
        }
    );
    assert_eq!(launcher.runs(), 1);
    let at_start = clock.waits();
    assert_eq!(at_start.first(), Some(&FIRST_LIMIT));
    assert!(
        at_start.iter().all(|wait| *wait == FIRST_LIMIT),
        "{at_start:?}"
    );

    let next = later_in(
        &root,
        &launcher,
        &clock,
        &program,
        &cached,
        &started.state,
        &started.later,
    );
    assert_eq!(next, Some(State::On(Some(skill_0_9_3()))));
    assert_eq!(launcher.runs(), 3);
    assert!(seen::is_seen(&marker(&root), &program));
}

/// A stetdoc that hangs on every run ends on without its skill, each try killed at its limit:
/// not seen yet, no run at the start and three in the background (20 s, 20 s, 30 s); seen
/// before, one at the start (3 s) and two in the background (20 s, 30 s). The marker is
/// unchanged.
#[test]
fn a_stetdoc_that_always_hangs_ends_without_its_skill_after_three_runs() {
    for seen_before in [false, true] {
        let root = Scratch::new(&format!("first-hangs-{seen_before}"));
        let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
        if seen_before {
            seen::record(&marker(&root), &program);
        }
        let held = std::fs::read(marker(&root)).ok();
        slow(&root);
        let clock = Clock::new(|_| true);
        let launcher = Counted::new();
        let started = start_in(&root, &launcher, &clock, &program, &cached, Limits::APP);
        assert_eq!(started.state, State::On(None), "seen before: {seen_before}");
        assert_eq!(launcher.runs(), usize::from(seen_before));
        let next = later_in(
            &root,
            &launcher,
            &clock,
            &program,
            &cached,
            &started.state,
            &started.later,
        );
        assert_eq!(next, None, "nothing changes: on, without the skill");
        assert_eq!(launcher.runs(), 3, "seen before: {seen_before}");
        let waits = clock.waits();
        let limits = if seen_before {
            vec![FIRST_LIMIT, LONG_LIMIT, RECHECK_LIMIT]
        } else {
            vec![LONG_LIMIT, RECHECK_LIMIT]
        };
        assert!(waits.iter().all(|wait| limits.contains(wait)), "{waits:?}");
        assert!(waits.contains(&RECHECK_LIMIT));
        assert_eq!(
            std::fs::read(marker(&root)).ok(),
            held,
            "the marker is unchanged"
        );
    }
}

/// A `--version` that fails for real: seen before, absent at the start with no background try;
/// not seen yet, absent after the background's first try, with no other.
#[test]
fn a_real_failure_gets_no_other_try() {
    let root = Scratch::new("first-broken");
    let (program, cached) = stetdoc_in(&root, "\n");
    let launcher = Counted::new();
    let clock = Clock::new(|_| false);
    let started = start_in(&root, &launcher, &clock, &program, &cached, APART);
    assert_eq!(launcher.runs(), 0);
    let next = later_in(
        &root,
        &launcher,
        &clock,
        &program,
        &cached,
        &started.state,
        &started.later,
    );
    assert_eq!(next, Some(State::Off));
    assert_eq!(launcher.runs(), 1);
    assert!(!marker(&root).exists());

    seen::record(&marker(&root), &program);
    let launcher = Counted::new();
    let started = start_in(&root, &launcher, &clock, &program, &cached, APART);
    assert_eq!(
        started,
        Start {
            state: State::Off,
            later: Vec::new()
        }
    );
    assert_eq!(launcher.runs(), 1);
}

/// A stetdoc not seen yet that cannot be started (a file with no execute bit) is tried in the
/// background three times, then absent.
#[cfg(unix)]
#[test]
fn an_unseen_stetdoc_that_cannot_start_is_tried_three_times_then_absent() {
    let root = Scratch::new("first-not-a-program");
    let program = root.join("bin").join("stetdoc");
    write::write(&program, b"not a program\n").unwrap();
    let cached = profile_paths(&root).claude_skills();
    let launcher = Counted::new();
    let clock = Clock::new(|_| false);
    let started = start_in(&root, &launcher, &clock, &program, &cached, APART);
    assert_eq!(started.state, State::On(None));
    let next = later_in(
        &root,
        &launcher,
        &clock,
        &program,
        &cached,
        &started.state,
        &started.later,
    );
    assert_eq!(next, Some(State::Off));
    assert_eq!(launcher.runs(), 3);
    assert_eq!(clock.waits(), unseen_tries(APART));
}

/// Whether `end` has ended, polled once.
#[cfg(test)]
fn ended(end: &mut TryEnd) -> bool {
    let waker = std::task::Waker::noop();
    end.poll_unpin(&mut std::task::Context::from_waker(waker))
        .is_ready()
}

/// The background tries are in flight for a launch to wait on: the first from before the
/// thread runs it, the next from the moment the first moves on, and the last until the caller,
/// having put the answer in place, drops it. A launch's wait on a try that moved on ends then.
#[test]
fn each_background_try_is_in_flight_until_it_moves_on_or_its_answer_is_in_place() {
    let root = Scratch::new("first-in-flight");
    let (program, cached) = stetdoc_in(&root, "stetdoc 0.9.3\n");
    slow(&root);
    let clock = Clock::new(|limit| limit == APART.unseen).releasing(APART.retry, &root);
    let tries = Tries::new();
    let later = Later {
        limits: &unseen_tries(APART),
        held: tries.begin(),
    };
    let mut first = tries.wait().expect("in flight before the thread runs it");
    assert!(!ended(&mut first));
    let seen = marker(&root);
    let answered = block_on(start::recheck(
        &Counted::new(),
        &clock,
        &program,
        &cached,
        &seen,
        &State::On(None),
        later,
    ));
    assert_eq!(answered.state, Some(State::On(Some(skill_0_9_3()))));
    assert!(ended(&mut first), "the first try timed out and moved on");
    let mut last = tries
        .wait()
        .expect("the answering try is in flight until dropped");
    assert!(!ended(&mut last));
    drop(answered.held);
    assert!(ended(&mut last));
    assert!(
        tries.wait().is_none(),
        "no try in flight: a launch does not wait"
    );
}
