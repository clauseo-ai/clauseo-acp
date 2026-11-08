//! The git watch's decisions against a fake file system, a fake stream and a fake session: no
//! `FSEvents`, no thread except where [`start_with`]'s own is the point. `shapes.rs` has the
//! repository and link shapes, `neutral.rs` what happens to an item that can't move,
//! `state.rs` the matter's state file, `backstop.rs` the canary, the 30 s pass and the
//! host's quit, and `runner.rs` the kill's order, the watchdog, a stream that can't start, the
//! notes folder's watch and a session that outlives its kill.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::time::Duration;

use fake::{
    FakeFs, FakeSession, MOVED_IN, MemStore, assert_moved, event, found, none, quiet, watch,
};

use super::run::{Flow, Mode, Msg, Runner, start_with};
use super::{Fs as _, Phase, flags};
use crate::{Clock as _, Denial, GitOutcome};

mod backstop;
mod fake;
mod finds;
mod neutral;
mod runner;
mod shapes;
mod state;

#[test]
fn a_moved_in_folder_holding_a_git_is_set_aside_with_its_record() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/foo");
    fs.repo("/m/foo/.git");
    found(&mut watch, &[event("/m/foo", MOVED_IN)]);
    assert!(!fs.exists("/m/foo/.git"));
    assert!(fs.exists("/m/foo"));
    assert_moved(&notices, "foo/.git");
    let kept = fs.set_aside();
    assert!(kept.iter().any(|p| p.ends_with("item/hooks")), "{kept:?}");
    // The item's own HEAD is renamed: the set-aside item is not a working repository.
    assert!(!kept.iter().any(|p| p.ends_with("item/HEAD")), "{kept:?}");
    assert!(
        kept.iter().any(|p| p
            .to_string_lossy()
            .contains("item/HEAD-set-aside-by-clauseo-")),
        "{kept:?}"
    );
    let records = fs.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["original"], "/m/foo/.git");
    assert_eq!(records[0]["path"], "foo/.git");
    assert!(records[0]["outcome"]["moved"].is_string(), "{}", records[0]);
    assert_eq!(records[0]["session_ended"], true);
    assert_eq!(records[0]["time"], "2025-11-08T00:00:00.000Z");
    assert!(
        records[0]["session"]
            .as_str()
            .unwrap()
            .starts_with("clauseo-1-0-")
    );
}

#[test]
fn a_nested_git_in_a_moved_in_folder_is_set_aside() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/foo");
    fs.dir("/m/foo/bar");
    fs.repo("/m/foo/bar/.git");
    found(&mut watch, &[event("/m/foo", MOVED_IN)]);
    assert!(!fs.exists("/m/foo/bar/.git"));
    assert_moved(&notices, "foo/bar/.git");
}

#[test]
fn a_git_file_is_set_aside() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.file("/m/.git");
    let created = flags::ITEM_CREATED | 0x1_0000;
    found(&mut watch, &[event("/m/.git", created)]);
    assert!(!fs.exists("/m/.git"));
    assert_moved(&notices, ".git");
    assert_eq!(fs.records()[0]["kind"], "file");
}

#[test]
fn a_git_in_another_letter_case_is_set_aside() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/foo");
    fs.repo("/m/foo/.GIT");
    found(&mut watch, &[event("/m/foo", MOVED_IN)]);
    assert!(!fs.exists("/m/foo/.GIT"));
    assert_moved(&notices, "foo/.GIT");
    // And by its own event.
    fs.repo("/m/.Git");
    found(&mut watch, &[event("/m/.Git", MOVED_IN)]);
    assert_moved(&notices, ".Git");
}

#[test]
fn gitignore_and_github_are_never_set_aside() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.file("/m/.gitignore");
    fs.dir("/m/.github");
    fs.dir("/m/.github/workflows");
    fs.file("/m/.gitmodules");
    fs.file("/m/a.git");
    let events = [
        event("/m/.gitignore", flags::ITEM_CREATED),
        event("/m/.github", MOVED_IN),
        event("/m/a.git", flags::ITEM_CREATED),
    ];
    quiet(watch.events(&events));
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
    assert!(fs.exists("/m/.gitignore") && fs.exists("/m/.github/workflows"));
    assert_eq!(fs.set_aside(), Vec::<PathBuf>::new());
}

#[test]
fn the_launch_git_is_left_alone_unless_replaced() {
    let fs = FakeFs::new();
    fs.repo("/m/.git");
    fs.dir("/m/sub");
    fs.repo("/m/sub/.git");
    let (mut watch, notices) = watch(&fs, &["/m/.git", "/m/sub/.git"]);
    fs.file("/m/.git/index");
    let events = [
        event("/m/.git/index", flags::ITEM_CREATED),
        event("/m/.git", flags::ITEM_RENAMED),
    ];
    quiet(watch.events(&events));
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
    assert!(fs.exists("/m/.git/index") && fs.exists("/m/sub/.git/HEAD"));
    // The same path holding another item (another file ID) is not the launch's.
    fs.rename_new(Path::new("/m/sub/.git"), Path::new("/elsewhere"))
        .unwrap();
    fs.repo("/m/sub/.git");
    found(&mut watch, &[event("/m/sub/.git", MOVED_IN)]);
    assert_moved(&notices, "sub/.git");
}

#[test]
fn incomplete_events_and_paths_outside_the_matter_rescan_it() {
    for (path, flag) in [
        ("/m", flags::MUST_SCAN_SUB_DIRS),
        ("/m", flags::USER_DROPPED),
        ("/m", flags::KERNEL_DROPPED),
        ("/m", flags::ROOT_CHANGED),
        ("/private/m/x", flags::ITEM_CREATED),
    ] {
        let fs = FakeFs::new();
        let (watch, notices) = watch(&fs, &[]);
        let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO);
        let session = FakeSession {
            dies: true,
            ..FakeSession::default()
        };
        let _ = runner.input(Some(Msg::Attach(Arc::new(session.clone()))), Duration::ZERO);
        fs.dir("/m/deep");
        fs.dir("/m/deep/er");
        fs.repo("/m/deep/er/.git");
        let msg = Msg::Events(vec![event(path, flag)]);
        assert_eq!(
            runner.input(Some(msg), Duration::ZERO),
            Flow::Done,
            "{flag:#x}"
        );
        assert_eq!(session.ends.load(Ordering::SeqCst), 1, "{flag:#x}");
        assert!(fs.exists("/m/deep/er/.git"), "nothing moves before the end");
        assert_eq!(runner.finish().stuck().len(), 0);
        assert!(!fs.exists("/m/deep/er/.git"), "{flag:#x}");
        assert_moved(&notices, "deep/er/.git");
    }
}

#[test]
fn without_a_stream_the_launch_is_refused_and_nothing_runs() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let failing = |_: &[PathBuf], _: Sender<Msg>| -> Result<(), String> { Err("no".to_owned()) };
    let store = MemStore::default();
    store.0.lock().unwrap().sessions.push(state::record_of("s"));
    let error = start_with(
        watch,
        "s".to_owned(),
        store.clone(),
        None,
        Mode::Live,
        failing,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(
        error,
        "Clauseo won't start the AI tool: it can't watch /m for changes (no)."
    );
    assert!(
        store.0.lock().unwrap().sessions.is_empty(),
        "the session's record goes with it"
    );
    // No thread runs: a repository is never checked, nothing is ever touched or set aside.
    fs.repo("/m/.git");
    async_io::block_on(async_io::Timer::after(Duration::from_millis(300)));
    assert!(fs.exists("/m/.git"));
    assert_eq!(fs.touched(), Vec::<PathBuf>::new());
    none(&notices);
}

#[test]
fn a_runner_never_scans_on_a_timer() {
    // The old fallback scanned the matter every 2 s; with a stream nothing is listed until an
    // event, the canary or the 30 s pass asks.
    let fs = FakeFs::new();
    let (watch, _notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO);
    let session = FakeSession::default();
    let _ = runner.input(Some(Msg::Attach(Arc::new(session.clone()))), Duration::ZERO);
    fs.repo("/m/.git");
    let lists = fs.lists();
    for n in 1..=50 {
        assert_eq!(runner.input(None, Duration::from_millis(100 * n)), Flow::Go);
    }
    assert_eq!(fs.lists(), lists, "nothing was listed");
    assert_eq!(session.ends.load(Ordering::SeqCst), 0);
}

#[test]
fn a_find_ends_the_session_before_anything_moves() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO);
    let session = FakeSession::default();
    let t = Duration::from_millis(100);
    let _ = runner.input(Some(Msg::Attach(Arc::new(session.clone()))), t);
    fs.dir("/m/foo");
    fs.repo("/m/foo/.git");
    let msg = Msg::Events(vec![event("/m/foo", MOVED_IN)]);
    assert_eq!(runner.input(Some(msg), t * 2), Flow::Go);
    assert_eq!(session.ends.load(Ordering::SeqCst), 1, "killed at once");
    assert!(
        fs.exists("/m/foo/.git"),
        "nothing moves while a process is left"
    );
    none(&notices);
    session.gone.store(true, Ordering::SeqCst);
    assert_eq!(runner.input(None, t * 3), Flow::Done);
    assert_eq!(runner.finish().stuck().len(), 0);
    assert!(!fs.exists("/m/foo/.git"));
    let notices = notices.taken();
    let [
        Denial::GitSetAside {
            outcome: GitOutcome::Moved(_),
            session_ended: true,
            ..
        },
    ] = notices.as_slice()
    else {
        panic!("{notices:?}");
    };
}

#[test]
fn a_kill_is_sent_again_until_the_session_goes_and_capped() {
    let fs = FakeFs::new();
    let (watch, _notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO);
    fs.repo("/m/.git");
    // The find comes before the agent is attached: the kill goes once it is.
    let msg = Msg::Events(vec![event("/m/.git", MOVED_IN)]);
    assert_eq!(runner.input(Some(msg), Duration::ZERO), Flow::Go);
    let session = FakeSession::default();
    let attach = Msg::Attach(Arc::new(session.clone()));
    let _ = runner.input(Some(attach), Duration::from_millis(100));
    assert_eq!(session.ends.load(Ordering::SeqCst), 1);
    let _ = runner.input(None, Duration::from_millis(150));
    assert_eq!(session.ends.load(Ordering::SeqCst), 1);
    let _ = runner.input(None, Duration::from_millis(200));
    assert_eq!(session.ends.load(Ordering::SeqCst), 2);
    assert_eq!(runner.input(None, Duration::from_secs(10)), Flow::Done);
}

#[test]
fn the_final_scan_catches_what_the_events_missed() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO);
    let session = FakeSession::default();
    let _ = runner.input(Some(Msg::Attach(Arc::new(session.clone()))), Duration::ZERO);
    fs.dir("/m/a");
    fs.repo("/m/a/.git");
    assert_eq!(runner.input(None, Duration::from_millis(100)), Flow::Go);
    session.gone.store(true, Ordering::SeqCst);
    assert_eq!(runner.input(None, Duration::from_millis(200)), Flow::Done);
    assert!(fs.exists("/m/a/.git"));
    runner.finish().stuck();
    assert!(!fs.exists("/m/a/.git"));
    assert_moved(&notices, "a/.git");
}

#[test]
fn an_agent_that_did_not_start_ends_the_watch() {
    let fs = FakeFs::new();
    let (watch, _notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, Mode::Live, Duration::ZERO);
    assert_eq!(runner.input(Some(Msg::Abandon), Duration::ZERO), Flow::Done);
}

#[test]
fn a_watch_thread_that_panics_ends_the_session() {
    let fs = FakeFs::new();
    let (watch, _notices) = watch(&fs, &[]);
    let (tx, rx) = std::sync::mpsc::channel::<Sender<Msg>>();
    let source = move |_: &[PathBuf], events: Sender<Msg>| -> Result<(), String> {
        tx.send(events).unwrap();
        Ok(())
    };
    let watching = start_with(
        watch,
        "p".to_owned(),
        MemStore::default(),
        None,
        Mode::Live,
        source,
    )
    .unwrap();
    let session = FakeSession::default();
    watching.attach(session.clone());
    let events = rx.recv().unwrap();
    fs.dir("/m/boom");
    fs.panic_on("/m/boom");
    events
        .send(Msg::Events(vec![event("/m/boom", MOVED_IN)]))
        .unwrap();
    let clock = crate::SystemClock::new();
    while session.ends.load(Ordering::SeqCst) == 0 {
        assert!(
            clock.now() < Duration::from_secs(5),
            "the session was not ended"
        );
        async_io::block_on(async_io::Timer::after(Duration::from_millis(20)));
    }
}

#[test]
fn two_sessions_of_one_matter_both_end_and_set_aside_once() {
    let fs = FakeFs::new();
    let (mut first, first_notices) = watch(&fs, &[]);
    let (mut second, second_notices) = watch(&fs, &[]);
    fs.dir("/m/foo");
    fs.repo("/m/foo/.git");
    let events = [event("/m/foo", MOVED_IN)];
    assert!(first.events(&events).end);
    assert!(second.events(&events).end);
    quiet(first.full_scan(Phase::Ended));
    quiet(second.full_scan(Phase::Ended));
    assert_moved(&first_notices, "foo/.git");
    none(&second_notices);
}

#[test]
fn the_volume_set_aside_folder_in_the_matter_is_never_entered() {
    let fs = FakeFs::new();
    fs.dir("/m/.clauseo");
    let launch = super::Launch {
        skip: vec![PathBuf::from("/m/.clauseo")],
        ..super::Launch::default()
    };
    let (mut watch, notices) = fake::watch_launch(&fs, &launch);
    fs.dir("/m/.clauseo/git-set-aside");
    fs.dir("/m/.clauseo/git-set-aside/x");
    fs.dir("/m/.clauseo/git-set-aside/x/item");
    fs.bare("/m/.clauseo/git-set-aside/x/item");
    quiet(watch.events(&[event("/m/.clauseo/git-set-aside/x", MOVED_IN)]));
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
}
