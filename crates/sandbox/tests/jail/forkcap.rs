//! The process cap (`macos/process_cap.rs`) on a jailed agent: a bounded fork bomb in a command
//! is stopped near the cap and the agent goes on to report it; an agent that forks itself past
//! the cap is ended. Each bomb is bounded on its own (a fixed tree depth or count, and every
//! process a 20 s sleep), so a cap that failed would leave nothing running for long.

#![expect(
    clippy::disallowed_methods,
    reason = "the bomb tests time a process start outside the jail while the bomb runs"
)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clauseo_process::{ExitReport, LaunchSpec, Launched, Launcher};
use clauseo_sandbox::SeatbeltLauncher;
use futures::StreamExt;
use futures::io::{AsyncBufReadExt, BufReader};
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::{Pid, getpgid, getsid};

use crate::support::{World, launcher, run_outside};

/// Windows' notice, word for word.
const NOTICE: &str = "A task tried to start too many programs at once, so Clauseo stopped it. \
                      Ask the assistant to try a smaller step.";

/// A binary tree of shells `$1` levels deep (2,047 processes at 10), each logging its pid to
/// `$2` and then becoming a 20 s sleep.
const TREE: &str = "echo $$ >> \"$2\"
if [ \"$1\" -gt 0 ]; then
  d=$(($1 - 1))
  \"$0\" \"$d\" \"$2\" &
  \"$0\" \"$d\" \"$2\" &
fi
exec /bin/sleep 20";

/// A launcher for `w`'s policy whose notices land in the returned list.
fn capped(w: &World) -> (SeatbeltLauncher, Arc<Mutex<Vec<String>>>) {
    let notices = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&notices);
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let launcher =
        launcher.on_too_many_programs(move |text| kept.lock().unwrap().push(text.to_owned()));
    (launcher, notices)
}

/// Runs `/bin/sh -c script` with `args` through `launcher` in `cwd`. `during` runs once
/// the agent's first line has come, while the agent still runs (it waits for a file `during`
/// makes: the handle holds the agent's stdin open), so the handle's tree kill at the exit hides
/// nothing from it. Returns the agent's whole stdout and its exit.
fn run_agent(
    launcher: &dyn Launcher,
    cwd: &Path,
    script: &str,
    args: &[&str],
    during: impl FnOnce(),
) -> (String, ExitReport) {
    let mut all = vec![
        OsString::from("-c"),
        OsString::from(script),
        OsString::from("sh"),
    ];
    all.extend(args.iter().map(OsString::from));
    let env = BTreeMap::from([(OsString::from("PATH"), OsString::from("/usr/bin:/bin"))]);
    let spec = LaunchSpec::new(PathBuf::from("/bin/sh"), all, env, cwd.to_owned()).unwrap();
    let Launched {
        stdout,
        stderr,
        mut handle,
        ..
    } = launcher.launch(&spec).unwrap();
    let mut lines = BufReader::new(stdout).lines();
    async_io::block_on(async {
        let agent = async {
            let mut out = String::new();
            if let Some(line) = lines.next().await {
                out.push_str(&line.unwrap());
                out.push('\n');
                during();
            }
            while let Some(line) = lines.next().await {
                out.push_str(&line.unwrap());
                out.push('\n');
            }
            (out, handle.exited().await)
        };
        let (drained, ran) = futures::join!(stderr.drain(), agent);
        drained.unwrap();
        ran
    })
}

/// The notices, once one came or 2 s passed: the watchdog says it just after the trip.
fn notices_soon(notices: &Mutex<Vec<String>>) -> Vec<String> {
    let start = Instant::now();
    while notices.lock().unwrap().is_empty() && start.elapsed() < Duration::from_secs(2) {
        std::thread::park_timeout(Duration::from_millis(20));
    }
    notices.lock().unwrap().clone()
}

/// The pids logged in `log` that still run 3 s after the agent's exit at most: a killed process
/// is a zombie until launchd reaps it.
fn survivors(log: &Path) -> Vec<i32> {
    let pids: Vec<i32> = std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| line.parse().unwrap())
        .collect();
    let start = Instant::now();
    loop {
        let alive: Vec<i32> = pids
            .iter()
            .copied()
            .filter(|&pid| kill(Pid::from_raw(pid), None) != Err(Errno::ESRCH))
            .collect();
        if alive.is_empty() || start.elapsed() > Duration::from_secs(3) {
            return alive;
        }
        std::thread::park_timeout(Duration::from_millis(50));
    }
}

#[test]
fn a_fork_bomb_in_a_command_is_stopped_and_the_agent_reports() {
    let w = World::new();
    let tree = w.script("m1/tree.sh", TREE);
    let log = w.file("m1/pids.log", "");
    let (launcher, notices) = capped(&w);
    // The lawyer's side while the bomb runs: process starts outside the jail, timed.
    let lawyer = std::thread::spawn(|| {
        (0..20)
            .map(|_| {
                let start = Instant::now();
                run_outside("/usr/bin/true", &[], Path::new("/")).assert_ok();
                std::thread::park_timeout(Duration::from_millis(25));
                start.elapsed()
            })
            .max()
            .unwrap()
    });
    let started = Instant::now();
    let script = "\"$1\" 10 \"$2\" >/dev/null 2>&1; echo \"agent: the task ended with $?\"; \
                  until [ -e go ]; do /bin/sleep 0.05; done";
    let mut left = Vec::new();
    let mut took = Duration::ZERO;
    let (out, report) = run_agent(
        &launcher,
        &w.at("m1"),
        script,
        &[tree.to_str().unwrap(), log.to_str().unwrap()],
        || {
            took = started.elapsed();
            left = survivors(&log);
            w.file("m1/go", "");
        },
    );
    let slowest = lawyer.join().unwrap();
    let made = std::fs::read_to_string(&log).unwrap().lines().count();
    eprintln!("bomb: {made} of 2047 started, stopped in {took:?}, slowest start {slowest:?}");
    eprintln!(
        "{}",
        std::fs::read_to_string(w.at("m1/err.log")).unwrap_or_default()
    );
    // The agent survived and reported: its child was SIGKILLed (128 + 9).
    assert_eq!(report.code(), Some(0), "{report:?} {out}");
    assert_eq!(out, "agent: the task ended with 137\n");
    assert_eq!(notices_soon(&notices), [NOTICE]);
    // Stopped near the cap: about 200 had logged on this Mac (the rest were mid-start).
    assert!(made < 1_000, "the bomb ran on past the cap: {made}");
    assert!(took < Duration::from_secs(10), "{took:?}");
    assert!(slowest < Duration::from_secs(2), "{slowest:?}");
    assert_eq!(left, [] as [i32; 0], "the bomb's processes still run");
}

#[test]
fn an_agent_that_forks_itself_past_the_cap_is_ended() {
    let w = World::new();
    let (launcher, notices) = capped(&w);
    let script = "i=0; while [ $i -lt 400 ]; do /bin/sleep 20 & i=$((i + 1)); done; \
                  echo survived";
    let started = Instant::now();
    let (out, report) = run_agent(&launcher, &w.at("m1"), script, &[], || {});
    assert_eq!(report.signal(), Some(9), "{report:?} {out}");
    assert_eq!(out, "");
    assert_eq!(notices_soon(&notices), [NOTICE]);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn the_jailed_agent_leads_its_session_under_the_backstop() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let per_user = run_outside(
        "/usr/sbin/sysctl",
        &["-n", "kern.maxprocperuid"],
        Path::new("/"),
    );
    let per_user: u64 = per_user.assert_ok().stdout.trim().parse().unwrap();
    let args = [
        "-c",
        "echo \"$(ulimit -u) $(ulimit -Hu)\"; exec /bin/sleep 20",
    ];
    let spec = LaunchSpec::new(
        PathBuf::from("/bin/sh"),
        args.iter().map(OsString::from).collect(),
        BTreeMap::new(),
        w.at("m1"),
    )
    .unwrap();
    let Launched { stdout, handle, .. } = launcher.launch(&spec).unwrap();
    let line = async_io::block_on(BufReader::new(stdout).lines().next());
    let line = line.unwrap().unwrap();
    let pid = Pid::from_raw(i32::try_from(handle.pid()).unwrap());
    // Read while the agent runs (a sleep now, the same process); the handle's drop ends it.
    assert_eq!(getsid(Some(pid)), Ok(pid), "not a session leader");
    assert_eq!(getpgid(Some(pid)), Ok(pid));
    let limit = per_user - 512;
    assert_eq!(line, format!("{limit} {limit}"));
    drop(handle);
}
