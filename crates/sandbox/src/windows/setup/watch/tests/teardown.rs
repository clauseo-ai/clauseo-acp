//! The watch task's removal's order over a recording fake: the key is taken only when its
//! delete is refused, a delete still refused once taken fails the run, and the task goes
//! whatever an earlier step gave.

use super::super::teardown::{KeyDelete, KeyLeft, WatchKey, WatchParts, remove_watch};

/// Each effect, in order; the key is `key` (SYSTEM's when `None`), its deletes give `deletes`
/// in turn, and `failing` fails.
#[derive(Default)]
struct Record {
    calls: Vec<&'static str>,
    key: Option<WatchKey>,
    deletes: Vec<KeyDelete>,
    failing: Option<&'static str>,
}

impl Record {
    fn call(&mut self, what: &'static str) -> Result<(), String> {
        self.calls.push(what);
        if self.failing == Some(what) {
            return Err(format!("{what} failed"));
        }
        Ok(())
    }
}

impl WatchParts for Record {
    type Error = String;

    fn end_task(&mut self) -> Result<(), String> {
        self.call("end_task")
    }

    fn stop_session(&mut self) -> Result<(), String> {
        self.call("stop_session")
    }

    fn inspect_key(&mut self) -> Result<WatchKey, String> {
        self.call("inspect_key")?;
        Ok(self.key.unwrap_or(WatchKey::System))
    }

    fn delete_key(&mut self) -> Result<KeyDelete, String> {
        self.call("delete_key")?;
        Ok(if self.deletes.is_empty() {
            KeyDelete::Gone
        } else {
            self.deletes.remove(0)
        })
    }

    fn take_key(&mut self) -> Result<(), String> {
        self.call("take_key")
    }

    fn left(&self, why: KeyLeft) -> String {
        format!("{why:?}")
    }

    fn delete_task(&mut self) -> Result<(), String> {
        self.call("delete_task")
    }
}

#[test]
fn a_key_deleted_at_once_is_never_taken() {
    let mut parts = Record::default();
    assert_eq!(remove_watch(&mut parts), Ok(()));
    let order = [
        "end_task",
        "stop_session",
        "inspect_key",
        "delete_key",
        "delete_task",
    ];
    assert_eq!(parts.calls, order);
}

#[test]
fn a_refused_delete_takes_the_key_once_and_a_second_refusal_fails_after_the_task_went() {
    let mut parts = Record {
        deletes: vec![KeyDelete::Denied],
        ..Record::default()
    };
    assert_eq!(remove_watch(&mut parts), Ok(()));
    assert_eq!(
        parts.calls[3..],
        ["delete_key", "take_key", "delete_key", "delete_task"]
    );
    let mut parts = Record {
        deletes: vec![KeyDelete::Denied, KeyDelete::Denied],
        ..Record::default()
    };
    assert_eq!(remove_watch(&mut parts), Err("StillDenied".to_owned()));
    assert_eq!(
        parts.calls[3..],
        ["delete_key", "take_key", "delete_key", "delete_task"]
    );
}

#[test]
fn the_task_goes_whatever_failed_before_it_and_the_first_failure_is_the_result() {
    for failing in [
        "end_task",
        "stop_session",
        "inspect_key",
        "delete_key",
        "take_key",
    ] {
        let mut parts = Record {
            deletes: vec![KeyDelete::Denied],
            failing: Some(failing),
            ..Record::default()
        };
        assert_eq!(remove_watch(&mut parts), Err(format!("{failing} failed")));
        assert_eq!(parts.calls.last(), Some(&"delete_task"), "{failing}");
    }
    let mut parts = Record {
        failing: Some("delete_task"),
        ..Record::default()
    };
    assert_eq!(
        remove_watch(&mut parts),
        Err("delete_task failed".to_owned())
    );
}

#[test]
fn a_link_or_a_key_not_systems_is_left_untouched_and_the_task_still_goes() {
    for (key, why) in [(WatchKey::Link, "Link"), (WatchKey::Other, "NotSystems")] {
        let mut parts = Record {
            key: Some(key),
            ..Record::default()
        };
        assert_eq!(remove_watch(&mut parts), Err(why.to_owned()));
        assert_eq!(parts.calls[2..], ["inspect_key", "delete_task"], "{why}");
    }
    let mut parts = Record {
        key: Some(WatchKey::Absent),
        ..Record::default()
    };
    assert_eq!(remove_watch(&mut parts), Ok(()));
    assert_eq!(parts.calls[2..], ["inspect_key", "delete_task"]);
}
