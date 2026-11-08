//! The probe against real `sandbox-exec` jails: it finds a child that left the group with
//! `setsid` and was re-parented, and nothing of another launch or outside any jail.

use std::process::Stdio;

use async_process::{Child, Command};
use std::time::Duration;

use super::{Probe, Verdict, kill_with, running, running_of, start_of, this_process, verdict};
use crate::{Clock as _, SystemClock};

/// A jail allowing the probe's name, as the launcher's profile does.
const PROFILE: &str = "(version 1)(deny default)(allow process*)(allow file-read*)\
                       (allow sysctl-read)\
                       (allow mach-lookup (global-name (param \"SESSION_PROBE\")))";

/// Perl in the jail: a child calls `setsid`, forks a grandchild that sleeps, and exits, so the
/// grandchild is in a session of its own with launchd for a parent; the agent sleeps too.
const SCRIPT: &str = "use POSIX; if (fork() == 0) { POSIX::setsid(); \
                      if (fork() == 0) { sleep 30; exit } exit } sleep 30";

fn jail(probe: &Probe) -> Child {
    Command::new("/usr/bin/sandbox-exec")
        .arg("-D")
        .arg(format!("SESSION_PROBE={}", probe.name()))
        .args(["-p", PROFILE, "/usr/bin/perl", "-e", SCRIPT])
        .stdin(Stdio::null())
        .spawn()
        .unwrap()
}

/// Waits up to 5 s for `check`.
fn within(check: impl Fn() -> bool) -> bool {
    let clock = SystemClock::new();
    while clock.now() < Duration::from_secs(5) {
        if check() {
            return true;
        }
        async_io::block_on(async_io::Timer::after(Duration::from_millis(20)));
    }
    false
}

#[test]
fn the_probe_finds_a_setsid_child_and_kills_the_whole_session() {
    let mine = Probe::new("clauseo-test-probe", u64::from(std::process::id()));
    let other = Probe::new("clauseo-test-other", u64::from(std::process::id()));
    let mut agent = jail(&mine);
    let mut neighbour = jail(&other);
    let agent_pid = i32::try_from(agent.id()).unwrap();
    // The agent and the grandchild; the child that called setsid exits.
    assert!(
        within(|| mine.pids().is_some_and(|pids| pids.len() == 2)),
        "{:?}",
        mine.pids()
    );
    let pids = mine.pids().unwrap();
    assert!(pids.contains(&agent_pid), "{pids:?}");
    let escaped = *pids.iter().find(|&&pid| pid != agent_pid).unwrap();
    assert!(!pids.contains(&i32::try_from(std::process::id()).unwrap()));
    assert!(
        within(|| {
            other.pids().is_some_and(|theirs| {
                theirs.len() == 2 && theirs.iter().all(|pid| !pids.contains(pid))
            })
        }),
        "another launch's processes are its own"
    );
    assert!(mine.kill_all() >= 2);
    drop(async_io::block_on(agent.status()));
    assert!(within(|| mine.gone()), "{:?}", mine.pids());
    assert!(start_of(u32::try_from(escaped).unwrap()).is_none());
    assert!(!other.gone(), "the other launch is untouched");
    other.kill_all();
    drop(async_io::block_on(neighbour.status()));
    assert!(within(|| other.gone()));
}

#[test]
fn a_probe_name_from_a_record_must_be_one_clauseo_makes() {
    let probe = Probe::new("clauseo-12-3", 4);
    assert_eq!(probe.name(), "chat.clauseo.session.clauseo-12-3-4");
    assert_eq!(Probe::from_name(probe.name()), Some(probe));
    for bad in [
        "",
        "chat.clauseo.session.",
        "com.apple.x",
        "chat.clauseo.session.a b",
    ] {
        assert_eq!(Probe::from_name(bad), None, "{bad}");
    }
}

#[test]
fn this_process_has_a_start() {
    let (pid, start) = this_process();
    assert_eq!(pid, std::process::id());
    assert!(start.is_some());
    assert_eq!(start, start_of(pid));
}

#[test]
fn a_failed_check_keeps_the_session_running_but_never_kills() {
    use Verdict::{In, Out, Unsure};
    assert_eq!(verdict(Some(true), Some(true), Some(false)), In);
    assert_eq!(
        verdict(Some(false), Some(true), Some(false)),
        Out,
        "not sandboxed"
    );
    assert_eq!(
        verdict(Some(true), Some(false), Some(false)),
        Out,
        "another launch's"
    );
    assert_eq!(
        verdict(Some(true), Some(true), Some(true)),
        Out,
        "allows every name"
    );
    assert_eq!(verdict(None, Some(true), Some(false)), Unsure);
    assert_eq!(verdict(Some(true), None, Some(false)), Unsure);
    assert_eq!(verdict(Some(true), Some(true), None), Unsure);
    assert_eq!(verdict(None, None, None), Unsure);
    assert_eq!(
        verdict(None, None, Some(true)),
        Out,
        "an unjailed process answers plainly"
    );
    // Pid 1 is the session's, 2 errs, 3 is not the session's.
    let of = |pid| match pid {
        1 => In,
        2 => Unsure,
        _ => Out,
    };
    assert_eq!(
        running_of(&[1, 2, 3], of),
        [1, 2],
        "the erring pid keeps the session running"
    );
    assert_eq!(
        running_of(&[2, 3], of),
        [2],
        "so a session of one erring pid isn't over"
    );
    let sent = std::cell::RefCell::new(Vec::new());
    let send = |pid| {
        sent.borrow_mut().push(pid);
        true
    };
    let start = |pid| u64::try_from(pid).ok();
    assert_eq!(kill_with(&[1, 2, 3], of, start, send), 1);
    assert_eq!(*sent.borrow(), [1], "the erring pid is never killed");
}

#[test]
fn a_pid_reused_before_its_signal_is_not_signalled() {
    let reads = std::cell::Cell::new(0_u64);
    // Pid 7's start reads 100 at the listing, then 200: another process has the pid now.
    let start = |pid: i32| {
        reads.set(reads.get() + 1);
        match (pid, reads.get()) {
            (7, 1) => Some(100),
            (7, _) => Some(200),
            _ => Some(5),
        }
    };
    let sent = std::cell::RefCell::new(Vec::new());
    let send = |pid| {
        sent.borrow_mut().push(pid);
        true
    };
    assert_eq!(kill_with(&[7], |_| Verdict::In, start, send), 0);
    assert!(sent.borrow().is_empty(), "the reused pid was not signalled");
    // A pid whose start can't be read is not signalled either; an unchanged one is.
    let start = |pid: i32| (pid == 8).then_some(5);
    assert_eq!(kill_with(&[8, 9], |_| Verdict::In, start, send), 1);
    assert_eq!(*sent.borrow(), [8]);
}

#[test]
fn fail_closed_still_leaves_out_every_process_outside_the_jail() {
    // No jail carries this name: no process on the Mac, sandboxed or not, is the session's,
    // so a session's end is still seen.
    let probe = Probe::new("clauseo-test-none", u64::from(std::process::id()));
    assert_eq!(probe.pids(), Some(Vec::new()));
    assert!(probe.gone());
    assert!(running(i32::try_from(std::process::id()).unwrap()));
    assert!(!running(i32::MAX), "a pid no process has");
}
