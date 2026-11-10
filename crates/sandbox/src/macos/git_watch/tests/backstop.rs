//! The backstops behind the stream: the canary whose miss ends the run, a must-scan or
//! dropped flag that rescans and carries on, the 30 s pass over changed folders and its cap, a
//! kill that runs out moving nothing, and the host's quit.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::super::flags;
use super::super::host::Register;
use super::super::run::{
    BATCH_LATE, CANARY_EVERY, CANARY_WAIT, End, Flow, Mode, Msg, Runner, SWEEP_CAP, SWEEP_EVERY,
    canary_file, start_with,
};
use super::fake::{FakeFs, FakeSession, MemStore, Notices, SET_ASIDE, event, none, quiet, watch};
use crate::{Clock, Denial, SystemClock};

const CANARY: &str = "/data/sandbox/git-set-aside/watch-canary";

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A runner of `/m` with the canary, its session attached and dying when ended.
fn runner(fs: &FakeFs) -> (Runner<FakeFs>, FakeSession, Arc<Notices>) {
    let (watch, notices) = watch(fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO)
        .with_canary(PathBuf::from(CANARY), Duration::ZERO);
    let session = FakeSession {
        dies: true,
        ..FakeSession::default()
    };
    let attach = Msg::Attach(Arc::new(session.clone()));
    assert_eq!(runner.input(Some(attach), Duration::ZERO), Flow::Go);
    (runner, session, notices)
}

/// The canary's event, queued `at`.
fn echo(at: Duration) -> Msg {
    Msg::Events(vec![event(CANARY, 0x1000)], at)
}

fn ends(session: &FakeSession) -> u32 {
    session.ends.load(Ordering::SeqCst)
}

#[test]
fn a_canary_miss_ends_the_run_kill_first_then_the_final_scan() {
    let fs = FakeFs::new();
    let (mut runner, session, notices) = runner(&fs);
    assert_eq!(fs.touched(), [PathBuf::from(CANARY)], "written at once");
    // A repository arrives and the stream says nothing, the canary's event included.
    fs.repo("/m/.git");
    let before = CANARY_WAIT.checked_sub(ms(100)).unwrap();
    assert_eq!(runner.input(None, before), Flow::Go);
    assert!(!runner.lost());
    assert_eq!(ends(&session), 0);
    assert_eq!(runner.input(None, CANARY_WAIT), Flow::Done);
    assert!(runner.lost());
    assert_eq!(ends(&session), 1, "the session was killed");
    assert!(fs.exists("/m/.git"), "nothing moved before the kill");
    assert_eq!(notices.taken(), [Denial::GitWatchLost]);
    assert_eq!(runner.finish(&|| {}).stuck().len(), 0);
    assert!(!fs.exists("/m/.git"), "the final scan set it aside");
    let after = notices.taken();
    assert!(
        matches!(after.as_slice(), [Denial::GitSetAside { .. }]),
        "{after:?}"
    );
}

#[test]
fn the_canarys_event_keeps_the_stream_trusted() {
    let fs = FakeFs::new();
    let (mut runner, session, notices) = runner(&fs);
    assert_eq!(runner.input(Some(echo(ms(100))), ms(100)), Flow::Go);
    // Other events in the set-aside folder are not the matter's: no rescan.
    let other = Msg::Events(
        vec![event(&format!("{SET_ASIDE}/x/item"), 0x800)],
        Duration::ZERO,
    );
    fs.repo("/m/.git");
    assert_eq!(runner.input(Some(other), ms(200)), Flow::Go);
    assert_eq!(runner.input(None, CANARY_WAIT + ms(1_000)), Flow::Go);
    fs.remove("/m/.git");
    // Each write's event comes: the run goes on.
    assert_eq!(runner.input(None, CANARY_EVERY), Flow::Go);
    assert_eq!(fs.touched().len(), 2);
    assert_eq!(
        runner.input(Some(echo(CANARY_EVERY + ms(100))), CANARY_EVERY + ms(100)),
        Flow::Go
    );
    let later = CANARY_EVERY + CANARY_WAIT + ms(1_000);
    assert_eq!(runner.input(None, later), Flow::Go);
    assert!(!runner.lost());
    assert_eq!(ends(&session), 0);
    none(&notices);
}

#[test]
fn a_must_scan_or_dropped_flag_rescans_at_once_and_carries_on() {
    for flag in [
        flags::MUST_SCAN_SUB_DIRS,
        flags::USER_DROPPED,
        flags::KERNEL_DROPPED,
    ] {
        let fs = FakeFs::new();
        let (mut runner, session, _notices) = runner(&fs);
        let rescan = Msg::Events(vec![event("/m", flag)], Duration::ZERO);
        assert_eq!(runner.input(Some(rescan), ms(100)), Flow::Go, "{flag:#x}");
        assert_eq!(ends(&session), 0, "nothing found: the run goes on");
        // The next one finds what arrived without an event of its own.
        fs.dir("/m/deep");
        fs.repo("/m/deep/.git");
        let rescan = Msg::Events(vec![event("/m", flag)], Duration::ZERO);
        assert_eq!(runner.input(Some(rescan), ms(200)), Flow::Done, "{flag:#x}");
        assert_eq!(ends(&session), 1);
    }
}

#[test]
fn the_thirty_second_pass_finds_what_the_events_missed() {
    let fs = FakeFs::new();
    fs.dir("/m/a");
    fs.dir("/m/a/b");
    let (mut runner, session, _notices) = runner(&fs);
    // Each canary write's event comes; the matter's events never do.
    let mut at = Duration::ZERO;
    while at < SWEEP_EVERY {
        assert_eq!(runner.input(Some(echo(at)), at), Flow::Go);
        at += ms(500);
        if at == ms(1_000) {
            fs.dir("/m/a/b/new");
            fs.file("/m/a/b/new/HEAD");
        }
    }
    assert_eq!(ends(&session), 0, "nothing before the pass");
    assert_eq!(
        runner.input(Some(echo(SWEEP_EVERY)), SWEEP_EVERY),
        Flow::Done
    );
    assert_eq!(ends(&session), 1);
    runner.finish(&|| {}).stuck();
    assert!(!fs.exists("/m/a/b/new/HEAD"));
}

#[test]
fn the_pass_lists_only_changed_folders_and_goes_on_where_it_stopped() {
    let fs = FakeFs::new();
    for n in 0..600 {
        fs.dir(&format!("/m/d{n}"));
    }
    let (mut watch, _notices) = watch(&fs, &[]);
    // A HEAD added to a launch folder moves only that folder's stamp.
    fs.file("/m/d300/HEAD");
    let calls = AtomicU64::new(0);
    let reads = fs.dated_reads();
    let (asks, done) = watch.sweep(&|| calls.fetch_add(1, Ordering::SeqCst) >= 100);
    assert!(!done);
    assert_eq!(fs.dated_reads() - reads, 100, "stopped at its cap");
    let mut found = asks.end;
    let (asks, done) = watch.sweep(&|| false);
    assert!(done);
    assert_eq!(
        fs.dated_reads() - reads,
        601,
        "every folder read once in all"
    );
    found |= asks.end;
    assert!(found, "the changed folder was listed");
    // Nothing changed since: the next pass finds nothing.
    let (asks, done) = watch.sweep(&|| false);
    quiet(asks);
    assert!(done);
}

/// A clock that moves 1 ms each time it is read.
struct Ticking(AtomicU64);

impl Clock for Ticking {
    fn now(&self) -> Duration {
        ms(self.0.fetch_add(1, Ordering::SeqCst))
    }
}

#[test]
fn one_pass_takes_at_most_its_cap() {
    let fs = FakeFs::new();
    for n in 0..1_000 {
        fs.dir(&format!("/m/d{n}"));
    }
    let (watch, _notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO)
        .with_clock(Arc::new(Ticking(AtomicU64::new(0))));
    let reads = fs.dated_reads();
    assert_eq!(runner.input(None, SWEEP_EVERY), Flow::Go);
    let first = fs.dated_reads() - reads;
    let cap = u64::try_from(SWEEP_CAP.as_millis()).unwrap();
    assert!(first > 0 && first <= cap, "{first} folders in one pass");
    assert_eq!(runner.input(None, SWEEP_EVERY * 2), Flow::Go);
    let second = fs.dated_reads() - reads - first;
    assert!(
        second > 0 && second <= cap,
        "the next pass went on: {second}"
    );
}

#[test]
fn a_kill_that_runs_out_moves_nothing_and_lists_the_finds() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO);
    let session = FakeSession::default();
    let _ = runner.input(Some(Msg::Attach(Arc::new(session))), Duration::ZERO);
    fs.repo("/m/.git");
    let found = Msg::Events(vec![event("/m/.git", 0x100)], Duration::ZERO);
    assert_eq!(runner.input(Some(found), ms(100)), Flow::Go);
    assert!(!runner.lingering());
    assert_eq!(runner.input(None, Duration::from_secs(11)), Flow::Done);
    assert!(
        runner.lingering(),
        "its record stays for the next start or launch"
    );
    let End::Outlived(finds) = runner.finish(&|| {}) else {
        panic!("the final scan moved with a process left");
    };
    assert_eq!(finds, [PathBuf::from("/m/.git")]);
    assert!(fs.exists("/m/.git"), "nothing moved");
    assert_eq!(fs.set_aside(), Vec::<PathBuf>::new());
    none(&notices);
}

#[test]
fn the_stream_watches_the_canarys_folder_too() {
    let fs = FakeFs::new();
    let (watch, _notices) = watch(&fs, &[]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let roots = Arc::clone(&seen);
    let source = move |paths: &[PathBuf], _: Sender<Msg>| -> Result<(), String> {
        roots.lock().unwrap().extend_from_slice(paths);
        Ok(())
    };
    let watching = start_with(
        watch,
        "c".to_owned(),
        MemStore::default(),
        Some(PathBuf::from(CANARY)),
        None,
        Mode::Live,
        source,
    )
    .unwrap();
    drop(watching);
    let want = [PathBuf::from("/m"), PathBuf::from(SET_ASIDE)];
    assert_eq!(*seen.lock().unwrap(), want);
}

#[test]
fn the_hosts_quit_kills_every_session_and_waits_for_its_watch() {
    let register = Arc::new(Register::new());
    let kills = Arc::new(AtomicU64::new(0));
    let (on, counted) = (Arc::downgrade(&register), Arc::clone(&kills));
    // The session dies on its first kill, and its watch, done, leaves the register.
    let kill = move || {
        counted.fetch_add(1, Ordering::SeqCst);
        if let Some(register) = on.upgrade() {
            register.remove("a");
        }
    };
    register.add("a", Arc::new(kill)).unwrap();
    assert!(register.quit(Duration::from_secs(5), &SystemClock::new()));
    assert_eq!(kills.load(Ordering::SeqCst), 1);
    assert!(
        register.add("b", Arc::new(|| {})).is_err(),
        "no launch once quitting"
    );
}

#[test]
fn the_hosts_quit_gives_up_on_a_watch_after_its_time() {
    let register = Register::new();
    let kills = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&kills);
    let kill = move || {
        counted.fetch_add(1, Ordering::SeqCst);
    };
    register.add("a", Arc::new(kill)).unwrap();
    assert!(!register.quit(ms(200), &SystemClock::new()));
    assert!(kills.load(Ordering::SeqCst) >= 2, "the kill went again");
    assert!(register.has("a"));
}

#[test]
fn an_agent_that_writes_nonstop_cannot_hold_the_canary_back() {
    let fs = FakeFs::new();
    let (mut runner, session, notices) = runner(&fs);
    // The agent's events come every 100 ms, each read as soon as queued; the canary's never.
    let mut at = ms(100);
    while at < CANARY_WAIT {
        let busy = Msg::Events(vec![event("/m/notes.txt", 0x1000)], at);
        assert_eq!(runner.input(Some(busy), at), Flow::Go);
        at += ms(100);
    }
    let busy = Msg::Events(vec![event("/m/notes.txt", 0x1000)], at);
    assert_eq!(
        runner.input(Some(busy), at),
        Flow::Done,
        "judged on events too"
    );
    assert!(runner.lost());
    assert_eq!(ends(&session), 1);
    assert_eq!(notices.taken(), [Denial::GitWatchLost]);
}

#[test]
fn a_batch_read_late_ends_the_session_as_a_lost_stream() {
    let fs = FakeFs::new();
    let (mut runner, session, notices) = runner(&fs);
    assert_eq!(runner.input(Some(echo(ms(50))), ms(50)), Flow::Go);
    // Read within the wait of its queueing: the run goes on.
    let batch = Msg::Events(vec![event("/m/a.txt", 0x1000)], ms(100));
    assert_eq!(runner.input(Some(batch), ms(99) + BATCH_LATE), Flow::Go);
    assert!(!runner.lost());
    // Read the wait or more after it was queued: the watch lags behind the agent.
    let batch = Msg::Events(vec![event("/m/b.txt", 0x1000)], ms(200));
    assert_eq!(runner.input(Some(batch), ms(200) + BATCH_LATE), Flow::Done);
    assert!(runner.lost());
    assert_eq!(ends(&session), 1);
    assert_eq!(notices.taken(), [Denial::GitWatchLost]);
}

#[test]
fn a_canary_event_in_the_batch_being_read_counts_before_lateness_is_judged() {
    let fs = FakeFs::new();
    let (mut runner, session, notices) = runner(&fs);
    // Past the wait by the clock, but the canary's event is in this batch, queued in time.
    let late = CANARY_WAIT + ms(100);
    let batch = vec![event("/m/notes.txt", 0x1000), event(CANARY, 0x1000)];
    assert_eq!(
        runner.input(Some(Msg::Events(batch, ms(300))), late),
        Flow::Go
    );
    assert!(!runner.lost());
    assert_eq!(ends(&session), 0);
    none(&notices);
}

#[test]
fn a_canary_write_that_fails_for_a_moment_is_tried_again() {
    let fs = FakeFs::new();
    fs.fail_touches(2);
    let (mut runner, session, notices) = runner(&fs);
    assert_eq!(fs.touched().len(), 1, "the third try went");
    assert_eq!(runner.input(Some(echo(ms(100))), ms(100)), Flow::Go);
    assert_eq!(ends(&session), 0);
    none(&notices);
    // One that keeps failing loses the stream.
    fs.fail_touches(3);
    assert_eq!(runner.input(None, CANARY_EVERY), Flow::Done);
    assert!(runner.lost());
    assert_eq!(notices.taken(), [Denial::GitWatchLost]);
}

/// The canary's window is 5 s: its event 4.9 s late ends nothing, 5.1 s late ends the run.
#[test]
fn the_canary_waits_five_seconds() {
    assert_eq!(CANARY_WAIT, Duration::from_secs(5));
    let fs = FakeFs::new();
    let (mut runner, session, notices) = runner(&fs);
    // Written at 0, its event never comes.
    assert_eq!(runner.input(None, ms(4_900)), Flow::Go);
    assert!(!runner.lost());
    assert_eq!(ends(&session), 0);
    none(&notices);
    assert_eq!(runner.input(None, ms(5_100)), Flow::Done);
    assert!(runner.lost());
    assert_eq!(ends(&session), 1);
    assert_eq!(notices.taken(), [Denial::GitWatchLost]);
}

/// A batch's window is 5 s: read 4.9 s after the stream queued it ends nothing, 5.1 s after
/// ends the run.
#[test]
fn a_batch_may_wait_five_seconds_to_be_read() {
    assert_eq!(BATCH_LATE, Duration::from_secs(5));
    let fs = FakeFs::new();
    let (mut runner, session, notices) = runner(&fs);
    // The canary's event comes at once, so only the batches' own lag is judged.
    assert_eq!(runner.input(Some(echo(ms(50))), ms(50)), Flow::Go);
    let batch = Msg::Events(vec![event("/m/a.txt", 0x1000)], ms(100));
    assert_eq!(runner.input(Some(batch), ms(5_000)), Flow::Go, "4.9 s late");
    assert!(!runner.lost());
    assert_eq!(ends(&session), 0);
    none(&notices);
    let batch = Msg::Events(vec![event("/m/b.txt", 0x1000)], ms(200));
    assert_eq!(
        runner.input(Some(batch), ms(5_300)),
        Flow::Done,
        "5.1 s late"
    );
    assert!(runner.lost());
    assert_eq!(ends(&session), 1);
    assert_eq!(notices.taken(), [Denial::GitWatchLost]);
}

/// Each watch's canary has a random part of its own, so no other session's write answers it,
/// and it is removed when the watch stops.
#[test]
fn each_watch_has_a_canary_of_its_own_and_removes_it() {
    let fs = FakeFs::new();
    let folder = PathBuf::from(SET_ASIDE);
    let one = canary_file(&fs, &folder, "watch-canary");
    let two = canary_file(&fs, &folder, "watch-canary");
    assert_ne!(one, two);
    for canary in [&one, &two] {
        assert_eq!(canary.parent(), Some(folder.as_path()));
        let name = canary.file_name().unwrap().to_str().unwrap();
        let hex = name.strip_prefix("watch-canary-").unwrap();
        assert_eq!(hex.len(), 16, "{name}");
    }
    let (watch, _notices) = watch(&fs, &[]);
    let mut runner =
        Runner::new(watch, Mode::Live, Duration::ZERO).with_canary(one.clone(), Duration::ZERO);
    assert_eq!(runner.input(None, Duration::ZERO), Flow::Go);
    assert_eq!(fs.touched(), [one]);
    runner.remove_canary();
    assert_eq!(fs.touched(), Vec::<PathBuf>::new(), "removed");
}
