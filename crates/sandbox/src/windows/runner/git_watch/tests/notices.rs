//! The lawyer's notice when the watch ends a session: each cause its own line, in plain words.

use std::cell::RefCell;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use super::super::{Cause, Guard, Live, Next, Session, watch};
use super::{Log, added, guard, never_armed, script};
use crate::SetAsideWhat;
use crate::windows::SetAsideFailure;
use crate::windows::set_aside::{Failed, Hooks};

/// A session that keeps every line said, whole.
#[derive(Default)]
struct Heard(RefCell<Vec<String>>);

impl Session for Heard {
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

impl Heard {
    /// The one notice of a stopped session said, checked to be the only one.
    fn stopped(&self) -> String {
        let lines = self.0.borrow();
        let stops: Vec<&String> = lines
            .iter()
            .filter(|line| line.contains("Clauseo stopped this session"))
            .collect();
        assert_eq!(stops.len(), 1, "{lines:?}");
        stops[0].clone()
    }
}

/// The notice said for `cause`, as the outer says it.
fn line(cause: Cause) -> String {
    format!("clauseo-sandbox-run: {}", cause.notice())
}

/// A guard whose moves fail without ending the session.
struct Stuck;

impl Guard for Stuck {
    fn changed(&self, _changes: &super::super::Changes, _hooks: &impl Hooks) -> Live {
        Live {
            failed: vec![Failed {
                path: PathBuf::from("a/.git"),
                failure: SetAsideFailure::Io(io::ErrorKind::ResourceBusy),
                what: SetAsideWhat::GitRepository,
            }],
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

#[test]
fn each_cause_of_a_stopped_session_tells_the_lawyer_its_own_reason() {
    let log = Log::default();

    let heard = Heard::default();
    watch(
        Ok(script(vec![added(&[".git"])])),
        never_armed,
        &guard(&log),
        &heard,
    );
    assert_eq!(heard.stopped(), line(Cause::Found));

    let (heard, mut slow) = (Heard::default(), guard(&log));
    slow.slow = true;
    watch(Ok(script(vec![Next::Overflow])), never_armed, &slow, &heard);
    assert_eq!(heard.stopped(), line(Cause::Slow));

    let heard = Heard::default();
    let floods = vec![Next::Overflow, Next::Overflow, Next::Overflow];
    watch(Ok(script(floods)), never_armed, &guard(&log), &heard);
    assert_eq!(heard.stopped(), line(Cause::Overflows));

    let (heard, mut chased) = (Heard::default(), guard(&log));
    chased.gone = vec![PathBuf::from(".git")];
    let reports = vec![added(&[".git"]), added(&[".git"]), added(&[".git"])];
    watch(Ok(script(reports)), never_armed, &chased, &heard);
    assert_eq!(heard.stopped(), line(Cause::Gone));

    let heard = Heard::default();
    watch(
        Ok(script(vec![added(&["a/.git"])])),
        never_armed,
        &Stuck,
        &heard,
    );
    assert_eq!(heard.stopped(), line(Cause::Stuck));

    // Never armed: the rescans stand in, and one past its budget says so.
    let (heard, mut slow) = (Heard::default(), guard(&log));
    slow.slow = true;
    watch(never_armed(), never_armed, &slow, &heard);
    assert_eq!(heard.stopped(), line(Cause::SlowUnwatched));
}

/// Every notice is its own, short, and says why in plain words.
#[test]
fn the_notices_are_distinct_short_and_plain() {
    let all = [
        Cause::Found,
        Cause::Slow,
        Cause::Overflows,
        Cause::Gone,
        Cause::Stuck,
        Cause::SlowUnwatched,
        Cause::WatchFailed,
    ];
    for (at, cause) in all.iter().enumerate() {
        let notice = cause.notice();
        assert!(
            notice.starts_with("Clauseo stopped this session because "),
            "{notice}"
        );
        assert!(notice.len() < 200, "{notice}");
        for word in [
            "overflow",
            "scan",
            "budget",
            "watch failed",
            "rescan",
            "error",
        ] {
            assert!(!notice.contains(word), "{notice}");
        }
        assert!(all[at + 1..].iter().all(|other| other.notice() != notice));
    }
}
