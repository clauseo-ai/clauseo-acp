//! Leftover session marks (`macos/sweep/mark.rs`): a mark goes with its last holder once no
//! process matches it, and a later launcher removes an earlier host's marks that no process
//! matches; a mark a live process still matches is never removed.

#![expect(
    clippy::disallowed_methods,
    reason = "the tests lay out an earlier host's mark by hand and wait on the real clock"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launcher, PassthroughLauncher, SpawnOptions};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::support::{World, launcher, marks_of};
use crate::sweep::{Agent, deadline, sleeper};

#[test]
fn a_mark_goes_with_its_last_holder_once_nothing_runs() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let mut agent = sleeper(&launcher, &w);
    let mark = launcher.session_mark().path().to_owned();
    // The launch's session handle still holds the mark.
    drop(launcher);
    assert!(mark.exists());
    let gone = agent.session.end(deadline(5)).unwrap();
    assert_eq!(gone.ended, 1);
    async_io::block_on(agent.handle.exited());
    drop(agent);
    // The watchdog holds it too, until its next poll finds the session empty.
    assert!(gone_within(&mark), "the unused mark stayed");
    assert!(!mark.parent().unwrap().exists());
}

/// Whether `path` is gone within 3 s.
fn gone_within(path: &std::path::Path) -> bool {
    let start = std::time::Instant::now();
    while path.exists() && start.elapsed() < Duration::from_secs(3) {
        std::thread::park_timeout(Duration::from_millis(20));
    }
    !path.exists()
}

#[test]
fn a_mark_a_live_process_matches_survives_its_drop_and_the_clean_up() {
    let w = World::new();
    let policy = w.builder().build().unwrap();
    let marks = marks_of(&policy);
    // A launch's mark, dropped by its launcher and its session handle while the agent runs.
    let (first, _) = launcher(policy.clone());
    let agent = sleeper(&first, &w);
    let ours = first.session_mark().path().to_owned();
    let Agent {
        mut handle,
        session,
        ..
    } = agent;
    drop(session);
    drop(first);
    assert!(ours.exists(), "the mark of a running agent went");

    // An earlier host's mark (its pid runs no longer) that a live process still matches: one
    // under a profile that may write everything but the canary.
    let dead = (90_000..99_000)
        .rev()
        .find(|&pid| kill(Pid::from_raw(pid), None) == Err(Errno::ESRCH))
        .unwrap();
    let left = marks.join(format!("clauseo-{dead}-0"));
    std::fs::create_dir(&left).unwrap();
    std::fs::write(left.join("mark"), "").unwrap();
    std::fs::write(left.join("canary"), "").unwrap();
    let left = dunce::canonicalize(left).unwrap();
    let profile = format!(
        "(version 1)(allow default)(deny file-write* (literal \"{}\"))",
        left.join("canary").display()
    );
    let outside = PassthroughLauncher::new(SpawnOptions::new());
    let spec = LaunchSpec::new(
        PathBuf::from("/usr/bin/sandbox-exec"),
        ["-p", &profile, "/bin/sleep", "30"]
            .iter()
            .map(OsString::from)
            .collect(),
        BTreeMap::new(),
        w.at("outside"),
    )
    .unwrap();
    let mut matcher = outside.launch(&spec).unwrap();
    std::thread::park_timeout(Duration::from_millis(500));
    let (next, _) = launcher(policy.clone());
    assert!(
        left.join("mark").exists(),
        "the clean-up removed a mark a process matches"
    );
    drop(next);

    // Once that process is gone, the next launcher's clean-up removes it.
    let pid = Pid::from_raw(i32::try_from(matcher.handle.pid()).unwrap());
    kill(pid, Signal::SIGKILL).unwrap();
    let report = async_io::block_on(matcher.handle.exited());
    assert_eq!(report.signal(), Some(9));
    let (_next, _) = launcher(policy);
    assert!(!left.exists(), "the stale mark stayed");

    // The running agent's own mark is still there (its watchdog, the last holder, keeps it),
    // and goes with a scan, its sentinel vouching, once the agent is ended.
    assert!(ours.exists());
    kill(
        Pid::from_raw(i32::try_from(handle.pid()).unwrap()),
        Signal::SIGKILL,
    )
    .unwrap();
    async_io::block_on(handle.exited());
    assert!(gone_within(&ours), "the mark stayed after its last process");
}
