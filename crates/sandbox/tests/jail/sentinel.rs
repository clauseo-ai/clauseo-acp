//! The session sweep's sentinel and the launch it belongs to (`macos/sweep/sentinel.rs`): it
//! vouches for every scan, is never found, goes at session end, and without it the gate
//! refuses; the agent cannot end it. A launcher makes one launch, and its mark takes no write.

#![expect(
    clippy::disallowed_methods,
    reason = "the tests hand the agent a pid through a file and wait on the real clock"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clauseo_process::{LaunchSpec, Launcher};
use clauseo_sandbox::SeatbeltLauncher;
use clauseo_sandbox::macos::sweep::{SweepError, end_session, processes, switch_to_read_only};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::support::{World, launcher, run_outside};
use crate::sweep::{alive, deadline, ids, sleeper, start};

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
fn without_its_sentinel_the_gate_refuses() {
    // H1: the agent cannot end its sentinel (another sandbox), but if it is gone (or the
    // kernel's answers no longer match it), no scan is vouched for: unknown, never Gone.
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let file = w.at("m1/sentinel-pid");
    let (mut agent, first) = start(
        &s,
        &w.at("m1"),
        "echo up; while [ ! -s \"$1\" ]; do sleep 0.05; done; kill -9 \"$(cat \"$1\")\" 2>&1; \
         echo \"rc=$?\"; exec /bin/sleep 30",
        &[file.to_str().unwrap()],
        &[],
    );
    assert_eq!(first, "up");
    let mark = Arc::clone(s.session_mark());
    let sentinel = mark.sentinel().unwrap();
    let pid = i32::try_from(sentinel.pid()).unwrap();
    std::fs::write(&file, pid.to_string()).unwrap();
    let refused = agent.line();
    assert!(refused.contains("Operation not permitted"), "{refused}");
    assert_eq!(agent.line(), "rc=1");
    assert!(alive(pid), "the agent ended its sentinel");
    assert_eq!(
        ids(&processes(&[&mark], std::time::UNIX_EPOCH).unwrap()),
        [agent.pid()]
    );

    // The test ends it (it started it, through the launcher).
    kill(Pid::from_raw(pid), Signal::SIGKILL).unwrap();
    let start = Instant::now();
    while processes(&[&mark], std::time::UNIX_EPOCH).is_ok()
        && start.elapsed() < Duration::from_secs(3)
    {
        std::thread::park_timeout(Duration::from_millis(20));
    }
    assert!(matches!(
        processes(&[&mark], std::time::UNIX_EPOCH),
        Err(SweepError::Unknown { .. })
    ));
    assert!(!agent.session.processes_gone());
    let refused = switch_to_read_only(&[&mark], deadline(5)).unwrap_err();
    assert!(
        matches!(refused.why, SweepError::Unknown { .. }),
        "{refused:?}"
    );
    // The known agent is ended all the same.
    assert_eq!(ids(&refused.left), [agent.pid()]);
    let report = async_io::block_on(agent.handle.exited());
    assert_eq!(report.signal(), Some(9));
    assert!(end_session(&[&mark], deadline(5)).is_err());
    // Nothing vouches for the mark any longer, so nothing removes it: the test does.
    mark.remove().unwrap();
}

#[test]
fn a_launcher_makes_one_launch() {
    // M2 (R17): a second launch is refused, so a mark, its sentinel and the cap's sweep belong
    // to one launch. Each launcher's tag carries a random nonce (L3).
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let (t, _) = launcher(w.builder().build().unwrap());
    for tag in [s.tag(), t.tag()] {
        let parts: Vec<&str> = tag.split('-').collect();
        assert_eq!(parts.len(), 4, "{tag}");
        assert_eq!(parts[1], std::process::id().to_string());
        assert_eq!(parts[3].len(), 16, "{tag}");
        assert!(parts[3].bytes().all(|b| b.is_ascii_hexdigit()), "{tag}");
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
