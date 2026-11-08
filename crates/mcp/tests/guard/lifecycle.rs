//! The guard's own Chrome and agent-browser's detached daemon, both started by the first tool
//! call: whichever way the guard ends (stdin's end, a SIGKILL of the guard, a SIGKILL of its
//! parent, Chrome's second exit, a refusal), no process of the session is left, and Chrome's
//! profile is gone. Lazy start, the idle stop and the crash restart are in `lazy.rs`. Also
//! Chrome's start checks and the download mover. These tests poll, at most [`GONE_WAIT`], for
//! processes to end and files to move.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use crate::support::{Guard, Scratch, example, guard_command, profiles, run_files};
use crate::write;

/// How long a test waits for a process to end or a file to appear.
pub(crate) const GONE_WAIT: Duration = Duration::from_secs(10);

/// Whether `ready` holds within [`GONE_WAIT`], looking every 50 ms.
#[expect(
    clippy::disallowed_methods,
    reason = "the lifecycle tests wait for other processes to end, which no read reports"
)]
pub(crate) fn within(mut ready: impl FnMut() -> bool) -> bool {
    for _ in 0..GONE_WAIT.as_millis() / 50 {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    ready()
}

/// Whether `ok` holds every 50 ms for `time`; `false` at the first look it does not.
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "the tests check that another process leaves a file or a process alone for a while"
)]
pub(crate) fn throughout(time: Duration, mut ok: impl FnMut() -> bool) -> bool {
    for _ in 0..time.as_millis() / 50 {
        if !ok() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    ok()
}

/// The pid in `file`, once it holds one.
#[cfg(unix)]
pub(crate) fn pid_in(file: &Path) -> i32 {
    let mut pid = None;
    assert!(
        within(|| {
            pid = std::fs::read_to_string(file)
                .ok()
                .and_then(|text| text.trim().parse().ok());
            pid.is_some()
        }),
        "no pid in {}",
        file.display()
    );
    pid.unwrap_or_default()
}

/// Whether process `pid` runs: it exists and, on Linux, is no zombie.
#[cfg(unix)]
pub(crate) fn alive(pid: i32) -> bool {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    if kill(Pid::from_raw(pid), None).is_err() {
        return false;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let state = stat.rsplit_once(") ").map(|(_, rest)| rest.chars().next());
    !matches!(state, Some(Some('Z')))
}

#[cfg(unix)]
pub(crate) fn sigkill(pid: i32) {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    kill(Pid::from_raw(pid), Signal::SIGKILL).unwrap();
}

/// The daemon stub's and the Chrome stand-in's pids, once both run.
#[cfg(unix)]
pub(crate) fn session_pids(scratch: &Scratch) -> [i32; 2] {
    let mut daemon = None;
    let found = within(|| {
        daemon = crate::support::daemon_pid_file(&scratch.socket_dir());
        daemon.is_some()
    });
    assert!(found, "no daemon pid file");
    let daemon = daemon.unwrap_or_default();
    [pid_in(&daemon), pid_in(&scratch.path().join("chrome.pid"))]
}

/// Neither process is left, and Chrome's profile is gone.
#[cfg(unix)]
pub(crate) fn assert_nothing_left(scratch: &Scratch, pids: [i32; 2]) {
    let [daemon, chrome] = pids;
    assert!(within(|| !alive(daemon)), "the daemon {daemon} is left");
    assert!(within(|| !alive(chrome)), "Chrome {chrome} is left");
    let left = || run_files(&scratch.cwd());
    assert!(within(|| left().is_empty()), "left: {:?}", left());
}

pub(crate) fn no_args() -> [&'static OsStr; 0] {
    []
}

#[cfg(unix)]
#[test]
fn the_end_of_stdin_ends_the_daemon_and_chrome() {
    let scratch = Scratch::new("life_eof");
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &no_args()));
    guard.call("open");
    let pids = session_pids(&scratch);
    assert_eq!(profiles(&scratch.cwd()).len(), 1);
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_nothing_left(&scratch, pids);
}

/// agent-browser's stand-in leaves a process in its group (a CLI call of its, which could start
/// a daemon later): the reaper ends that too.
#[cfg(unix)]
#[test]
fn a_killed_guard_leaves_nothing_behind() {
    let scratch = Scratch::new("life_kill");
    let linger = [OsStr::new("linger")];
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &linger));
    guard.call("open");
    let pids = session_pids(&scratch);
    let lingering = pid_in(&scratch.socket_dir().join("linger.pid"));
    sigkill(i32::try_from(guard.pid()).unwrap());
    // The reaper sees the guard's pipe end and cleans up.
    assert_nothing_left(&scratch, pids);
    assert!(
        within(|| !alive(lingering)),
        "agent-browser's {lingering} is left"
    );
    drop(guard);
}

/// The same when agent-browser exits by itself at the end of its stdin: the guard kills what is
/// left in its process group before the daemon, so no CLI call outlives the cleanup.
#[cfg(unix)]
#[test]
fn what_agent_browser_leaves_in_its_group_is_ended() {
    let scratch = Scratch::new("life_linger");
    let linger = [OsStr::new("linger")];
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &linger));
    guard.call("open");
    let pids = session_pids(&scratch);
    let lingering = pid_in(&scratch.socket_dir().join("linger.pid"));
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_nothing_left(&scratch, pids);
    assert!(
        within(|| !alive(lingering)),
        "agent-browser's {lingering} is left"
    );
}

/// The test holds the guard's stdin itself, so the stdin's end cannot be what stops the guard:
/// only its parent check can.
#[cfg(unix)]
#[test]
fn a_killed_parent_leaves_nothing_behind() {
    use crate::support::{GUARD, guard_args, host_env};
    let scratch = Scratch::new("life_parent");
    let guard_pid_file = scratch.path().join("guard.pid");
    let mut command = std::process::Command::new(example("parent_stub"));
    command
        .arg(GUARD)
        .args(guard_args(&scratch, &example("daemon_stub"), &no_args()))
        .envs(host_env(&scratch))
        .env("PARENT_STUB_CHILD_PID", &guard_pid_file);
    let mut parent = Guard::start(command);
    parent.call("open");
    let guard = pid_in(&guard_pid_file);
    let pids = session_pids(&scratch);
    sigkill(i32::try_from(parent.pid()).unwrap());
    assert!(within(|| !alive(guard)), "the guard {guard} is left");
    assert_nothing_left(&scratch, pids);
    drop(parent);
}

/// Chrome's second death ends the guard (its first is in `lazy.rs`).
#[cfg(unix)]
#[test]
fn chrome_that_dies_twice_ends_agent_browser_and_the_guard() {
    let scratch = Scratch::new("life_chrome");
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &no_args()));
    guard.call("open");
    let first = session_pids(&scratch);
    sigkill(first[1]);
    assert!(within(|| !alive(first[0])), "the first daemon is left");
    guard.call("again");
    let mut pids = [0, 0];
    assert!(within(|| {
        pids = session_pids(&scratch);
        pids != first && alive(pids[1])
    }));
    sigkill(pids[1]);
    assert_eq!(guard.wait().code(), Some(1));
    let stderr = guard.stderr_text();
    assert!(
        stderr.contains("clauseo-mcp browser-guard: Chrome exited; the guard ended agent-browser"),
        "{stderr}"
    );
    assert_nothing_left(&scratch, pids);
}

/// The first call's start fails: the call gets an error and the guard exits 1.
fn start_fails(scratch: &Scratch) -> String {
    let log = scratch.log();
    let mut guard = Guard::start(guard_command(
        scratch,
        &example("echo_stub"),
        &[log.as_os_str()],
    ));
    let answer = guard.call("open");
    assert_eq!(answer["error"]["code"], -32603, "{answer}");
    let (status, rest) = guard.finish_with_stderr();
    assert_eq!(status.code(), Some(1));
    assert_eq!(rest.0, "");
    rest.1
}

#[test]
fn chrome_on_another_port_is_refused_before_agent_browser_starts() {
    let scratch = Scratch::new("life_port");
    scratch.chrome_mode("wrong-port");
    let stderr = start_fails(&scratch);
    assert_eq!(
        stderr.trim_end().lines().last(),
        Some(
            "clauseo-mcp browser-guard: Chrome listens on debugging port 47120, not the claimed \
             port 61101"
        )
    );
    assert!(!scratch.log().exists(), "agent-browser never started");
    assert_eq!(run_files(&scratch.cwd()), Vec::<std::path::PathBuf>::new());
}

#[test]
fn chrome_that_exits_at_its_start_is_refused() {
    let scratch = Scratch::new("life_exit");
    scratch.chrome_mode("exit");
    let stderr = start_fails(&scratch);
    assert!(
        stderr.lines().last().is_some_and(|line| line.starts_with(
            "clauseo-mcp browser-guard: Chrome stopped before it listened on the claimed \
             debugging port"
        )),
        "{stderr}"
    );
    assert!(!scratch.log().exists(), "agent-browser never started");
}

/// The mover at the guard's level: files Chrome finished in the staging folder reach the
/// downloads folder, a protected name renamed in any case; an unfinished one stays (and goes
/// with the profile).
#[test]
fn downloads_with_protected_names_are_renamed() {
    let scratch = Scratch::new("life_downloads");
    let mut guard = Guard::start(guard_command(&scratch, &example("daemon_stub"), &no_args()));
    guard.call("open");
    assert!(within(|| profiles(&scratch.cwd()).len() == 1), "no profile");
    let staging = profiles(&scratch.cwd())[0].join("downloads-staging");
    assert!(within(|| staging.is_dir()), "no staging folder");
    // Each protected name once, in its own case: on Windows and macOS `CLAUDE.md` and `claude.MD`
    // are one file, so two spellings of one name would leave four files, not five.
    let names = ["CLAUDE.md", ".MCP.json", "agents.MD", "report.pdf"];
    write::write(staging.join("big.zip.crdownload"), "half").unwrap();
    for name in names {
        write::write(staging.join(name), name).unwrap();
    }
    let downloads = scratch.cwd().join("downloads");
    let listed = |dir: &Path| -> BTreeSet<String> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let expected: BTreeSet<String> = [
        "CLAUDE (download).md",
        ".MCP (download).json",
        "agents (download).MD",
        "report.pdf",
    ]
    .map(str::to_owned)
    .into();
    // On Windows and macOS the mover waits up to 30 s for Chrome's download mark, which these
    // files never get; the sweep at the guard's exit, which waits for no mark, moves them.
    let marks_wait = cfg!(any(windows, target_os = "macos"));
    if !marks_wait {
        assert!(
            within(|| listed(&downloads) == expected),
            "{:?}",
            listed(&downloads)
        );
        assert_eq!(
            listed(&staging),
            BTreeSet::from(["big.zip.crdownload".to_owned()])
        );
    }
    let (status, _) = guard.finish();
    assert_eq!(status.code(), Some(0));
    assert_eq!(listed(&downloads), expected);
    assert_eq!(
        std::fs::read_to_string(downloads.join("CLAUDE (download).md")).unwrap(),
        "CLAUDE.md"
    );
}

/// The guard's list is the sandbox's `PROTECTED_NAMES`, read from both sources.
#[test]
fn the_download_guard_protects_the_sandboxs_names() {
    fn names(source: &str) -> Vec<String> {
        let start = source
            .find("PROTECTED_NAMES: [&str; 8] = [")
            .expect("the list");
        let block = &source[start..];
        let block = &block[..block.find("];").expect("its end")];
        block
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }
    let sandbox = names(include_str!("../../../sandbox/src/policy.rs"));
    let guard = names(include_str!("../../src/browser_guard/downloads.rs"));
    assert_eq!(sandbox.len(), 8);
    assert_eq!(guard, sandbox);
}
