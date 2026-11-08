//! The git watch's decisions against a fake file system, a fake stream and a fake session: no
//! `FSEvents`, no thread except where [`start_with`]'s own is the point. `shapes.rs` has the
//! repository and link shapes.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::time::Duration;

use fake::{FakeFs, FakeSession, MOVED_IN, assert_moved, event, none, quiet, watch};

use super::run::{Flow, Msg, RESCAN_EVERY, Runner, start_with};
use super::{Fs as _, Phase, flags};
use crate::{Clock as _, Denial, GitItem};

mod fake;
mod shapes;

#[test]
fn a_moved_in_folder_holding_a_git_is_set_aside_with_its_record() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/foo");
    fs.repo("/m/foo/.git");
    let asks = watch.events(&[event("/m/foo", MOVED_IN)], Phase::Running);
    assert!(!asks.end && !asks.rescan);
    assert!(!fs.exists("/m/foo/.git"));
    assert!(fs.exists("/m/foo"));
    assert_moved(&notices, "foo/.git");
    let kept = fs.set_aside();
    assert!(kept.iter().any(|p| p.ends_with("item/HEAD")), "{kept:?}");
    assert!(kept.iter().any(|p| p.ends_with("item/hooks")), "{kept:?}");
    let records = fs.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["original"], "/m/foo/.git");
    assert_eq!(records[0]["path"], "foo/.git");
    assert_eq!(records[0]["how"], "moved");
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
    quiet(watch.events(&[event("/m/foo", MOVED_IN)], Phase::Running));
    assert!(!fs.exists("/m/foo/bar/.git"));
    assert_moved(&notices, "foo/bar/.git");
}

#[test]
fn a_git_file_is_set_aside() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.file("/m/.git");
    let created = flags::ITEM_CREATED | 0x1_0000;
    quiet(watch.events(&[event("/m/.git", created)], Phase::Running));
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
    quiet(watch.events(&[event("/m/foo", MOVED_IN)], Phase::Running));
    assert!(!fs.exists("/m/foo/.GIT"));
    assert_moved(&notices, "foo/.GIT");
    // And by its own event.
    fs.repo("/m/.Git");
    quiet(watch.events(&[event("/m/.Git", MOVED_IN)], Phase::Running));
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
    quiet(watch.events(&events, Phase::Running));
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
    quiet(watch.events(&events, Phase::Running));
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
    assert!(fs.exists("/m/.git/index") && fs.exists("/m/sub/.git/HEAD"));
    // The same path holding another item (another file ID) is not the launch's.
    fs.rename_new(Path::new("/m/sub/.git"), Path::new("/elsewhere"))
        .unwrap();
    fs.repo("/m/sub/.git");
    quiet(watch.events(&[event("/m/sub/.git", MOVED_IN)], Phase::Running));
    assert_moved(&notices, "sub/.git");
}

#[test]
fn incomplete_events_rescan_the_whole_matter() {
    for flag in [
        flags::MUST_SCAN_SUB_DIRS,
        flags::USER_DROPPED,
        flags::KERNEL_DROPPED,
        flags::ROOT_CHANGED,
    ] {
        let fs = FakeFs::new();
        let (watch, notices) = watch(&fs, &[]);
        let mut runner = Runner::new(watch, false, Duration::ZERO);
        fs.dir("/m/deep");
        fs.dir("/m/deep/er");
        fs.repo("/m/deep/er/.git");
        let msg = Msg::Events(vec![event("/m", flag)]);
        assert_eq!(runner.input(Some(msg), Duration::ZERO), Flow::Go);
        assert!(!fs.exists("/m/deep/er/.git"), "{flag:#x}");
        assert_moved(&notices, "deep/er/.git");
    }
}

#[test]
fn without_a_stream_the_matter_is_scanned_every_two_seconds() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, true, Duration::ZERO);
    assert_eq!(runner.input(None, Duration::ZERO), Flow::Go);
    fs.repo("/m/.git");
    let _ = runner.input(
        None,
        RESCAN_EVERY
            .checked_sub(Duration::from_millis(100))
            .unwrap(),
    );
    assert!(fs.exists("/m/.git"), "not before 2 s");
    let _ = runner.input(None, RESCAN_EVERY);
    assert!(!fs.exists("/m/.git"));
    assert_moved(&notices, ".git");
}

#[test]
fn a_stream_that_does_not_start_falls_back_to_the_rescan() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let failing = |_: &Path, _: Sender<Msg>| -> Result<(), String> { Err("no".to_owned()) };
    let watching = start_with(watch, failing).unwrap();
    let session = FakeSession::default();
    watching.attach(Box::new(session.clone()));
    let clock = crate::SystemClock::new();
    let gone_within = |path: &str, cap: Duration| {
        while fs.exists(path) {
            assert!(clock.now() < cap, "{path} not set aside within {cap:?}");
            async_io::block_on(async_io::Timer::after(Duration::from_millis(20)));
        }
        clock.now()
    };
    // The first scan runs at once, then one every 2 s.
    fs.repo("/m/.git");
    let first = gone_within("/m/.git", Duration::from_secs(3));
    assert_moved(&notices, ".git");
    fs.dir("/m/b");
    fs.repo("/m/b/.git");
    let second = gone_within("/m/b/.git", Duration::from_secs(8));
    let gap = second.saturating_sub(first);
    assert!(gap >= Duration::from_millis(1500), "{gap:?}");
    assert_moved(&notices, "b/.git");
    session.gone.store(true, Ordering::SeqCst);
}

#[test]
fn a_failed_move_ends_the_session_then_renames_in_place() {
    let fs = FakeFs::new();
    fs.fail_moves(18); // EXDEV
    let (watch, notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, false, Duration::ZERO);
    let session = FakeSession {
        dies: true,
        ..FakeSession::default()
    };
    let t = Duration::from_millis(100);
    assert_eq!(
        runner.input(Some(Msg::Attach(Box::new(session.clone()))), t),
        Flow::Go
    );
    fs.dir("/m/foo");
    fs.repo("/m/foo/.git");
    fs.file("/m/foo/.git-set-aside-by-clauseo-1");
    let msg = Msg::Events(vec![event("/m/foo", MOVED_IN)]);
    assert_eq!(runner.input(Some(msg), t * 2), Flow::Done);
    assert_eq!(
        session.ends.load(Ordering::SeqCst),
        1,
        "the session was ended"
    );
    assert!(
        fs.exists("/m/foo/.git"),
        "nothing renamed while the agent may run"
    );
    none(&notices);
    runner.finish();
    assert!(!fs.exists("/m/foo/.git"));
    assert!(fs.exists("/m/foo/.git-set-aside-by-clauseo-2/HEAD"));
    let notices = notices.taken();
    assert_eq!(
        notices,
        [Denial::GitSetAside {
            path: PathBuf::from("foo/.git"),
            what: GitItem::DotGit,
            moved_to: Some(PathBuf::from("/m/foo/.git-set-aside-by-clauseo-2")),
            session_ended: true,
        }]
    );
    let records = fs.records();
    assert_eq!(records[0]["how"], "renamed_in_place");
    assert_eq!(records[0]["session_ended"], true);
    assert!(
        records[0]["error"]
            .as_str()
            .unwrap()
            .contains("os error 18")
    );
}

#[test]
fn a_kill_is_sent_again_until_the_tree_goes_and_capped() {
    let fs = FakeFs::new();
    fs.fail_moves(1);
    let (watch, _notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, false, Duration::ZERO);
    fs.repo("/m/.git");
    // The failure comes before the agent is attached: the kill goes once it is.
    let msg = Msg::Events(vec![event("/m/.git", MOVED_IN)]);
    assert_eq!(runner.input(Some(msg), Duration::ZERO), Flow::Go);
    let session = FakeSession::default();
    let attach = Msg::Attach(Box::new(session.clone()));
    let _ = runner.input(Some(attach), Duration::from_millis(100));
    assert_eq!(session.ends.load(Ordering::SeqCst), 1);
    let _ = runner.input(None, Duration::from_millis(600));
    assert_eq!(session.ends.load(Ordering::SeqCst), 1);
    let _ = runner.input(None, Duration::from_millis(1200));
    assert_eq!(session.ends.load(Ordering::SeqCst), 2);
    assert_eq!(runner.input(None, Duration::from_secs(10)), Flow::Done);
}

#[test]
fn the_final_scan_catches_what_the_events_missed() {
    let fs = FakeFs::new();
    let (watch, notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, false, Duration::ZERO);
    let session = FakeSession::default();
    let _ = runner.input(Some(Msg::Attach(Box::new(session.clone()))), Duration::ZERO);
    fs.dir("/m/a");
    fs.repo("/m/a/.git");
    assert_eq!(runner.input(None, Duration::from_millis(100)), Flow::Go);
    session.gone.store(true, Ordering::SeqCst);
    assert_eq!(runner.input(None, Duration::from_millis(200)), Flow::Done);
    assert!(fs.exists("/m/a/.git"));
    runner.finish();
    assert!(!fs.exists("/m/a/.git"));
    assert_moved(&notices, "a/.git");
}

#[test]
fn an_agent_that_did_not_start_ends_the_watch() {
    let fs = FakeFs::new();
    let (watch, _notices) = watch(&fs, &[]);
    let mut runner = Runner::new(watch, false, Duration::ZERO);
    assert_eq!(runner.input(Some(Msg::Abandon), Duration::ZERO), Flow::Done);
}

#[test]
fn two_sessions_of_one_matter_set_aside_once_and_neither_ends() {
    let fs = FakeFs::new();
    let (mut first, first_notices) = watch(&fs, &[]);
    let (mut second, second_notices) = watch(&fs, &[]);
    fs.dir("/m/foo");
    fs.repo("/m/foo/.git");
    let events = [event("/m/foo", MOVED_IN)];
    assert!(!first.events(&events, Phase::Running).end);
    assert!(!second.events(&events, Phase::Running).end);
    assert_moved(&first_notices, "foo/.git");
    none(&second_notices);
}

#[test]
fn a_network_volume_in_the_matter_is_never_entered() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/share");
    fs.repo("/m/share/.git");
    fs.network("/m/share");
    quiet(watch.full_scan(Phase::Ended));
    quiet(watch.events(&[event("/m/share/.git", MOVED_IN)], Phase::Running));
    assert!(fs.exists("/m/share/.git"));
    none(&notices);
}
