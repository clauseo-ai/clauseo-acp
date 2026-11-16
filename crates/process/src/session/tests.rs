//! The marker, the matcher, and the sweep's passes over a fake table of processes.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::future::Future;
use std::time::Duration;

use super::passes::{self, MAX_PASSES, Table};
use super::{SESSION_VAR, SessionMark, SessionSwept, carries, entries};
use crate::Timer;

const THIS: &str = "0123456789abcdef0123456789abcdef";
const OTHER: &str = "fedcba9876543210fedcba9876543210";
const OWN_PID: u32 = 100;

/// A timer whose waits end at once.
struct Now;

impl Timer for Now {
    fn after(&self, _duration: Duration) -> impl Future<Output = ()> + Send {
        std::future::ready(())
    }
}

/// One fake process.
#[derive(Debug, Clone)]
struct Fake {
    start: u64,
    ours: bool,
    environ: Vec<u8>,
}

fn fake(start: u64, ours: bool, vars: &[&str]) -> Fake {
    let mut environ = Vec::new();
    for var in vars {
        environ.extend_from_slice(var.as_bytes());
        environ.push(0);
    }
    Fake {
        start,
        ours,
        environ,
    }
}

fn marker(value: &str) -> String {
    format!("{SESSION_VAR}={value}")
}

/// A table of fake processes. `on_pin` runs when a pid is pinned (a reuse between the listing
/// and the kill); `on_kill` when one is signalled (what it forked as it died).
#[derive(Default)]
struct Machine {
    procs: RefCell<BTreeMap<u32, Fake>>,
    killed: RefCell<Vec<u32>>,
    listings: RefCell<u32>,
    unlistable: bool,
    undying: bool,
    on_pin: RefCell<BTreeMap<u32, Fake>>,
    on_kill: RefCell<BTreeMap<u32, (u32, Fake)>>,
}

impl Machine {
    fn with(procs: &[(u32, Fake)]) -> Machine {
        Machine {
            procs: RefCell::new(procs.iter().cloned().collect()),
            ..Machine::default()
        }
    }

    fn sweep(&self, value: &str) -> SessionSwept {
        let entry = marker(value).into_bytes();
        futures::executor::block_on(passes::sweep(self, &entry, &Now))
    }

    fn killed(&self) -> BTreeSet<u32> {
        self.killed.borrow().iter().copied().collect()
    }
}

impl Table for Machine {
    type Pin = ();

    fn pids(&self) -> Option<Vec<u32>> {
        *self.listings.borrow_mut() += 1;
        (!self.unlistable).then(|| self.procs.borrow().keys().copied().collect())
    }

    fn own(&self) -> u32 {
        OWN_PID
    }

    fn marked(&self, pid: u32, entry: &[u8]) -> Option<u64> {
        let procs = self.procs.borrow();
        let found = procs.get(&pid)?;
        (found.ours && carries(entries(&found.environ), entry)).then_some(found.start)
    }

    fn pin(&self, pid: u32) -> Option<()> {
        if let Some(next) = self.on_pin.borrow_mut().remove(&pid) {
            self.procs.borrow_mut().insert(pid, next);
        }
        self.procs.borrow().contains_key(&pid).then_some(())
    }

    fn signal(&self, pid: u32, _pin: &()) -> bool {
        self.killed.borrow_mut().push(pid);
        if let Some((child, forked)) = self.on_kill.borrow_mut().remove(&pid) {
            self.procs.borrow_mut().insert(child, forked);
        }
        if self.undying {
            // It forks a fresh copy of itself as it dies, each time.
            let next = pid + 1_000;
            let copy = self.procs.borrow().get(&pid).cloned();
            if let Some(copy) = copy {
                self.procs.borrow_mut().insert(next, copy);
            }
        }
        self.procs.borrow_mut().remove(&pid);
        true
    }

    fn runs(&self, pid: u32, start: u64) -> bool {
        self.procs
            .borrow()
            .get(&pid)
            .is_some_and(|found| found.start == start)
    }
}

#[test]
fn a_mark_is_32_lowercase_hex_digits_and_reads_back_from_its_env() {
    let mark = SessionMark::from_random([0xab; 16]);
    assert_eq!(mark.value(), "ab".repeat(16));
    let env: BTreeMap<OsString, OsString> = [mark.var()].into_iter().collect();
    assert_eq!(SessionMark::of_env(&env), Some(mark.clone()));
    assert_eq!(mark.entry(), marker(&"ab".repeat(16)).into_bytes());
}

#[test]
fn an_env_without_a_well_formed_mark_has_none() {
    let one = |name: &str, value: &str| -> BTreeMap<OsString, OsString> {
        [(OsString::from(name), OsString::from(value))]
            .into_iter()
            .collect()
    };
    assert_eq!(SessionMark::of_env(&BTreeMap::new()), None);
    assert_eq!(SessionMark::of_env(&one("clauseo_session", THIS)), None);
    assert_eq!(
        SessionMark::of_env(&one(SESSION_VAR, &THIS.to_uppercase())),
        None
    );
    assert_eq!(SessionMark::of_env(&one(SESSION_VAR, &THIS[1..])), None);
    assert_eq!(
        SessionMark::of_env(&one(SESSION_VAR, &format!("{THIS}0"))),
        None
    );
    assert_eq!(SessionMark::of_env(&one(SESSION_VAR, "")), None);
}

#[test]
fn only_the_exact_entry_matches() {
    let entry = marker(THIS).into_bytes();
    let block = |vars: &[&str]| fake(1, true, vars).environ;
    let holds = |vars: &[&str]| carries(entries(&block(vars)), &entry);
    assert!(holds(&["PATH=/bin", &marker(THIS), "HOME=/Users/sharma"]));
    assert!(!holds(&["PATH=/bin"]), "no marker");
    assert!(!holds(&[&marker(OTHER)]), "another session's");
    assert!(!holds(&[&format!("{}0", marker(THIS))]), "a longer value");
    assert!(!holds(&[&marker(&THIS[..31])]), "a shorter value");
    assert!(!holds(&[&format!("X{}", marker(THIS))]), "another name");
    assert!(
        !holds(&[&format!("NOTE={}", marker(THIS))]),
        "inside a value"
    );
    assert!(!holds(&[&format!("{SESSION_VAR}= {THIS}")]), "a space");
}

#[test]
fn the_sweep_kills_this_sessions_processes_and_nothing_else() {
    let machine = Machine::with(&[
        (10, fake(1, true, &["PATH=/bin", &marker(THIS)])),
        (11, fake(2, true, &[&marker(THIS)])),
        (20, fake(3, true, &[&marker(OTHER)])),
        (21, fake(4, true, &["PATH=/bin"])),
        (22, fake(5, false, &[&marker(THIS)])),
        (OWN_PID, fake(6, true, &[&marker(THIS)])),
    ]);
    let swept = machine.sweep(THIS);
    assert_eq!(
        swept,
        SessionSwept::Swept {
            ended: 2,
            left: false
        }
    );
    assert_eq!(machine.killed(), BTreeSet::from([10, 11]));
    let left: BTreeSet<u32> = machine.procs.borrow().keys().copied().collect();
    assert_eq!(left, BTreeSet::from([20, 21, 22, OWN_PID]));
}

#[test]
fn a_pid_reused_before_the_kill_is_spared() {
    let machine = Machine::with(&[(10, fake(1, true, &[&marker(THIS)]))]);
    // Between the listing and the kill, pid 10 exits and an unrelated process gets the pid.
    machine
        .on_pin
        .borrow_mut()
        .insert(10, fake(9, true, &["PATH=/bin"]));
    let swept = machine.sweep(THIS);
    assert_eq!(
        swept,
        SessionSwept::Swept {
            ended: 0,
            left: false
        }
    );
    assert!(machine.killed().is_empty());
    assert!(machine.procs.borrow().contains_key(&10));
}

#[test]
fn a_pid_reused_by_another_marked_process_is_left_for_the_next_pass() {
    let machine = Machine::with(&[(10, fake(1, true, &[&marker(THIS)]))]);
    machine
        .on_pin
        .borrow_mut()
        .insert(10, fake(9, true, &[&marker(THIS)]));
    let swept = machine.sweep(THIS);
    // The first pass spares it (its start changed); the second lists it afresh and kills it.
    assert_eq!(
        swept,
        SessionSwept::Swept {
            ended: 1,
            left: false
        }
    );
    assert_eq!(*machine.killed.borrow(), vec![10]);
}

#[test]
fn what_a_killed_process_forked_is_caught_by_the_next_pass() {
    let machine = Machine::with(&[(10, fake(1, true, &[&marker(THIS)]))]);
    machine
        .on_kill
        .borrow_mut()
        .insert(10, (11, fake(2, true, &[&marker(THIS)])));
    let swept = machine.sweep(THIS);
    assert_eq!(
        swept,
        SessionSwept::Swept {
            ended: 2,
            left: false
        }
    );
    assert_eq!(*machine.killed.borrow(), vec![10, 11]);
    assert_eq!(*machine.listings.borrow(), 3);
}

#[test]
fn the_passes_are_bounded() {
    let mut machine = Machine::with(&[(10, fake(1, true, &[&marker(THIS)]))]);
    machine.undying = true;
    let swept = machine.sweep(THIS);
    let SessionSwept::Swept { ended, left } = swept else {
        panic!("{swept:?}");
    };
    assert!(left, "a process that forks as it dies outlasts the passes");
    assert_eq!(ended, MAX_PASSES as usize);
    assert_eq!(*machine.listings.borrow(), MAX_PASSES + 1);
}

#[test]
fn a_failed_listing_ends_nothing() {
    let mut machine = Machine::with(&[(10, fake(1, true, &[&marker(THIS)]))]);
    machine.unlistable = true;
    assert_eq!(machine.sweep(THIS), SessionSwept::NotListed);
    assert!(machine.killed().is_empty());
}

#[test]
fn procargs2_gives_the_environment_after_the_arguments() {
    let mut buffer = 2_i32.to_ne_bytes().to_vec();
    buffer.extend_from_slice(b"/bin/sh\0\0\0\0sh\0-c\0PATH=/bin\0");
    buffer.extend_from_slice(marker(THIS).as_bytes());
    buffer.extend_from_slice(b"\0\0executable_path=/bin/sh\0ptr_munge=\0\0\0");
    let env = crate::os::procargs::environment(&buffer).unwrap();
    assert_eq!(env, vec![b"PATH=/bin".as_slice(), marker(THIS).as_bytes()]);
    // An argument that looks like the entry is not environment.
    let mut arg = 1_i32.to_ne_bytes().to_vec();
    arg.extend_from_slice(b"/usr/bin/env\0");
    arg.extend_from_slice(marker(THIS).as_bytes());
    arg.extend_from_slice(b"\0\0");
    assert_eq!(
        crate::os::procargs::environment(&arg),
        Some(Vec::<&[u8]>::new())
    );
    // A truncated answer is no answer.
    let mut short = 5_i32.to_ne_bytes().to_vec();
    short.extend_from_slice(b"/bin/sh\0sh\0");
    assert_eq!(crate::os::procargs::environment(&short), None);
    assert_eq!(crate::os::procargs::environment(&[1, 0]), None);
}

#[cfg(target_os = "linux")]
#[test]
fn the_status_uid_line_gives_the_real_and_effective_uid() {
    use crate::os::linux_uids as uids;
    let status = "Name:\tsleep\nUmask:\t0022\nState:\tS (sleeping)\nUid:\t1000\t1001\t1002\t1003\n";
    assert_eq!(uids(status), Some((1000, 1001)));
    assert_eq!(uids("Name:\tsleep\n"), None);
    assert_eq!(uids("Uid:\tx\t1\n"), None);
}
