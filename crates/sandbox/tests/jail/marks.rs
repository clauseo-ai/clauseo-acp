//! Leftover session marks (`macos/sweep/mark.rs`): a mark goes with its last holder once no
//! process matches it, and a mark a live process of a running host still matches is never
//! removed. A later launcher ends what an earlier, crashed host's marks still find, a fresh
//! control vouching for the scan, and then removes them.

#![expect(
    clippy::disallowed_methods,
    reason = "the tests lay out an earlier host's mark by hand and wait on the real clock"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launcher, PassthroughLauncher, SpawnOptions};
use futures::StreamExt;
use futures::io::{AsyncBufReadExt, BufReader};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::escaper::ESCAPE_DIR;
use crate::support::{World, launcher, marks_of};
use crate::sweep::{Agent, ESCAPE, Guard, alive, deadline, left_after, pids_in, sleeper, start};

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
fn a_mark_a_live_process_matches_survives_its_drop_and_a_stale_one_s_process_is_ended() {
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
    let left = marks.join(format!("clauseo-{dead}-1-0-00"));
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
    // The next launcher ends that process, then removes the mark.
    let (_next, _) = launcher(policy);
    let report = async_io::block_on(matcher.handle.exited());
    assert_eq!(report.signal(), Some(9), "{report:?}");
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

/// The world folder the crash test's host runs in.
const CRASH_WORLD: &str = "CLAUSEO_JAIL_CRASH_WORLD";

/// Run by the crash test as a host of its own, never on its own: in the world at
/// `CRASH_WORLD`, it launches the escaper through a launcher whose marks are the world's,
/// prints the agent's pid, and waits for the test to kill it (30 s at most).
#[test]
#[ignore = "run by the crash test as a host of its own"]
fn crash_host() {
    let Some(root) = std::env::var_os(CRASH_WORLD) else {
        return;
    };
    let w = World::reopen(PathBuf::from(root));
    let exe = dunce::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let policy = w.builder().runtime(exe.parent().unwrap()).build().unwrap();
    let (jail, _) = launcher(policy);
    let (agent, first) = start(
        &jail,
        &w.at("m1"),
        ESCAPE,
        &[exe.to_str().unwrap()],
        &[(ESCAPE_DIR, w.at("m1/escape").to_str().unwrap())],
    );
    assert_eq!(first, "escaped");
    println!("agent {}", agent.pid());
    std::thread::park_timeout(Duration::from_secs(30));
}

#[test]
fn a_crashed_host_s_escaped_process_is_ended_by_the_next_launcher() {
    // D-M2: a host is killed while its agent's escaped process runs; neither dies with it. The
    // next launcher ends both, a control of its own vouching, and removes the host's mark.
    let w = World::new();
    let guard = Guard::default();
    let dir = w.dir("m1/escape");
    let exe = std::env::current_exe().unwrap();
    let args = [
        "--exact",
        "marks::crash_host",
        "--ignored",
        "--nocapture",
        "--test-threads",
        "1",
    ];
    let env = BTreeMap::from([(OsString::from(CRASH_WORLD), w.root().as_os_str().to_owned())]);
    let spec = LaunchSpec::new(
        exe,
        args.iter().map(OsString::from).collect(),
        env,
        w.at("outside"),
    )
    .unwrap();
    let mut host = PassthroughLauncher::new(SpawnOptions::new())
        .launch(&spec)
        .unwrap();
    let mut lines = BufReader::new(host.stdout).lines();
    let agent: i32 = loop {
        let line = async_io::block_on(lines.next())
            .expect("the host ended before its agent ran")
            .unwrap();
        // libtest prints the test's name on the same line first.
        if let Some(pid) = line.split("agent ").nth(1) {
            break pid.trim().parse().unwrap();
        }
    };
    let pids = pids_in(&dir.join("pids"), 1);
    guard.keep(&pids);
    guard.keep(&[agent]);
    let [escaped] = pids[..] else {
        panic!("the escaper logged {pids:?}");
    };
    let marks = marks_of(&w.builder().build().unwrap());
    let left: Vec<PathBuf> = std::fs::read_dir(&marks)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");

    // The crash.
    let pid = Pid::from_raw(i32::try_from(host.handle.pid()).unwrap());
    kill(pid, Signal::SIGKILL).unwrap();
    let report = async_io::block_on(host.handle.exited());
    assert_eq!(report.signal(), Some(9), "{report:?}");
    std::thread::park_timeout(Duration::from_millis(300));
    assert!(alive(escaped) && alive(agent), "nothing outlived the host");
    assert!(left[0].join("mark").exists());

    // The next launcher.
    let (next, _) = launcher(w.builder().build().unwrap());
    assert_eq!(left_after(&[escaped, agent]), [] as [i32; 0]);
    assert!(!left[0].exists(), "the crashed host's mark stayed");
    drop(next);
}
