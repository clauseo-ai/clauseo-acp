//! The watch's loop over fakes: what each kind of report does, kill first (the session ends
//! before anything moves), the overflow's rescans, what else ends the session (overflows in a
//! row, items gone again and again, a slow scan, a broken watch), an end that can't confirm the
//! job is empty, the fail-closed retries and the last rescan. What a check moves is
//! `set_aside/tests/live.rs`'s, over the set-aside's own volume.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{
    Cause, Changes, GONE_LIMIT, Guard, Layout, Live, Next, OVERFLOWS, Pulse, Session, Source, watch,
};
use crate::windows::SetAsideFailure;
use crate::windows::denial_pipe::Record;
use crate::windows::set_aside::{Ending, Entry, Failed, Hooks, Moved, Reason};
use crate::windows::test_world::matter_one;
use crate::{Denial, SetAsideWhat};

mod ends;
mod lost;
mod notices;
mod pairs;
mod records;
mod watchdog;

/// The paths of what failed.
fn paths(failed: &[Failed]) -> Vec<PathBuf> {
    failed.iter().map(|each| each.path.clone()).collect()
}

/// Names made or renamed, as the watch reports them.
fn added(paths: &[&str]) -> Next {
    Next::Changed(Changes {
        added: paths.iter().map(PathBuf::from).collect(),
        relinked: Vec::new(),
        moved_folders: Vec::new(),
        fresh_folders: Vec::new(),
        left: Vec::new(),
    })
}

/// Reports given in turn, then the stop.
struct Script(VecDeque<Next>);

impl Source for Script {
    fn next(&mut self) -> Next {
        self.0.pop_front().unwrap_or(Next::Stopped)
    }
    fn layout(&self) -> Layout {
        Layout::Extended
    }
}

fn script(nexts: Vec<Next>) -> Script {
    Script(nexts.into())
}

/// What the fakes saw, in order.
#[derive(Default)]
struct Log(RefCell<Vec<String>>);

impl Log {
    fn push(&self, line: impl Into<String>) {
        self.0.borrow_mut().push(line.into());
    }

    fn lines(&self) -> Vec<String> {
        self.0.borrow().clone()
    }
}

/// A guard that moves every reported path ending in `.git` (the decision itself is
/// `live.rs`'s), ending the session first as `live.rs` does; its moves fail for the paths in
/// `stuck` until the session ended, the paths in `gone` are gone each time, and with the end
/// unconfirmed each is left, as `live.rs` leaves it.
struct FakeGuard<'a> {
    log: &'a Log,
    stuck: Vec<PathBuf>,
    rescan_finds: Vec<PathBuf>,
    gone: Vec<PathBuf>,
    slow: bool,
}

fn entry(path: &Path) -> Entry {
    Entry {
        id: 1,
        matter: matter_one(),
        from: path.to_owned(),
        to: PathBuf::from("aside").join(path),
        folder: "20251105T101500-123Z".to_owned(),
        within: Vec::new(),
        name: ".git".into(),
        reason: Reason::New,
        owner: None,
        at: "2025-11-05T10:15:00.123Z".to_owned(),
    }
}

impl FakeGuard<'_> {
    fn moved(&self, paths: &[PathBuf], hooks: &impl Hooks) -> Live {
        let mut live = Live::default();
        if self.slow {
            hooks.end(Ending::Slow);
        }
        for path in paths.iter().filter(|p| p.ends_with(".git")) {
            if self.gone.contains(path) {
                live.gone.push(path.clone());
                continue;
            }
            if !hooks.end(Ending::Found) {
                live.left.push(path.clone());
                continue;
            }
            if self.stuck.contains(path) && !self.ended_twice() {
                let failure = SetAsideFailure::Io(io::ErrorKind::ResourceBusy);
                live.failed.push(Failed {
                    path: path.clone(),
                    failure,
                    what: SetAsideWhat::GitRepository,
                });
            } else {
                self.log.push(format!("moved {}", path.display()));
                live.moved.push(Moved {
                    entry: entry(path),
                    path: path.clone(),
                    what: SetAsideWhat::GitRepository,
                    by_agent: true,
                });
            }
        }
        live
    }

    /// Whether a retry is under way: the agent's handles are closed.
    fn ended_twice(&self) -> bool {
        self.log.lines().iter().any(|line| line == "retry")
    }
}

impl Guard for FakeGuard<'_> {
    fn changed(&self, changes: &Changes, hooks: &impl Hooks) -> Live {
        self.log.push("changed");
        self.moved(&changes.added, hooks)
    }

    fn rescan(&self, hooks: &impl Hooks) -> Live {
        self.log.push("rescan");
        self.moved(&self.rescan_finds, hooks)
    }

    fn retry(&self, failed: &[Failed], hooks: &impl Hooks) -> Live {
        self.log.push("retry");
        self.moved(&paths(failed), hooks)
    }
}

/// A session whose watch is stopped at its `stop_after`th pause, and whose end says the job is
/// `empty` or not.
struct FakeSession<'a> {
    log: &'a Log,
    stop_after: usize,
    pauses: RefCell<usize>,
    empty: bool,
    pulse: Pulse,
}

impl Session for FakeSession<'_> {
    fn end(&self) -> bool {
        self.log.push("end");
        self.empty
    }

    fn say(&self, line: &str) {
        assert!(!for_the_host(line), "{line}");
        self.log.push("said");
    }

    fn record(&self, record: &Record) {
        self.log.push(match record {
            Record::SetAside { .. } => "denial",
            Record::Notice { text } if text.contains("Clauseo stopped it") => "notice",
            // A stopped session's notice: the sequences count it with the lines said.
            Record::Notice { .. } => "said",
        });
    }

    fn pause(&self, _wait: Duration) -> bool {
        self.log.push("pause");
        *self.pauses.borrow_mut() += 1;
        *self.pauses.borrow() >= self.stop_after
    }

    fn settle(&self, _wait: Duration) {
        self.log.push("settle");
    }

    fn now(&self) -> Duration {
        Duration::ZERO
    }

    fn pulse(&self) -> &Pulse {
        &self.pulse
    }
}

/// Whether a stderr line looks like a record for the host: a notice, or the old set-aside line.
fn for_the_host(line: &str) -> bool {
    line.contains("Clauseo stopped") || line.contains("set aside\t")
}

fn session(log: &Log, stop_after: usize) -> FakeSession<'_> {
    FakeSession {
        log,
        stop_after,
        pauses: RefCell::new(0),
        empty: true,
        pulse: Pulse::default(),
    }
}

fn guard(log: &Log) -> FakeGuard<'_> {
    FakeGuard {
        log,
        stuck: Vec::new(),
        rescan_finds: Vec::new(),
        gone: Vec::new(),
        slow: false,
    }
}

/// Kill first: the first item found ends the session before it moves; then the lawyer's notice
/// and the host's line; at the stop, one last rescan.
#[test]
fn a_reported_git_ends_the_session_first_then_is_set_aside_and_the_lawyer_told() {
    let log = Log::default();
    let first = script(vec![added(&["sub/.git", "b/.git"])]);
    watch(first, &guard(&log), &session(&log, 1));
    assert_eq!(
        log.lines(),
        [
            "changed",
            "said",
            "end",
            "moved sub/.git",
            "moved b/.git",
            "notice",
            "denial",
            "notice",
            "denial",
            "rescan"
        ]
    );
}

#[test]
fn an_overflow_rescans_the_matter_and_reads_on() {
    let log = Log::default();
    let mut found = guard(&log);
    found.rescan_finds = vec![PathBuf::from("a/.git")];
    let first = script(vec![Next::Overflow, added(&[])]);
    watch(first, &found, &session(&log, 1));
    assert_eq!(
        log.lines()[..7],
        [
            "said",
            "rescan",
            "said",
            "end",
            "moved a/.git",
            "notice",
            "denial"
        ]
    );
    assert_eq!(log.lines()[7], "changed");
}

#[test]
fn overflows_in_a_row_end_the_session_and_a_change_between_resets_the_count() {
    assert_eq!(OVERFLOWS, 3);
    let log = Log::default();
    let first = script(vec![
        Next::Overflow,
        Next::Overflow,
        added(&[]),
        Next::Overflow,
        Next::Overflow,
    ]);
    watch(first, &guard(&log), &session(&log, 1));
    assert!(
        !log.lines().contains(&"end".to_owned()),
        "{:?}",
        log.lines()
    );

    let log = Log::default();
    let first = script(vec![Next::Overflow, Next::Overflow, Next::Overflow]);
    watch(first, &guard(&log), &session(&log, 1));
    let lines = log.lines();
    let end = lines.iter().position(|l| l == "end").unwrap();
    assert_eq!(lines.iter().filter(|l| *l == "rescan").count(), 4);
    assert_eq!(lines[..end].iter().filter(|l| *l == "rescan").count(), 2);
}

/// Renaming a tree back and forth: each report finds it gone. Past [`GONE_LIMIT`] the session
/// ends, instead of the watch chasing it.
#[test]
fn a_path_gone_again_and_again_ends_the_session() {
    assert_eq!(GONE_LIMIT, 3);
    let log = Log::default();
    let mut chased = guard(&log);
    chased.gone = vec![PathBuf::from(".git")];
    let first = script(vec![added(&[".git"]), added(&[".git"])]);
    watch(first, &chased, &session(&log, 1));
    assert!(!log.lines().contains(&"end".to_owned()));

    let log = Log::default();
    let mut chased = guard(&log);
    chased.gone = vec![PathBuf::from(".git")];
    let reports = vec![added(&[".git"]), added(&[".git"]), added(&[".git"])];
    watch(script(reports), &chased, &session(&log, 1));
    assert_eq!(
        log.lines(),
        ["changed", "changed", "changed", "said", "end", "rescan"]
    );
}

#[test]
fn a_scan_past_its_budget_ends_the_session() {
    let log = Log::default();
    let mut slow = guard(&log);
    slow.slow = true;
    watch(script(vec![Next::Overflow]), &slow, &session(&log, 1));
    assert_eq!(log.lines()[..4], ["said", "rescan", "said", "end"]);
}

/// M4: once the job is empty and the watch stopped, the last rescan still moves what it finds,
/// with no session left to end.
#[test]
fn the_last_rescan_at_the_stop_sets_aside_what_it_finds() {
    let log = Log::default();
    let mut found = guard(&log);
    found.rescan_finds = vec![PathBuf::from("late/.git")];
    watch(script(vec![]), &found, &session(&log, 1));
    assert_eq!(
        log.lines(),
        ["rescan", "moved late/.git", "notice", "denial"]
    );
}

/// Final review, item 3: when emptying the job failed, the last rescan moves nothing either; what
/// it finds is left for the check after the session, and said once.
#[test]
fn with_the_job_not_emptied_the_last_rescan_moves_nothing_and_leaves_it() {
    let log = Log::default();
    let mut found = guard(&log);
    found.rescan_finds = vec![PathBuf::from("late/.git")];
    let mut stuck = session(&log, 1);
    stuck.empty = false;
    watch(script(vec![added(&["a/.git"])]), &found, &stuck);
    assert_eq!(log.lines(), ["changed", "said", "end", "said", "rescan"]);
}

#[test]
fn a_git_that_cant_be_moved_is_tried_again_once_the_session_ended() {
    let log = Log::default();
    let mut stuck = guard(&log);
    stuck.stuck = vec![PathBuf::from(".git")];
    let first = script(vec![added(&[".git"])]);
    watch(first, &stuck, &session(&log, 1));
    assert_eq!(
        log.lines(),
        [
            "changed",
            "said",
            "end",
            "said",
            "retry",
            "moved .git",
            "notice",
            "denial",
            "rescan"
        ]
    );
}

/// A guard whose moves never go through, even once the session ended.
struct Stuck<'a>(FakeGuard<'a>);

impl Guard for Stuck<'_> {
    fn changed(&self, changes: &Changes, hooks: &impl Hooks) -> Live {
        let mut live = Live::default();
        hooks.end(Ending::Found);
        live.failed = changes
            .added
            .iter()
            .map(|path| Failed {
                path: path.clone(),
                failure: SetAsideFailure::Io(io::ErrorKind::ResourceBusy),
                what: SetAsideWhat::GitRepository,
            })
            .collect();
        live
    }

    fn rescan(&self, _hooks: &impl Hooks) -> Live {
        Live::default()
    }

    fn retry(&self, failed: &[Failed], _hooks: &impl Hooks) -> Live {
        self.0.log.push("retry");
        Live {
            failed: failed.to_vec(),
            ..Live::default()
        }
    }
}

#[test]
fn a_git_still_stuck_after_the_end_is_tried_and_said() {
    let log = Log::default();
    let first = script(vec![added(&[".git"])]);
    watch(first, &Stuck(guard(&log)), &session(&log, 1));
    let lines = log.lines();
    assert_eq!(lines[..3], ["said", "end", "said"]);
    assert_eq!(lines.iter().filter(|l| *l == "retry").count(), 5);
    assert_eq!(lines.last().unwrap(), "said");
    assert!(!lines.iter().any(|l| l == "notice"));
}

/// The lawyer's notice says where the item went (the agent's, here: no step to put it back);
/// the set-aside the host's sink takes goes with it. Both are records for the denial pipe:
/// nothing said on stderr, where the agent's own lines go too, reads as either.
#[test]
fn each_item_moved_is_recorded_for_the_host_and_never_said_on_stderr() {
    #[derive(Default)]
    struct Heard {
        said: RefCell<Vec<String>>,
        records: RefCell<Vec<Record>>,
        pulse: Pulse,
    }
    impl Session for Heard {
        fn end(&self) -> bool {
            true
        }
        fn say(&self, line: &str) {
            self.said.borrow_mut().push(line.to_owned());
        }
        fn record(&self, record: &Record) {
            self.records.borrow_mut().push(record.clone());
        }
        fn pause(&self, _wait: Duration) -> bool {
            true
        }
        fn settle(&self, _wait: Duration) {}
        fn now(&self) -> Duration {
            Duration::ZERO
        }
        fn pulse(&self) -> &Pulse {
            &self.pulse
        }
    }
    let log = Log::default();
    let heard = Heard::default();
    let git = PathBuf::from("Precedents").join(".git");
    watch(
        script(vec![added(&[git.to_str().unwrap()])]),
        &guard(&log),
        &heard,
    );

    let to = PathBuf::from("aside").join(&git);
    let notice = format!(
        "The AI tool tried to put a code repository in the matter. Clauseo stopped it and moved \
         {} to {}.",
        git.display(),
        to.display()
    );
    let records = heard.records.borrow();
    assert_eq!(
        *records,
        [
            Record::notice(Cause::Found.notice()),
            Record::notice(notice),
            Record::set_aside(&git, &to, SetAsideWhat::GitRepository),
        ]
    );
    assert_eq!(
        records[2].denial(),
        Some(Denial::SetAside {
            path: git,
            to,
            what: SetAsideWhat::GitRepository,
        })
    );
    for line in heard.said.borrow().iter() {
        assert!(!for_the_host(line), "{line}");
    }
}
