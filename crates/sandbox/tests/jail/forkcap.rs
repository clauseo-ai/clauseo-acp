//! The process cap (`macos/process_cap.rs`) on a jailed agent: a bounded fork bomb in a command
//! is stopped near the cap and the agent goes on to report it, the job-control variant (`set -m`,
//! each job in a group of its own and orphaned) too; an agent that forks itself past the cap is
//! ended, and so is one whose session trips the cap a third time, across launchers; zombies
//! never trip it, and an agent that exits at once launches cleanly. Each bomb is bounded on its
//! own (a fixed tree depth or count, and every process a 20 s sleep), so a cap that failed would
//! leave nothing running for long.

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
use clauseo_sandbox::{ProcessCapNotice, ProcessCapSession};
use futures::StreamExt;
use futures::io::{AsyncBufReadExt, BufReader};
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::{Pid, getpgid, getsid};

use crate::support::{World, launcher, launcher_in, run_outside};

/// A binary tree of shells `$1` levels deep (2,047 processes at 10), each logging its pid to
/// `$2` and then becoming a 20 s sleep.
const TREE: &str = "echo $$ >> \"$2\"
if [ \"$1\" -gt 0 ]; then
  d=$(($1 - 1))
  \"$0\" \"$d\" \"$2\" &
  \"$0\" \"$d\" \"$2\" &
fi
exec /bin/sleep 20";

/// [`TREE`] under job control: each shell puts its two jobs in groups of their own (`set -m`)
/// and exits, so launchd adopts them; only the leaves stay, as 20 s sleeps. Neither the agent's
/// group nor a parent walk from the agent finds any of it: only its session does.
const JOBS: &str = "set -m
echo $$ >> \"$2\"
if [ \"$1\" -gt 0 ]; then
  d=$(($1 - 1))
  \"$0\" \"$d\" \"$2\" &
  \"$0\" \"$d\" \"$2\" &
  exit 0
fi
exec /bin/sleep 20";

/// The notices one session gave.
type Told = Arc<Mutex<Vec<ProcessCapNotice>>>;

/// A session whose notices land in the returned list.
fn session() -> (ProcessCapSession, Told) {
    let told = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&told);
    let session = ProcessCapSession::new(move |notice| kept.lock().unwrap().push(notice));
    (session, told)
}

/// A launcher for `w`'s policy in `session`.
fn capped_in(w: &World, session: &ProcessCapSession) -> crate::support::Jail {
    launcher_in(w.builder().build().unwrap(), session.clone()).0
}

/// A launcher for `w`'s policy in a session of its own.
fn capped(w: &World) -> (crate::support::Jail, ProcessCapSession, Told) {
    let (session, told) = session();
    (capped_in(w, &session), session, told)
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

/// The notices, once `n` came or 10 s passed: the watchdog tells just after the trip.
fn notices_soon(told: &Mutex<Vec<ProcessCapNotice>>, n: usize) -> Vec<ProcessCapNotice> {
    let start = Instant::now();
    while told.lock().unwrap().len() < n && start.elapsed() < Duration::from_secs(10) {
        std::thread::park_timeout(Duration::from_millis(20));
    }
    told.lock().unwrap().clone()
}

/// The pids logged in `log`.
fn logged(log: &Path) -> Vec<i32> {
    std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| line.parse().unwrap())
        .collect()
}

/// The pids logged in `log` that still run 3 s after the agent's exit at most: a killed process
/// is a zombie until launchd reaps it.
fn survivors(log: &Path) -> Vec<i32> {
    let pids = logged(log);
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
    let (launcher, cap, told) = capped(&w);
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
    let made = logged(&log).len();
    eprintln!("bomb: {made} of 2047 started, stopped in {took:?}, slowest start {slowest:?}");
    // The agent survived and reported: its child was SIGKILLed (128 + 9).
    assert_eq!(report.code(), Some(0), "{report:?} {out}");
    assert_eq!(out, "agent: the task ended with 137\n");
    assert_eq!(notices_soon(&told, 1), [ProcessCapNotice::TooManyPrograms]);
    assert_eq!(cap.trips(), 1);
    // Stopped near the cap: about 200 had logged on this Mac (the rest were mid-start).
    assert!(made < 1_000, "the bomb ran on past the cap: {made}");
    assert!(took < Duration::from_secs(10), "{took:?}");
    assert!(slowest < Duration::from_secs(2), "{slowest:?}");
    assert_eq!(left, [] as [i32; 0], "the bomb's processes still run");
}

#[test]
fn a_job_control_bomb_is_stopped_by_its_session() {
    // H1: `set -m` puts every job in a group of its own and each shell exits at once, so the
    // bomb is launchd's orphans in groups that are not the agent's.
    let w = World::new();
    let jobs = w.script("m1/jobs.sh", JOBS);
    let log = w.file("m1/pids.log", "");
    let (launcher, cap, told) = capped(&w);
    let script = "\"$1\" 10 \"$2\" >/dev/null 2>&1; echo \"agent: the task ended with $?\"; \
                  until [ -e go ]; do /bin/sleep 0.05; done";
    let mut notices = Vec::new();
    let mut left = Vec::new();
    let started = Instant::now();
    let mut took = Duration::ZERO;
    let (out, report) = run_agent(
        &launcher,
        &w.at("m1"),
        script,
        &[jobs.to_str().unwrap(), log.to_str().unwrap()],
        || {
            notices = notices_soon(&told, 1);
            took = started.elapsed();
            left = survivors(&log);
            w.file("m1/go", "");
        },
    );
    let made = logged(&log).len();
    eprintln!("job-control bomb: {made} of 2047 started, stopped in {took:?}");
    // The tree's first shell exited 0 at once, its jobs detached; the agent was only paused.
    assert_eq!(report.code(), Some(0), "{report:?} {out}");
    assert_eq!(out, "agent: the task ended with 0\n");
    assert_eq!(notices, [ProcessCapNotice::TooManyPrograms]);
    assert_eq!(cap.trips(), 1);
    assert!(made < 2_047, "the bomb ran to its end: {made}");
    assert_eq!(left, [] as [i32; 0], "the bomb's processes still run");
}

#[test]
fn an_agent_that_forks_itself_past_the_cap_is_ended() {
    let w = World::new();
    let (launcher, _, told) = capped(&w);
    let script = "i=0; while [ $i -lt 400 ]; do /bin/sleep 20 & i=$((i + 1)); done; \
                  echo survived";
    let started = Instant::now();
    let (out, report) = run_agent(&launcher, &w.at("m1"), script, &[], || {});
    assert_eq!(report.signal(), Some(9), "{report:?} {out}");
    assert_eq!(out, "");
    assert_eq!(notices_soon(&told, 1), [ProcessCapNotice::AssistantStopped]);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn two_launchers_of_one_session_count_trips_together() {
    // M1, D2: launcher A's agent bombs once, launcher B's once, and both agents report it; the
    // third bomb, A's again, ends that agent. The first notice comes once, not once a launcher.
    let w = World::new();
    let tree = w.script("m1/tree.sh", TREE);
    let log = w.file("m1/pids.log", "");
    let (session, told) = session();
    let (a, b) = (capped_in(&w, &session), capped_in(&w, &session));
    let args = [tree.to_str().unwrap(), log.to_str().unwrap()];
    let bomb = "\"$1\" 10 \"$2\" >/dev/null 2>&1; echo \"ended $?\"";
    for (n, launcher) in [(1, &a), (2, &b)] {
        let (out, report) = run_agent(launcher, &w.at("m1"), bomb, &args, || {});
        assert_eq!(report.code(), Some(0), "trip {n}: {report:?} {out}");
        assert_eq!(out, "ended 137\n", "trip {n}");
        assert_eq!(session.trips(), n);
    }
    assert_eq!(notices_soon(&told, 1), [ProcessCapNotice::TooManyPrograms]);
    let (out, report) = run_agent(&a, &w.at("m1"), bomb, &args, || {});
    eprintln!("three trips: {} shells started in all", logged(&log).len());
    assert_eq!(report.signal(), Some(9), "{report:?} {out}");
    assert_eq!(out, "");
    assert_eq!(session.trips(), 3);
    assert_eq!(
        notices_soon(&told, 2),
        [
            ProcessCapNotice::TooManyPrograms,
            ProcessCapNotice::AssistantStopped
        ]
    );
    assert_eq!(
        survivors(&log),
        [] as [i32; 0],
        "the bombs' processes still run"
    );
}

#[test]
fn an_agent_that_exits_at_once_launches_cleanly() {
    // D3: an empty session at the first count is the agent's normal end: no notice, no error.
    let w = World::new();
    let (launcher, cap, told) = capped(&w);
    for _ in 0..20 {
        let spec = LaunchSpec::new(
            PathBuf::from("/bin/sh"),
            vec![OsString::from("-c"), OsString::from("exit 0")],
            BTreeMap::new(),
            w.at("m1"),
        )
        .unwrap();
        let Launched {
            stdout,
            stderr,
            mut handle,
            ..
        } = launcher.launch(&spec).unwrap();
        let report = async_io::block_on(async {
            let mut lines = BufReader::new(stdout).lines();
            let (drained, _, report) =
                futures::join!(stderr.drain(), lines.next(), handle.exited());
            drained.unwrap();
            report
        });
        assert_eq!(report.code(), Some(0), "{report:?}");
    }
    std::thread::park_timeout(Duration::from_millis(200));
    assert_eq!(cap.trips(), 0);
    assert!(
        told.lock().unwrap().is_empty(),
        "{:?}",
        told.lock().unwrap()
    );
}

#[test]
fn zombies_never_trip_the_cap() {
    // M2: 400 children exit and stay unreaped (perl does not wait), while the agent starts
    // short-lived programs beside them.
    let w = World::new();
    let (launcher, cap, told) = capped(&w);
    let script = "/usr/bin/perl -e 'for (1..400) { fork or exit 0 } print \"held\\n\"; \
                  $| = 1; sleep 3' & \
                  i=0; while [ $i -lt 100 ]; do /usr/bin/true; i=$((i + 1)); done; wait; \
                  echo done";
    let (out, report) = run_agent(&launcher, &w.at("m1"), script, &[], || {});
    assert_eq!(report.code(), Some(0), "{report:?} {out}");
    assert_eq!(out, "held\ndone\n");
    assert_eq!(cap.trips(), 0);
    assert!(told.lock().unwrap().is_empty());
}

#[test]
fn launchd_starts_nothing_for_the_agent() {
    // L7: the profile allows no Mach lookup of launchd's or LaunchServices' services, so a job
    // the agent submits is refused and never runs outside its tree.
    let w = World::new();
    let (launcher, _, _) = capped(&w);
    let label = format!("chat.clauseo.forkcap-probe.{}", std::process::id());
    let script = "said=$(/bin/launchctl submit -l \"$1\" -- /bin/sleep 30 2>&1); \
                  echo \"submit $? $said\"";
    let (out, report) = run_agent(&launcher, &w.at("m1"), script, &[&label], || {});
    let listed = run_outside("/bin/launchctl", &["list", &label], Path::new("/"));
    // Whatever the outcome, nothing of ours stays with launchd.
    drop(run_outside(
        "/bin/launchctl",
        &["remove", &label],
        Path::new("/"),
    ));
    eprintln!(
        "launchctl in the jail: {out:?}; outside, list said {:?}",
        listed.code
    );
    assert_eq!(report.code(), Some(0), "{report:?} {out}");
    assert!(
        !out.starts_with("submit 0"),
        "launchctl submit worked from the jail: {out}"
    );
    assert_ne!(listed.code, Some(0), "launchd holds the agent's job");
}

#[test]
fn the_jailed_agent_leads_its_session_under_the_backstop() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let sysctl = |name| {
        let out = run_outside("/usr/sbin/sysctl", &["-n", name], Path::new("/"));
        out.assert_ok().stdout.trim().parse::<u64>().unwrap()
    };
    let per_user = sysctl("kern.maxprocperuid");
    let max_proc = sysctl("kern.maxproc");
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
    let limit = per_user.min(max_proc - 512) - 512;
    assert_eq!(line, format!("{limit} {limit}"));
    drop(handle);
}
