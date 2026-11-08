//! The lawyer's notice when the watch ends a session: each cause its own line, in plain words.

use std::cell::RefCell;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use super::super::{Cause, Guard, Live, Next, Pulse, Session, watch};
use super::{Log, added, guard, script};
use crate::SetAsideWhat;
use crate::windows::SetAsideFailure;
use crate::windows::set_aside::{Failed, Hooks};

/// A session that keeps every line said, whole.
#[derive(Default)]
struct Heard(RefCell<Vec<String>>, Pulse);

impl Session for Heard {
    fn end(&self) -> bool {
        true
    }

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

    fn pulse(&self) -> &Pulse {
        &self.1
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
    watch(script(vec![added(&[".git"])]), &guard(&log), &heard);
    assert_eq!(heard.stopped(), line(Cause::Found));

    let (heard, mut slow) = (Heard::default(), guard(&log));
    slow.slow = true;
    watch(script(vec![Next::Overflow]), &slow, &heard);
    assert_eq!(heard.stopped(), line(Cause::Slow));

    let heard = Heard::default();
    let floods = vec![Next::Overflow, Next::Overflow, Next::Overflow];
    watch(script(floods), &guard(&log), &heard);
    assert_eq!(heard.stopped(), line(Cause::Overflows));

    let (heard, mut chased) = (Heard::default(), guard(&log));
    chased.gone = vec![PathBuf::from(".git")];
    let reports = vec![added(&[".git"]), added(&[".git"]), added(&[".git"])];
    watch(script(reports), &chased, &heard);
    assert_eq!(heard.stopped(), line(Cause::Gone));

    let heard = Heard::default();
    watch(script(vec![added(&["a/.git"])]), &Stuck, &heard);
    assert_eq!(heard.stopped(), line(Cause::Stuck));

    // Rule 4: a watch that breaks while the agent runs ends the session, saying so.
    let heard = Heard::default();
    let broken = vec![Next::Broken(io::Error::from_raw_os_error(6))];
    watch(script(broken), &guard(&log), &heard);
    assert_eq!(heard.stopped(), line(Cause::Broken));

    // The watchdog ended it first and said its own: the watch's notice isn't said again.
    let heard = Heard::default();
    assert!(heard.1.end());
    watch(script(vec![added(&[".git"])]), &guard(&log), &heard);
    assert!(
        heard
            .0
            .borrow()
            .iter()
            .all(|line| !line.contains("Clauseo stopped this session")),
        "{:?}",
        heard.0.borrow()
    );
}

/// Review item 7: a git repository the agent made (a pool user owned it) is the AI tool's, with
/// no step to put it back; anyone else's, or one whose owner couldn't be read, may be the
/// lawyer's own, so its notice blames no one and says how to put it back.
#[test]
fn a_repository_the_agent_made_and_one_it_didnt_have_their_own_notices() {
    let (name, to) = (
        PathBuf::from("Precedents/.git"),
        PathBuf::from("aside/.git"),
    );
    let what = SetAsideWhat::GitRepository;
    assert_eq!(
        what.notice_by(&name, &to, true),
        "The AI tool tried to put a code repository in the matter. Clauseo stopped it and moved \
         Precedents/.git to aside/.git."
    );
    let lawyers = "Clauseo moved Precedents/.git to aside/.git because a git repository appeared \
                   in the matter while an agent was working. If you made it yourself, move it \
                   back after the session ends.";
    assert_eq!(what.notice_by(&name, &to, false), lawyers);
    assert_eq!(what.notice(&name, &to), lawyers, "unknown: not the agent's");
    for other in [SetAsideWhat::LinkOut, SetAsideWhat::UncheckedFolder] {
        assert_eq!(
            other.notice_by(&name, &to, true),
            other.notice_by(&name, &to, false)
        );
    }
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
        Cause::WatchFailed,
        Cause::Broken,
        Cause::Stalled,
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
