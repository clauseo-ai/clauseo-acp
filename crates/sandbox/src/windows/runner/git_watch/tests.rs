//! The watch's loop over fakes: what each kind of report does, kill first (the session ends
//! before anything moves), the overflow's and the arm failure's rescans, what else ends the
//! session (overflows in a row, a path gone again and again, a slow scan), the fail-closed
//! retries and the last rescan. What a check moves is `set_aside/tests/live.rs`'s, over the
//! set-aside's own volume.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::records::{Layout, changes};
use super::{
    Changes, GONE_LIMIT, Guard, Live, Next, OVERFLOWS, RESCAN_EVERY, Session, Source,
    denial_from_line, watch,
};
use crate::windows::SetAsideFailure;
use crate::windows::set_aside::{Ending, Entry, Failed, Hooks, Moved, Reason};
use crate::windows::test_world::matter_one;
use crate::{Denial, SetAsideWhat};

mod notices;

/// The paths of what failed.
fn paths(failed: &[Failed]) -> Vec<PathBuf> {
    failed.iter().map(|each| each.path.clone()).collect()
}

/// Names made or renamed, as the watch reports them.
fn added(paths: &[&str]) -> Next {
    Next::Changed(Changes {
        added: paths.iter().map(PathBuf::from).collect(),
        relinked: Vec::new(),
    })
}

/// Reports given in turn, then the stop.
struct Script(VecDeque<Next>);

impl Source for Script {
    fn next(&mut self) -> Next {
        self.0.pop_front().unwrap_or(Next::Stopped)
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
/// `stuck` until the session ended, and the paths in `gone` are gone each time.
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
            hooks.end(Ending::Found);
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

/// A session whose watch is stopped at its `stop_after`th pause.
struct FakeSession<'a> {
    log: &'a Log,
    stop_after: usize,
    pauses: RefCell<usize>,
}

impl Session for FakeSession<'_> {
    fn end(&self) {
        self.log.push("end");
    }

    fn say(&self, line: &str) {
        let line = if line.contains("Clauseo stopped it") {
            "notice"
        } else if denial_from_line(line).is_some() {
            "denial"
        } else {
            "said"
        };
        self.log.push(line);
    }

    fn pause(&self, wait: Duration) -> bool {
        assert_eq!(wait, RESCAN_EVERY);
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
}

fn session(log: &Log, stop_after: usize) -> FakeSession<'_> {
    FakeSession {
        log,
        stop_after,
        pauses: RefCell::new(0),
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

fn never_armed() -> io::Result<Script> {
    Err(io::Error::from_raw_os_error(5))
}

/// Kill first: the first item found ends the session before it moves; then the lawyer's notice
/// and the host's line; at the stop, one last rescan.
#[test]
fn a_reported_git_ends_the_session_first_then_is_set_aside_and_the_lawyer_told() {
    let log = Log::default();
    let first = Ok(script(vec![added(&["sub/.git", "b/.git"])]));
    watch(first, never_armed, &guard(&log), &session(&log, 1));
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
    let first = Ok(script(vec![Next::Overflow, added(&[])]));
    watch(first, never_armed, &found, &session(&log, 1));
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
    let first = Ok(script(vec![
        Next::Overflow,
        Next::Overflow,
        added(&[]),
        Next::Overflow,
        Next::Overflow,
    ]));
    watch(first, never_armed, &guard(&log), &session(&log, 1));
    assert!(
        !log.lines().contains(&"end".to_owned()),
        "{:?}",
        log.lines()
    );

    let log = Log::default();
    let first = Ok(script(vec![Next::Overflow, Next::Overflow, Next::Overflow]));
    watch(first, never_armed, &guard(&log), &session(&log, 1));
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
    let first = Ok(script(vec![added(&[".git"]), added(&[".git"])]));
    watch(first, never_armed, &chased, &session(&log, 1));
    assert!(!log.lines().contains(&"end".to_owned()));

    let log = Log::default();
    let mut chased = guard(&log);
    chased.gone = vec![PathBuf::from(".git")];
    let reports = vec![added(&[".git"]), added(&[".git"]), added(&[".git"])];
    watch(Ok(script(reports)), never_armed, &chased, &session(&log, 1));
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
    watch(
        Ok(script(vec![Next::Overflow])),
        never_armed,
        &slow,
        &session(&log, 1),
    );
    assert_eq!(log.lines()[..4], ["said", "rescan", "said", "end"]);
}

#[test]
fn a_watch_that_cant_be_armed_rescans_every_two_seconds_and_arms_again() {
    let log = Log::default();
    let arms = RefCell::new(0);
    let arm = || {
        *arms.borrow_mut() += 1;
        if *arms.borrow() < 3 {
            never_armed()
        } else {
            Ok(script(vec![]))
        }
    };
    watch(never_armed(), arm, &guard(&log), &session(&log, 9));
    assert_eq!(RESCAN_EVERY, Duration::from_secs(2));
    assert_eq!(
        log.lines(),
        // Degraded once: three rescans, two seconds apart, then armed again with a rescan for
        // the gap, until the stop, and the last rescan.
        [
            "said", "rescan", "pause", "rescan", "pause", "rescan", "pause", "said", "rescan",
            "rescan"
        ]
    );
}

#[test]
fn a_broken_watch_rescans_and_is_armed_again() {
    let log = Log::default();
    let first = Ok(script(vec![Next::Broken(io::Error::from_raw_os_error(6))]));
    watch(
        first,
        || Ok(script(vec![])),
        &guard(&log),
        &session(&log, 9),
    );
    assert_eq!(
        log.lines(),
        ["said", "rescan", "pause", "said", "rescan", "rescan"]
    );
}

/// M4: once the job is empty and the watch stopped, the last rescan still moves what it finds,
/// with no session left to end.
#[test]
fn the_last_rescan_at_the_stop_sets_aside_what_it_finds() {
    let log = Log::default();
    let mut found = guard(&log);
    found.rescan_finds = vec![PathBuf::from("late/.git")];
    watch(Ok(script(vec![])), never_armed, &found, &session(&log, 1));
    assert_eq!(
        log.lines(),
        ["rescan", "moved late/.git", "notice", "denial"]
    );
}

#[test]
fn a_git_that_cant_be_moved_is_tried_again_once_the_session_ended() {
    let log = Log::default();
    let mut stuck = guard(&log);
    stuck.stuck = vec![PathBuf::from(".git")];
    let first = Ok(script(vec![added(&[".git"])]));
    watch(first, never_armed, &stuck, &session(&log, 1));
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
    let first = Ok(script(vec![added(&[".git"])]));
    watch(first, never_armed, &Stuck(guard(&log)), &session(&log, 1));
    let lines = log.lines();
    assert_eq!(lines[..3], ["said", "end", "said"]);
    assert_eq!(lines.iter().filter(|l| *l == "retry").count(), 5);
    assert_eq!(lines.last().unwrap(), "said");
    assert!(!lines.iter().any(|l| l == "notice"));
}

/// One record: its action, name and attributes, padded to a `u32` boundary, in `layout`.
fn record(layout: Layout, (action, name, attributes): (u32, &str, u32), last: bool) -> Vec<u8> {
    let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let header = if layout == Layout::Plain { 12 } else { 84 };
    let size = (header + units.len()).next_multiple_of(4);
    let next = if last {
        0
    } else {
        u32::try_from(size).unwrap()
    };
    let mut bytes = vec![0; header];
    bytes[0..4].copy_from_slice(&next.to_le_bytes());
    bytes[4..8].copy_from_slice(&action.to_le_bytes());
    let length = u32::try_from(units.len()).unwrap().to_le_bytes();
    if layout == Layout::Plain {
        bytes[8..12].copy_from_slice(&length);
    } else {
        bytes[56..60].copy_from_slice(&attributes.to_le_bytes());
        bytes[80..84].copy_from_slice(&length);
    }
    bytes.extend(units);
    bytes.resize(size, 0);
    bytes
}

fn buffer(layout: Layout, records: &[(u32, &str, u32)]) -> Vec<u8> {
    let mut buffer = Vec::new();
    for (at, each) in records.iter().enumerate() {
        buffer.extend(record(layout, *each, at + 1 == records.len()));
    }
    buffer
}

/// H1: names made or renamed to are read, and of the changes, only an item that is a reparse
/// point now (`FILE_ATTRIBUTE_REPARSE_POINT`, 0x400): a junction set on an empty folder.
#[test]
fn names_made_and_items_turned_into_reparse_points_are_read_from_the_buffer() {
    let records = [
        (1, ".git", 0x10),      // FILE_ACTION_ADDED
        (2, "gone", 0x10),      // REMOVED
        (3, "edited", 0x20),    // MODIFIED, an archive bit
        (3, "junction", 0x410), // MODIFIED, a folder that is a junction now
        (4, "old-name", 0x10),  // RENAMED_OLD_NAME
        (5, "sub/.GIT", 0x10),  // RENAMED_NEW_NAME
        (1, "foo", 0x20),
    ];
    let read = changes(&buffer(Layout::Extended, &records), Layout::Extended).unwrap();
    assert_eq!(read.added, [".git", "sub/.GIT", "foo"].map(PathBuf::from));
    assert_eq!(read.relinked, [PathBuf::from("junction")]);

    // With no attributes to tell by, every change is checked.
    let read = changes(&buffer(Layout::Plain, &records), Layout::Plain).unwrap();
    assert_eq!(read.added, [".git", "sub/.GIT", "foo"].map(PathBuf::from));
    assert_eq!(read.relinked, ["edited", "junction"].map(PathBuf::from));
}

#[test]
fn a_buffer_that_doesnt_read_as_records_is_an_overflow() {
    for layout in [Layout::Plain, Layout::Extended] {
        let whole = record(layout, (1, ".git", 0), true);
        assert_eq!(changes(&whole[..10], layout), None);
        let mut long = whole.clone();
        let at = if layout == Layout::Plain { 8 } else { 80 };
        long[at] = 200;
        assert_eq!(changes(&long, layout), None);
        let mut loops = record(layout, (1, "a", 0), false);
        loops[0..4].copy_from_slice(&256_u32.to_le_bytes());
        assert_eq!(changes(&loops, layout), None);
    }
}

/// The lawyer's notice says where the item went and how to put back one the lawyer made; the
/// host's line reads back as the denial its sink takes, with the agent's plain words.
#[test]
fn each_item_moved_is_noticed_and_said_in_a_line_the_host_reads_back_as_a_denial() {
    struct Said(RefCell<Vec<String>>);
    impl Session for Said {
        fn end(&self) {}
        fn say(&self, line: &str) {
            self.0.borrow_mut().push(line.to_owned());
        }
        fn pause(&self, _wait: Duration) -> bool {
            true
        }
        fn settle(&self, _wait: Duration) {}
        fn now(&self) -> Duration {
            Duration::ZERO
        }
    }
    let log = Log::default();
    let said = Said(RefCell::new(Vec::new()));
    let git = PathBuf::from("Precedents").join(".git");
    let first = Ok(script(vec![added(&[git.to_str().unwrap()])]));
    watch(first, never_armed, &guard(&log), &said);

    let lines = said.0.borrow();
    let to = PathBuf::from("aside").join(&git);
    assert_eq!(
        lines[1],
        format!(
            "clauseo-sandbox-run: The AI tool tried to put a code repository in the matter. \
             Clauseo stopped it and moved {} to {}. If you made it yourself, move it back after \
             the session ends.",
            git.display(),
            to.display()
        )
    );
    assert_eq!(
        denial_from_line(&lines[2]),
        Some(Denial::SetAside {
            path: git,
            to,
            what: SetAsideWhat::GitRepository,
        })
    );
    assert!(
        SetAsideWhat::GitRepository
            .why()
            .starts_with("an agent can't create a git repository")
    );
    let link = format!(
        "clauseo-sandbox-run: set aside\tfoo\taside\\foo\t{}",
        SetAsideWhat::LinkOut.why()
    );
    assert_eq!(
        denial_from_line(&link),
        Some(Denial::SetAside {
            path: PathBuf::from("foo"),
            to: PathBuf::from("aside\\foo"),
            what: SetAsideWhat::LinkOut,
        })
    );
    for not in [
        "clauseo-sandbox-run: git watch: armed again",
        "clauseo-sandbox-run: set aside\tfoo\tbar",
        "clauseo-sandbox-run: set aside\tfoo\tbar\tno such words",
    ] {
        assert_eq!(denial_from_line(not), None, "{not}");
    }
}
