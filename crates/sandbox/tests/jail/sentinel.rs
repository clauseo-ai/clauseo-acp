//! The session sweep's sentinel and the launch it belongs to (`macos/sweep/sentinel.rs`): it
//! vouches for every scan, is never found, goes at session end, holds no descriptor of the
//! host's, and the agent cannot signal it. Without it the gate refuses but still ends what
//! matched, and the watchdog ends what matched too; killed mid-session, it ends the session and
//! is restarted. A launcher makes one launch, and its mark takes no write.

#![expect(
    clippy::disallowed_methods,
    reason = "the tests hand the agent a pid through a file and wait on the real clock"
)]

use std::collections::BTreeMap;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clauseo_process::{LaunchSpec, Launcher};
use clauseo_sandbox::macos::sweep::{SweepError, end_session, processes, switch_to_read_only};
use clauseo_sandbox::{ProcessCapNotice, SeatbeltLauncher};
use nix::fcntl::{FcntlArg, FdFlag, fcntl};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::escaper::ESCAPE_DIR;
use crate::support::{Jail, World, launcher, run_outside};
use crate::sweep::{
    Agent, ESCAPE, Guard, alive, deadline, escaping, ids, left_after, pids_in, sleeper, start,
};

#[test]
fn the_sentinel_vouches_for_each_scan_and_goes_at_session_end() {
    // H1: a process under the launch's own profile, never found, never counted.
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let mut agent = sleeper(&s, &w);
    let mark = s.session_mark();
    let sentinel = mark.sentinel().expect("the launch has a sentinel");
    let pid = i32::try_from(sentinel.pid()).unwrap();
    assert!(alive(pid));
    let command = run_outside(
        "/bin/ps",
        &["-o", "command=", "-p", &pid.to_string()],
        Path::new("/"),
    );
    assert_eq!(command.assert_ok().stdout.trim(), "/bin/cat");
    // It matches the mark (else the scan fails), and is never among what the scan finds.
    for since in [std::time::UNIX_EPOCH, mark.created()] {
        assert_eq!(ids(&processes(&[mark], since).unwrap()), [agent.pid()]);
    }
    // The gate leaves it running; session end ends it with the mark.
    let read_only = switch_to_read_only(&[mark], deadline(5)).unwrap();
    assert_eq!(read_only.gone().ended, 1);
    assert!(alive(pid), "the gate ended the sentinel");
    let report = async_io::block_on(agent.handle.exited());
    assert_eq!(report.signal(), Some(9), "{report:?}");
    assert!(agent.session.processes_gone());
    let gone = end_session(&[mark], deadline(5)).unwrap();
    assert_eq!(gone.ended, 0);
    assert!(!alive(pid), "the sentinel outlived session end");
    assert_eq!(mark.sentinel(), None);
    assert!(!mark.path().exists() && !mark.canary().exists());
}

#[test]
fn the_agent_cannot_signal_its_sentinel() {
    // D-H1's premise: the sentinel is a separate `sandbox-exec` run of the same profile, and
    // `(allow signal (target same-sandbox))` means the same sandbox instance, not the same
    // profile: every signal the agent sends it is refused (EPERM), `kill -0` included.
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let file = w.at("m1/sentinel-pid");
    let (mut agent, first) = start(
        &s,
        &w.at("m1"),
        "echo up; while [ ! -s \"$1\" ]; do sleep 0.05; done; p=\"$(cat \"$1\")\"; \
         for sig in 0 STOP TERM KILL; do kill -s \"$sig\" \"$p\" 2>&1; echo \"$sig rc=$?\"; done; \
         exec /bin/sleep 30",
        &[file.to_str().unwrap()],
        &[],
    );
    assert_eq!(first, "up");
    let mark = Arc::clone(s.session_mark());
    let sentinel = mark.sentinel().unwrap();
    let pid = i32::try_from(sentinel.pid()).unwrap();
    std::fs::write(&file, pid.to_string()).unwrap();
    for sig in ["0", "STOP", "TERM", "KILL"] {
        let refused = agent.line();
        assert!(
            refused.contains("Operation not permitted"),
            "{sig}: {refused}"
        );
        assert_eq!(agent.line(), format!("{sig} rc=1"));
    }
    assert!(alive(pid), "the agent ended its sentinel");
    assert_eq!(mark.sentinel(), Some(sentinel));
    assert_eq!(
        ids(&processes(&[&mark], std::time::UNIX_EPOCH).unwrap()),
        [agent.pid()]
    );
    let _gone = end_session(&[&mark], deadline(5)).unwrap();
}

#[test]
fn the_sentinel_holds_no_descriptor_but_its_own() {
    // D-M1: the sentinel starts as the launches do, every other descriptor closed: one this
    // process leaves open across exec does not reach it.
    let (reader, writer) = std::io::pipe().unwrap();
    fcntl(&writer, FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
    let leaked = writer.as_raw_fd();
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let agent = sleeper(&s, &w);
    let pid = s.session_mark().sentinel().unwrap().pid();
    let listed = run_outside(
        "/usr/sbin/lsof",
        &["-nP", "-a", "-p", &pid.to_string(), "-Ff"],
        Path::new("/"),
    );
    let fds: Vec<i32> = listed
        .stdout
        .lines()
        .filter_map(|line| line.strip_prefix('f')?.parse().ok())
        .collect();
    assert_eq!(fds, [0, 1, 2], "fd {leaked} leaked? {listed:#?}");
    drop((reader, writer, agent));
    let _gone = end_session(&[s.session_mark()], deadline(5)).unwrap();
}

/// An escaping launch: the agent, its escaped process (adopted by launchd), the launch's
/// notices and the guard that kills the escaped process should the test fail.
fn escaped_launch(w: &World) -> (Jail, Agent, i32, Arc<Mutex<Vec<ProcessCapNotice>>>, Guard) {
    let guard = Guard::default();
    let (launcher, notices) = escaping(w);
    let exe = std::env::current_exe().unwrap();
    let dir = w.dir("m1/escape");
    let (agent, first) = start(
        &launcher,
        &w.at("m1"),
        ESCAPE,
        &[exe.to_str().unwrap()],
        &[(ESCAPE_DIR, dir.to_str().unwrap())],
    );
    assert_eq!(first, "escaped");
    let pids = pids_in(&dir.join("pids"), 1);
    guard.keep(&pids);
    let [escaped] = pids[..] else {
        panic!("the escaper logged {pids:?}");
    };
    (launcher, agent, escaped, notices, guard)
}

/// The notices `told` holds, once it holds one, within 5 s.
fn notices_soon(told: &Mutex<Vec<ProcessCapNotice>>) -> Vec<ProcessCapNotice> {
    let start = Instant::now();
    while told.lock().unwrap().is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::park_timeout(Duration::from_millis(20));
    }
    told.lock().unwrap().clone()
}

#[test]
fn an_unvouched_gate_still_ends_what_matched() {
    // (a): with its sentinel ended no scan is vouched for, so the gate refuses, but a match is
    // positive evidence: the agent and its escaped process are ended all the same.
    let w = World::new();
    let (s, mut agent, escaped, _, _guard) = escaped_launch(&w);
    let mark = Arc::clone(s.session_mark());
    mark.end_sentinel();
    let refused = switch_to_read_only(&[&mark], deadline(5)).unwrap_err();
    assert!(
        matches!(refused.why, SweepError::Unknown { .. }),
        "{refused:?}"
    );
    assert!(refused.left.is_empty(), "{refused:?}");
    assert_eq!(left_after(&[escaped]), [] as [i32; 0]);
    let report = async_io::block_on(agent.handle.exited());
    assert_eq!(report.signal(), Some(9), "{report:?}");
    // Nothing vouches for the mark any longer, so nothing removes it: the test does.
    assert!(end_session(&[&mark], deadline(5)).is_err());
    mark.remove().unwrap();
}

#[test]
fn an_unvouched_watchdog_ends_what_matched() {
    // (c): the process cap's sweep cannot vouch either; it ends the agent and the escaped
    // process (which its session no longer holds), and tells the lawyer.
    let w = World::new();
    let (s, mut agent, escaped, notices, _guard) = escaped_launch(&w);
    let mark = Arc::clone(s.session_mark());
    mark.end_sentinel();
    let report = async_io::block_on(agent.handle.exited());
    assert_eq!(report.signal(), Some(9), "{report:?}");
    assert_eq!(left_after(&[escaped]), [] as [i32; 0]);
    assert_eq!(notices_soon(&notices), [ProcessCapNotice::CapFailed]);
    mark.remove().unwrap();
}

#[test]
fn a_killed_sentinel_ends_the_session_and_is_restarted() {
    // (d) and (b): the agent cannot signal its sentinel, so a sentinel killed mid-session means
    // tampering; the watchdog ends the agent and its escaped process, tells the lawyer, and
    // restarts the sentinel, which vouches again. The test kills it (it started it, through
    // the launcher).
    let w = World::new();
    let (s, mut agent, escaped, notices, _guard) = escaped_launch(&w);
    let mark = Arc::clone(s.session_mark());
    let first = mark.sentinel().unwrap();
    let pid = i32::try_from(first.pid()).unwrap();
    kill(Pid::from_raw(pid), Signal::SIGKILL).unwrap();
    let report = async_io::block_on(agent.handle.exited());
    assert_eq!(report.signal(), Some(9), "{report:?}");
    assert_eq!(left_after(&[escaped]), [] as [i32; 0]);
    assert_eq!(notices_soon(&notices), [ProcessCapNotice::Tampered]);
    let start = Instant::now();
    while mark.sentinel() == Some(first) && start.elapsed() < Duration::from_secs(3) {
        std::thread::park_timeout(Duration::from_millis(20));
    }
    let second = mark.sentinel().expect("the sentinel was restarted");
    assert_ne!(second, first);
    assert!(alive(i32::try_from(second.pid()).unwrap()));
    assert_eq!(processes(&[&mark], std::time::UNIX_EPOCH).unwrap(), []);
    let gone = end_session(&[&mark], deadline(5)).unwrap();
    assert_eq!(gone.ended, 0);
    assert!(!mark.path().exists());
}

#[test]
fn a_launcher_makes_one_launch() {
    // M2 (R17): a second launch is refused, so a mark, its sentinel and the cap's sweep belong
    // to one launch. Each launcher's tag carries a random nonce (L3).
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let (t, _) = launcher(w.builder().build().unwrap());
    for tag in [s.tag(), t.tag()] {
        // `clauseo-<host pid>-<host start>-<n>-<nonce>`.
        let parts: Vec<&str> = tag.split('-').collect();
        assert_eq!(parts.len(), 5, "{tag}");
        assert_eq!(parts[1], std::process::id().to_string());
        assert!(
            parts[2].parse::<u64>().is_ok_and(|start| start > 0),
            "{tag}"
        );
        assert_eq!(parts[4].len(), 16, "{tag}");
        assert!(parts[4].bytes().all(|b| b.is_ascii_hexdigit()), "{tag}");
    }
    assert_ne!(s.tag().rsplit('-').next(), t.tag().rsplit('-').next());
    assert!(!s.has_launched());
    let agent = sleeper(&s, &w);
    assert!(s.has_launched());
    let spec = LaunchSpec::new(
        PathBuf::from("/usr/bin/true"),
        Vec::new(),
        BTreeMap::new(),
        w.at("m1"),
    )
    .unwrap();
    let launcher: &SeatbeltLauncher = &s;
    let error = launcher.launch_jailed(&spec).unwrap_err();
    assert!(error.to_string().contains("one launch"), "{error}");
    let error = Launcher::launch(launcher, &spec).unwrap_err();
    assert!(error.to_string().contains("one launch"), "{error}");
    assert_eq!(
        ids(&processes(&[s.session_mark()], std::time::UNIX_EPOCH).unwrap()),
        [agent.pid()]
    );
    let _gone = end_session(&[s.session_mark()], deadline(5)).unwrap();
}

#[test]
fn the_mark_takes_no_write() {
    // L4: the profile allows data writes to the mark, but the mark is immutable: the agent
    // cannot grow it, and the kernel still answers "allowed", so the scan finds it.
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let mark = s.session_mark();
    let (agent, first) = start(
        &s,
        &w.at("m1"),
        "{ printf x >> \"$1\"; } 2>/dev/null; echo \"rc=$?\"; exec /bin/sleep 30",
        &[mark.path().to_str().unwrap()],
        &[],
    );
    assert_eq!(first, "rc=1");
    assert_eq!(std::fs::metadata(mark.path()).unwrap().len(), 0);
    assert_eq!(
        ids(&processes(&[mark], std::time::UNIX_EPOCH).unwrap()),
        [agent.pid()]
    );
    let _gone = end_session(&[mark], deadline(5)).unwrap();
}
