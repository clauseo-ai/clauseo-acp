//! The watch's loop over fakes: what each kind of report does, the overflow's and the arm
//! failure's rescans, and the fail-closed order (end the session, then set aside). What a
//! check moves is `set_aside/tests/live.rs`'s, over the set-aside's own volume.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{
    Guard, Live, Next, RESCAN_EVERY, Session, Source, added_names, denial_from_line, watch,
};
use crate::windows::SetAsideFailure;
use crate::windows::set_aside::{Entry, Failed, Moved, Reason};
use crate::windows::test_world::matter_one;
use crate::{Denial, SetAsideWhat};

/// The paths of what failed.
fn paths(failed: &[Failed]) -> Vec<PathBuf> {
    failed.iter().map(|each| each.path.clone()).collect()
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
/// `live.rs`'s), and whose moves fail for the paths in `stuck` until the session ended.
struct FakeGuard<'a> {
    log: &'a Log,
    stuck: Vec<PathBuf>,
    rescan_finds: Vec<PathBuf>,
}

fn entry(path: &PathBuf) -> Entry {
    Entry {
        id: 1,
        matter: matter_one(),
        from: path.clone(),
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
    fn moved(&self, paths: &[PathBuf], ended: bool) -> Live {
        let mut live = Live::default();
        for path in paths.iter().filter(|p| p.ends_with(".git")) {
            if self.stuck.contains(path) && !ended {
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

    fn ended(&self) -> bool {
        self.log.lines().iter().any(|line| line == "end")
    }
}

impl Guard for FakeGuard<'_> {
    fn changed(&self, paths: &[PathBuf]) -> Live {
        self.log.push("changed");
        self.moved(paths, self.ended())
    }

    fn rescan(&self) -> Live {
        self.log.push("rescan");
        self.moved(&self.rescan_finds, self.ended())
    }

    fn retry(&self, failed: &[Failed]) -> Live {
        self.log.push("retry");
        self.moved(&paths(failed), self.ended())
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
        let line = if line.contains("Clauseo set aside") {
            "notice"
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
    }
}

fn never_armed() -> io::Result<Script> {
    Err(io::Error::from_raw_os_error(5))
}

#[test]
fn a_reported_git_is_set_aside_at_once_and_the_lawyer_told() {
    let log = Log::default();
    let first = Ok(script(vec![Next::Changed(vec![
        PathBuf::from("sub").join(".git"),
    ])]));
    watch(first, never_armed, &guard(&log), &session(&log, 1));
    assert_eq!(log.lines(), ["changed", "moved sub/.git", "notice"]);
}

#[test]
fn an_overflow_rescans_the_matter_and_reads_on() {
    let log = Log::default();
    let mut found = guard(&log);
    found.rescan_finds = vec![PathBuf::from("a/.git")];
    let first = Ok(script(vec![Next::Overflow, Next::Changed(Vec::new())]));
    watch(first, never_armed, &found, &session(&log, 1));
    assert_eq!(
        log.lines(),
        ["said", "rescan", "moved a/.git", "notice", "changed"]
    );
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
        // the gap, until the stop.
        [
            "said", "rescan", "pause", "rescan", "pause", "rescan", "pause", "said", "rescan"
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
    assert_eq!(log.lines(), ["said", "rescan", "pause", "said", "rescan"]);
}

#[test]
fn a_git_that_cant_be_moved_ends_the_session_and_then_is_set_aside() {
    let log = Log::default();
    let git = PathBuf::from(".git");
    let mut stuck = guard(&log);
    stuck.stuck = vec![git.clone()];
    let first = Ok(script(vec![Next::Changed(vec![git])]));
    watch(first, never_armed, &stuck, &session(&log, 1));
    assert_eq!(
        log.lines(),
        ["changed", "said", "end", "retry", "moved .git", "notice"]
    );
}

/// A guard whose moves never go through, even once the session ended.
struct Stuck<'a>(FakeGuard<'a>);

impl Guard for Stuck<'_> {
    fn changed(&self, paths: &[PathBuf]) -> Live {
        self.0.moved(paths, false)
    }

    fn rescan(&self) -> Live {
        Live::default()
    }

    fn retry(&self, failed: &[Failed]) -> Live {
        self.0.log.push("retry");
        self.0.moved(&paths(failed), false)
    }
}

#[test]
fn a_git_still_stuck_after_the_end_is_tried_and_said() {
    let log = Log::default();
    let git = PathBuf::from(".git");
    let never = FakeGuard {
        log: &log,
        stuck: vec![git.clone()],
        rescan_finds: Vec::new(),
    };
    let first = Ok(script(vec![Next::Changed(vec![git])]));
    watch(first, never_armed, &Stuck(never), &session(&log, 1));
    let lines = log.lines();
    assert_eq!(lines[..2], ["said", "end"]);
    assert_eq!(lines.iter().filter(|l| *l == "retry").count(), 5);
    assert_eq!(lines.last().unwrap(), "said");
    assert!(!lines.iter().any(|l| l == "notice"));
}

/// One `FILE_NOTIFY_INFORMATION` record: its action and name, padded to a `u32` boundary.
fn record(action: u32, name: &str, last: bool) -> Vec<u8> {
    let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let size = (12 + units.len()).next_multiple_of(4);
    let next = if last {
        0
    } else {
        u32::try_from(size).unwrap()
    };
    let mut bytes = Vec::new();
    bytes.extend(next.to_le_bytes());
    bytes.extend(action.to_le_bytes());
    bytes.extend(u32::try_from(units.len()).unwrap().to_le_bytes());
    bytes.extend(units);
    bytes.resize(size, 0);
    bytes
}

#[test]
fn only_names_made_or_renamed_to_are_read_from_the_buffer() {
    let mut buffer = Vec::new();
    for (action, name) in [
        (1, ".git"),     // FILE_ACTION_ADDED
        (2, "gone"),     // REMOVED
        (3, "edited"),   // MODIFIED
        (4, "old-name"), // RENAMED_OLD_NAME
        (5, "sub/.GIT"), // RENAMED_NEW_NAME
    ] {
        buffer.extend(record(action, name, false));
    }
    buffer.extend(record(1, "foo", true));
    let names = added_names(&buffer).unwrap();
    assert_eq!(names, [".git", "sub/.GIT", "foo"].map(PathBuf::from));
}

#[test]
fn a_buffer_that_doesnt_read_as_records_is_an_overflow() {
    let whole = record(1, ".git", true);
    assert_eq!(added_names(&whole[..10]), None);
    let mut long = whole;
    long[8] = 200;
    assert_eq!(added_names(&long), None);
    let mut loops = record(1, "a", false);
    loops[0..4].copy_from_slice(&64_u32.to_le_bytes());
    assert_eq!(added_names(&loops), None);
}

/// The agent's refusal: the line the watch says for each item carries the plain words, and the
/// host reads it back as the denial its sink takes.
#[test]
fn each_item_moved_is_said_in_plain_words_the_host_reads_back_as_a_denial() {
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
    }
    let log = Log::default();
    let said = Said(RefCell::new(Vec::new()));
    let git = PathBuf::from("Precedents").join(".git");
    watch(
        Ok(script(vec![Next::Changed(vec![git.clone()])])),
        never_armed,
        &guard(&log),
        &said,
    );

    let lines = said.0.borrow();
    let expected = format!(
        "clauseo-sandbox-run: Clauseo set aside {}: an agent can't create a git repository \
         inside a matter. Clone or init in your scratch folder instead.",
        git.display()
    );
    assert_eq!(*lines, [expected]);
    assert_eq!(
        denial_from_line(&lines[0]),
        Some(Denial::SetAside {
            path: git,
            what: SetAsideWhat::GitRepository,
        })
    );
    let link = SetAsideWhat::LinkOut.words(Path::new("foo"));
    assert!(link.starts_with("Clauseo set aside foo: an agent can't make a link"));
    assert_eq!(
        denial_from_line(&format!("clauseo-sandbox-run: {link}")),
        Some(Denial::SetAside {
            path: PathBuf::from("foo"),
            what: SetAsideWhat::LinkOut,
        })
    );
    assert_eq!(
        denial_from_line("clauseo-sandbox-run: git watch: armed again"),
        None
    );
}
