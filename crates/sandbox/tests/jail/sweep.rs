//! The session sweep (`macos/sweep.rs`) on jailed launches: a child that called `setsid`,
//! double-forked, closed every descriptor and was adopted by launchd is found and ended; another
//! launch with its own mark is never touched; an unsandboxed process, a process under another
//! profile and launchd never match; the scan logs no denial the watcher would report; the
//! process cap counts and kills escaped descendants. Every escaped process is bounded on its own
//! (a 60 s tick loop or a 20 s sleep), and the guard kills those still running at a test's end.
//!
//! The escaper is this test binary itself, run in the jail (`escaper.rs`).

#![expect(
    clippy::disallowed_methods,
    reason = "the sweep tests time the scan and wait for escaped processes on the real clock"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_process::ChildStdout;
use clauseo_process::{Handle, StderrPump};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_sandbox::macos::sweep::{
    JailedSession, ProcId, end_session, end_session_processes, processes, switch_to_read_only,
};
use clauseo_sandbox::{Access, Denial};
use clauseo_sandbox::{ProcessCapNotice, ProcessCapSession, SeatbeltLauncher};
use futures::StreamExt;
use futures::io::{AsyncBufReadExt, BufReader, Lines};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill};
use nix::unistd::{Pid, getpgid, getsid};

use crate::escaper::{ESCAPE_DIR, ESCAPE_FANOUT};
use crate::support::{World, launcher, launcher_in, marks_of, run, run_outside};

/// Kills, at the end of a test, each pid it was given that still runs: the escaped processes,
/// started by this test, in case an assertion failed before the sweep ended them.
#[derive(Default)]
struct Guard(Mutex<Vec<i32>>);

impl Guard {
    fn keep(&self, pids: &[i32]) {
        self.0.lock().unwrap().extend_from_slice(pids);
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        for pid in self.0.lock().unwrap().iter() {
            // A pid already gone answers ESRCH: nothing to do.
            match kill(Pid::from_raw(*pid), Signal::SIGKILL) {
                Ok(()) | Err(_) => {}
            }
        }
    }
}

/// A running launch: its first stdout line read, its stderr kept open.
struct Agent {
    lines: Lines<BufReader<ChildStdout>>,
    _stderr: StderrPump,
    handle: Handle,
    session: JailedSession,
}

impl Agent {
    fn pid(&self) -> u32 {
        self.handle.pid()
    }

    /// The next stdout line.
    fn line(&mut self) -> String {
        async_io::block_on(self.lines.next()).unwrap().unwrap()
    }
}

/// `/bin/sh -c script sh args...` through `launcher` in `cwd` with `env` plus a `PATH`, its
/// first line read (`script` prints one once it is ready).
fn start(
    launcher: &SeatbeltLauncher,
    cwd: &Path,
    script: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> (Agent, String) {
    let mut all = vec![
        OsString::from("-c"),
        OsString::from(script),
        OsString::from("sh"),
    ];
    all.extend(args.iter().map(OsString::from));
    let mut vars = BTreeMap::from([(OsString::from("PATH"), OsString::from("/usr/bin:/bin"))]);
    vars.extend(
        env.iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v))),
    );
    let spec = LaunchSpec::new(PathBuf::from("/bin/sh"), all, vars, cwd.to_owned()).unwrap();
    let (
        Launched {
            stdout,
            stderr,
            handle,
            ..
        },
        session,
    ) = launcher.launch_jailed(&spec).unwrap();
    let mut agent = Agent {
        lines: BufReader::new(stdout).lines(),
        _stderr: stderr,
        handle,
        session,
    };
    let first = agent.line();
    (agent, first)
}

/// A jailed sleeper of `launcher`: prints `up` and becomes a 30 s sleep.
fn sleeper(launcher: &SeatbeltLauncher, w: &World) -> Agent {
    let (agent, first) = start(
        launcher,
        &w.at("m1"),
        "echo up; exec /bin/sleep 30",
        &[],
        &[],
    );
    assert_eq!(first, "up");
    agent
}

/// A launcher for `w` whose policy runs this test binary (the escaper), and its process cap's
/// notices.
fn escaping(w: &World) -> (SeatbeltLauncher, Arc<Mutex<Vec<ProcessCapNotice>>>) {
    let exe = dunce::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let policy = w.builder().runtime(exe.parent().unwrap()).build().unwrap();
    let notices = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&notices);
    let session = ProcessCapSession::new(move |notice| kept.lock().unwrap().push(notice));
    (launcher_in(policy, session).0, notices)
}

/// The agent script that runs the escaper (`$1`), says `escaped` and sleeps.
const ESCAPE: &str = "\"$1\" --exact escaper::escaper --ignored --nocapture --test-threads 1 \
                      >/dev/null 2>&1; echo escaped; exec /bin/sleep 30";

/// The pids in `file`, once it holds `count` of them, within 10 s.
fn pids_in(file: &Path, count: usize) -> Vec<i32> {
    let start = Instant::now();
    loop {
        let pids: Vec<i32> = std::fs::read_to_string(file)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.parse().ok())
            .collect();
        if pids.len() >= count || start.elapsed() > Duration::from_secs(10) {
            return pids;
        }
        std::thread::park_timeout(Duration::from_millis(20));
    }
}

/// The lines in `file`.
fn lines_in(file: &Path) -> usize {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .count()
}

/// Whether `pid` still exists (a zombie counts until launchd reaps it).
fn alive(pid: i32) -> bool {
    kill(Pid::from_raw(pid), None) != Err(Errno::ESRCH)
}

/// The pids of `pids` still alive after up to 3 s.
fn left_after(pids: &[i32]) -> Vec<i32> {
    let start = Instant::now();
    loop {
        let alive: Vec<i32> = pids.iter().copied().filter(|&pid| alive(pid)).collect();
        if alive.is_empty() || start.elapsed() > Duration::from_secs(3) {
            return alive;
        }
        std::thread::park_timeout(Duration::from_millis(20));
    }
}

fn deadline(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}

fn ids(found: &[ProcId]) -> Vec<u32> {
    let mut pids: Vec<u32> = found.iter().map(ProcId::pid).collect();
    pids.sort_unstable();
    pids
}

#[test]
fn an_escaped_child_is_found_and_ended() {
    let w = World::new();
    let guard = Guard::default();
    let (launcher, _) = escaping(&w);
    let exe = std::env::current_exe().unwrap();
    let dir = w.dir("m1/escape");
    let (mut agent, first) = start(
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
    let ticks = dir.join("ticks");
    let start = Instant::now();
    while lines_in(&ticks) < 3 && start.elapsed() < Duration::from_secs(5) {
        std::thread::park_timeout(Duration::from_millis(20));
    }

    // It left everything the tree walk follows: adopted by launchd, in the session its exited
    // parent made, not the agent's, and not in the agent's group.
    let pid = Pid::from_raw(escaped);
    let root = Pid::from_raw(i32::try_from(agent.pid()).unwrap());
    let ppid = run_outside(
        "/bin/ps",
        &["-o", "ppid=", "-p", &escaped.to_string()],
        Path::new("/"),
    );
    assert_eq!(ppid.assert_ok().stdout.trim(), "1");
    let session = getsid(Some(pid)).unwrap();
    assert_ne!(session, root);
    assert_ne!(getpgid(Some(pid)).unwrap(), root);
    assert_eq!(getpgid(Some(pid)), Ok(session));
    assert_eq!(
        kill(session, None),
        Err(Errno::ESRCH),
        "its session's leader still runs"
    );
    assert!(lines_in(&ticks) >= 3, "the escaped process does not tick");

    // The scan finds it and the agent, and nothing else.
    let mark = Arc::clone(launcher.session_mark());
    let found = processes(&[&mark], mark.created()).unwrap();
    let want = {
        let mut want = vec![agent.pid(), escaped.unsigned_abs()];
        want.sort_unstable();
        want
    };
    assert_eq!(ids(&found), want);
    assert!(!agent.session.processes_gone());

    // The end stops and kills both; the escaped process stops writing.
    let gone = end_session_processes(&[&mark], deadline(5)).unwrap();
    assert_eq!(gone.ended, 2);
    assert_eq!(left_after(&[escaped]), [] as [i32; 0]);
    let after = lines_in(&ticks);
    std::thread::park_timeout(Duration::from_millis(300));
    assert_eq!(lines_in(&ticks), after, "the escaped process still writes");
    let report = async_io::block_on(agent.handle.exited());
    assert_eq!(report.signal(), Some(9), "{report:?}");
    assert!(agent.session.processes_gone());

    // Session end: nothing left, so the mark goes.
    let gone = end_session(&[&mark], deadline(5)).unwrap();
    assert_eq!(gone.ended, 0);
    assert!(!mark.path().exists() && !mark.canary().exists());
}

#[test]
fn another_session_with_its_own_mark_is_never_touched() {
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let (t, _) = launcher(w.builder().build().unwrap());
    let s_agent = sleeper(&s, &w);
    let t_agent = sleeper(&t, &w);
    let (s_mark, t_mark) = (s.session_mark(), t.session_mark());
    assert_ne!(s_mark.path(), t_mark.path());
    assert_eq!(
        s_mark.path().parent().and_then(Path::parent),
        t_mark.path().parent().and_then(Path::parent)
    );
    assert_eq!(
        ids(&processes(&[s_mark], s_mark.created()).unwrap()),
        [s_agent.pid()]
    );
    assert_eq!(
        ids(&processes(&[t_mark], t_mark.created()).unwrap()),
        [t_agent.pid()]
    );

    // The gate ends S, and only S.
    let read_only = switch_to_read_only(&[s_mark], deadline(5)).unwrap();
    assert_eq!(read_only.gone().ended, 1);
    let t_pid = i32::try_from(t_agent.pid()).unwrap();
    assert!(alive(t_pid));
    assert_eq!(
        ids(&processes(&[t_mark], t_mark.created()).unwrap()),
        [t_agent.pid()]
    );
    let state = run_outside(
        "/bin/ps",
        &["-o", "state=", "-p", &t_pid.to_string()],
        Path::new("/"),
    );
    assert!(
        !state.assert_ok().stdout.trim().starts_with('T'),
        "T's agent was stopped: {state:?}"
    );
    let mut s_agent = s_agent;
    let report = async_io::block_on(s_agent.handle.exited());
    assert_eq!(report.signal(), Some(9), "{report:?}");

    let mut t_agent = t_agent;
    let gone = t_agent.session.end(deadline(5)).unwrap();
    assert_eq!(gone.ended, 1);
    let report = async_io::block_on(t_agent.handle.exited());
    assert_eq!(report.signal(), Some(9), "{report:?}");
}

#[test]
fn an_unsandboxed_process_another_profile_and_launchd_never_match() {
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let agent = sleeper(&s, &w);
    // Outside the jail: a plain sleep, and one under a profile that allows everything (as an
    // Apple daemon that may write any file of the folder allows the mark and the canary).
    let outside = PassthroughLauncher::new(SpawnOptions::new());
    let spec = |program: &str, args: &[&str]| {
        LaunchSpec::new(
            PathBuf::from(program),
            args.iter().map(OsString::from).collect(),
            BTreeMap::new(),
            w.at("outside"),
        )
        .unwrap()
    };
    let plain = outside.launch(&spec("/bin/sleep", &["30"])).unwrap();
    let open = outside
        .launch(&spec(
            "/usr/bin/sandbox-exec",
            &["-p", "(version 1)(allow default)", "/bin/sleep", "30"],
        ))
        .unwrap();
    // Past the profile's start.
    std::thread::park_timeout(Duration::from_millis(500));
    let mark = s.session_mark();
    for since in [mark.created(), std::time::UNIX_EPOCH] {
        let found = ids(&processes(&[mark], since).unwrap());
        assert_eq!(found, [agent.pid()], "since {since:?}");
        for pid in [plain.handle.pid(), open.handle.pid(), 1, std::process::id()] {
            assert!(!found.contains(&pid), "{pid} matched");
        }
    }
    let _gone = agent.session.end(deadline(5)).unwrap();
    drop((plain, open));
}

#[test]
fn the_scan_logs_no_denial_the_watcher_reports() {
    let w = World::new();
    let (s, denials) = launcher(w.builder().build().unwrap());
    let (t, _) = launcher(w.builder().build().unwrap());
    let agent = sleeper(&s, &w);
    let (s_mark, t_mark) = (s.session_mark(), t.session_mark());
    // Each scan asks about S's agent: T's mark and both canaries are denied to it, which the
    // kernel would log under S's tag without NO_REPORT (about 700 lines for 3 scans, measured).
    for _ in 0..3 {
        processes(&[s_mark, t_mark], s_mark.created()).unwrap();
    }
    // The control: a real denial of S, reading its own canary, is reported.
    run(
        &s,
        "/bin/cat",
        &[s_mark.canary().to_str().unwrap()],
        &w.at("m1"),
        &[],
    )
    .assert_refused();
    let mut seen = Vec::new();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if let Ok(denial) = denials.recv_timeout(Duration::from_millis(100)) {
            seen.push(denial);
        }
    }
    let read = Denial::Path {
        path: s_mark.canary().to_owned(),
        access: Access::Read,
        class: clauseo_sandbox::PathClass::Hidden,
    };
    assert!(
        seen.contains(&read),
        "the watcher missed the control: {seen:?}"
    );
    let marks = [
        s_mark.path(),
        s_mark.canary(),
        t_mark.path(),
        t_mark.canary(),
    ];
    let from_scan: Vec<&Denial> = seen
        .iter()
        .filter(|d| matches!(d, Denial::Path { path, access: Access::Write, .. } if marks.contains(&path.as_path())))
        .collect();
    assert_eq!(from_scan, [] as [&Denial; 0]);
    let _gone = agent.session.end(deadline(5)).unwrap();
}

#[test]
fn the_process_cap_counts_and_kills_escaped_descendants() {
    let w = World::new();
    let guard = Guard::default();
    let (launcher, notices) = escaping(&w);
    let exe = std::env::current_exe().unwrap();
    let dir = w.dir("m1/escape");
    // 300 sleepers in the escaped process's own session, children of a launchd orphan: the
    // tree walk finds none of them; only the mark does.
    let (mut agent, first) = start(
        &launcher,
        &w.at("m1"),
        ESCAPE,
        &[exe.to_str().unwrap()],
        &[(ESCAPE_DIR, dir.to_str().unwrap()), (ESCAPE_FANOUT, "300")],
    );
    assert_eq!(first, "escaped");
    let pids = pids_in(&dir.join("pids"), 301);
    guard.keep(&pids);
    assert_eq!(pids.len(), 301);
    // The watchdog's sweep runs every 2 s.
    let start = Instant::now();
    while notices.lock().unwrap().is_empty() && start.elapsed() < Duration::from_secs(6) {
        std::thread::park_timeout(Duration::from_millis(50));
    }
    assert_eq!(
        *notices.lock().unwrap(),
        [ProcessCapNotice::TooManyPrograms]
    );
    assert_eq!(left_after(&pids), [] as [i32; 0]);
    // The agent was spared: its own children are none of the descendants.
    assert!(alive(i32::try_from(agent.pid()).unwrap()));
    let _gone = agent.session.end(deadline(5)).unwrap();
    let report = async_io::block_on(agent.handle.exited());
    assert_eq!(report.signal(), Some(9));
}

/// The scan's cost on this Mac, the mean of 200 with one jailed agent running.
#[test]
fn the_scan_costs_a_few_milliseconds() {
    let w = World::new();
    let (s, _) = launcher(w.builder().build().unwrap());
    let agent = sleeper(&s, &w);
    let mark = s.session_mark();
    let mut times = Vec::new();
    for since in [std::time::UNIX_EPOCH, mark.created()] {
        let start = Instant::now();
        for _ in 0..200 {
            processes(&[mark], since).unwrap();
        }
        times.push(start.elapsed() / 200);
    }
    eprintln!(
        "scan: mean of 200 {:?} with no start filter, {:?} since the mark",
        times[0], times[1]
    );
    assert!(
        times.iter().all(|t| *t < Duration::from_millis(50)),
        "{times:?}"
    );
    let _gone = agent.session.end(deadline(5)).unwrap();
}

#[test]
fn a_marks_folder_the_agent_may_write_is_refused() {
    let w = World::new();
    let policy = w.builder().build().unwrap();
    let sink: Arc<dyn clauseo_sandbox::DenialSink> = Arc::new(NoSink);
    for folder in [
        "m1",
        "data/agent-state/m1/codex",
        "data/agent-state/m1/notes",
    ] {
        let error = SeatbeltLauncher::new(
            policy.clone(),
            SpawnOptions::new(),
            Arc::clone(&sink),
            ProcessCapSession::new(|_| {}),
            &w.at(folder),
        )
        .unwrap_err();
        assert!(error.to_string().contains("may write"), "{folder}: {error}");
    }
    assert!(
        SeatbeltLauncher::new(
            policy.clone(),
            SpawnOptions::new(),
            sink,
            ProcessCapSession::new(|_| {}),
            &marks_of(&policy)
        )
        .is_ok()
    );
}

/// A sink that drops every denial.
struct NoSink;

impl clauseo_sandbox::DenialSink for NoSink {
    fn denied(&self, _: Denial) {}
}
