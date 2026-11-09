//! The end sweeper's order through fakes: shut, sweep, widen, the classes, then COM; no widening
//! and no COM after a sweep that failed or a widening that did; no BITS call, and no
//! `RunExit::PoolStop` exit.

use std::io;

use super::*;
use crate::windows::runner::leftovers::{Leftovers, remove_all};

/// Each step and each removal call, in order.
#[derive(Default)]
struct Fake {
    events: Vec<String>,
    shut_fails: bool,
    sweep: Option<SweepError>,
    widen_fails: bool,
}

/// The removal's calls, recorded in the same list: one item of each kind, but no class key once
/// it was deleted.
struct Calls<'a>(&'a mut Vec<String>);

fn key(kind: Kind) -> &'static str {
    match kind {
        Kind::Classes => "classes",
        Kind::Tasks => "tasks",
        Kind::RunKeys => "run",
        Kind::Startup => "startup",
    }
}

impl Leftovers for Calls<'_> {
    type Item = Kind;
    fn list(&mut self, kind: Kind) -> io::Result<Vec<Kind>> {
        self.0.push(format!("list {}", key(kind)));
        if kind == Kind::Classes && self.0.iter().any(|event| event == "delete classes") {
            return Ok(vec![]);
        }
        Ok(vec![kind])
    }
    fn delete(&mut self, kind: Kind, _: &Kind) -> io::Result<()> {
        self.0.push(format!("delete {}", key(kind)));
        Ok(())
    }
}

impl SweeperActs for Fake {
    fn shut(&mut self) -> Result<(), (&'static str, io::Error)> {
        self.events.push("shut".to_owned());
        if self.shut_fails {
            return Err(("shut its token", io::Error::from_raw_os_error(5)));
        }
        Ok(())
    }
    fn sweep(&mut self, _: &mut dyn FnMut(&str)) -> Result<(), SweepError> {
        self.events.push("sweep".to_owned());
        self.sweep.clone().map_or(Ok(()), Err)
    }
    fn widen_for_com(&mut self) -> io::Result<()> {
        self.events.push("widen".to_owned());
        if self.widen_fails {
            return Err(io::Error::from_raw_os_error(5));
        }
        Ok(())
    }
    fn remove(&mut self, kinds: &[Kind], log: &mut dyn FnMut(&str)) -> bool {
        let mut calls = Calls(&mut self.events);
        remove_all(&mut calls, kinds, "end sweeper", log)
    }
}

fn go(fake: &mut Fake) -> (u32, Vec<String>) {
    let mut lines = Vec::new();
    let code = run(fake, &mut |line| lines.push(line.to_owned()));
    (code, lines)
}

/// The list and delete of each kind, in order.
fn removal(kinds: &[&str]) -> Vec<String> {
    kinds
        .iter()
        .flat_map(|kind| [format!("list {kind}"), format!("delete {kind}")])
        .collect()
}

fn events(head: &[&str], kinds: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = head.iter().map(|step| (*step).to_owned()).collect();
    all.extend(removal(kinds));
    all
}

#[test]
fn the_token_is_widened_only_after_the_sweep_and_before_the_classes_and_com() {
    let mut fake = Fake::default();
    let (code, _) = go(&mut fake);
    assert_eq!(code, 0);
    // The tasks are the last kind: no BITS step follows.
    let want = events(
        &["shut", "sweep", "widen"],
        &["classes", "run", "startup", "tasks"],
    );
    assert_eq!(fake.events, want);
}

#[test]
fn a_sweep_that_failed_leaves_the_token_shut_and_com_unmade() {
    let escapee = SweepError::Left {
        count: 1,
        unkillable: 1,
        images: vec!["hidden.exe".to_owned()],
    };
    for (failure, exit) in [
        (escapee, RunExit::Escapee),
        (SweepError::StillRunning, RunExit::Sweep),
    ] {
        let mut fake = Fake {
            sweep: Some(failure.clone()),
            ..Fake::default()
        };
        let (code, lines) = go(&mut fake);
        assert_eq!(code, exit.code(), "{failure:?}");
        assert_eq!(
            fake.events,
            events(&["shut", "sweep"], &["classes", "run", "startup"]),
            "{failure:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("its token stays shut")),
            "{lines:?}"
        );
    }
}

#[test]
fn a_widening_that_failed_leaves_com_unmade() {
    let mut fake = Fake {
        widen_fails: true,
        ..Fake::default()
    };
    let (code, lines) = go(&mut fake);
    assert_eq!(code, RunExit::Sweep.code());
    assert_eq!(
        fake.events,
        events(&["shut", "sweep", "widen"], &["classes", "run", "startup"])
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("its token can't be opened to COM (5)")),
        "{lines:?}"
    );
}

#[test]
fn a_shut_that_failed_stops_the_sweeper_first() {
    let mut fake = Fake {
        shut_fails: true,
        ..Fake::default()
    };
    let (code, lines) = go(&mut fake);
    assert_eq!(code, RunExit::Sweep.code());
    assert_eq!(fake.events, ["shut"]);
    assert_eq!(
        lines,
        ["clauseo-sandbox-run: the end sweeper can't shut its token (5)"]
    );
}

#[test]
fn a_clean_run_exits_0_never_pool_stop() {
    // `RunExit::PoolStop` is reserved: no sweeper exit gives it any more.
    let (code, lines) = go(&mut Fake::default());
    assert_eq!(code, 0);
    assert_ne!(code, RunExit::PoolStop.code());
    assert!(lines.iter().all(|l| !l.contains("BITS")), "{lines:?}");
}

#[test]
fn the_verify_pass_sweeps_and_removes_nothing() {
    // Clean: 0, and nothing but the shut and the sweep.
    let mut fake = Fake::default();
    let code = verify(&mut fake, &mut |_| {});
    assert_eq!(code, 0);
    assert_eq!(fake.events, ["shut", "sweep"]);
    // A survivor: RunExit::Escapee; any other failure: RunExit::Sweep. Neither widens or
    // removes.
    let survivor = SweepError::Left {
        count: 1,
        unkillable: 1,
        images: vec!["chrome-headless-shell.exe".to_owned()],
    };
    for (failure, exit) in [
        (survivor, RunExit::Escapee),
        (SweepError::Deadline, RunExit::Sweep),
    ] {
        let mut fake = Fake {
            sweep: Some(failure.clone()),
            ..Fake::default()
        };
        let mut lines = Vec::new();
        let code = verify(&mut fake, &mut |l| lines.push(l.to_owned()));
        assert_eq!(code, exit.code(), "{failure:?}");
        assert_eq!(fake.events, ["shut", "sweep"], "{failure:?}");
        assert_eq!(lines.len(), 1, "{lines:?}");
    }
    let mut fake = Fake {
        shut_fails: true,
        ..Fake::default()
    };
    assert_eq!(verify(&mut fake, &mut |_| {}), RunExit::Sweep.code());
    assert_eq!(fake.events, ["shut"]);
}
