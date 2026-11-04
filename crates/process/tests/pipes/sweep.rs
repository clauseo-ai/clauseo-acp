//! The descendant listing and the sweep, against `fake_agent`'s `children-on-requests` (a child
//! in a group of its own at start, a second once `initialize` comes, a third once `session/new`
//! comes), and the stop's kill of a grandchild that left the agent's group
//! (`grandchild-own-group`).
//!
//! A child the sweep kills stays its living parent's unreaped zombie on Unix, so no exit event
//! comes for it: [`gone_within`] asks the OS until the process no longer runs, under a deadline,
//! and [`cutoff_after`] waits until a cutoff would cover a child (the clock's unit is 10 ms on
//! Linux) the same way.

use std::path::PathBuf;
use std::pin::pin;
use std::time::Duration;

use async_io::{Timer, block_on};
use clauseo_process::{
    Cutoff, Descendants, Launched, ManualTimer, ShutdownOutcome, Snapshot, Swept, kill, running,
    started_before_now,
};
use futures::future::{Either, select};
use futures::{AsyncWriteExt, join};
use serde_json::json;

use crate::exit_watch::{self, Watch};
use crate::harness::{RunDir, launch, lines};

/// How long a process the sweep ended may take to stop running, and the most a test waits for
/// a cutoff to cover a child.
const GONE_WITHIN: Duration = Duration::from_secs(5);

/// The `initialize` request that moves `children-on-requests` on to its second child.
const INITIALIZE: &[u8] =
    b"{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\",\"params\":{\"protocolVersion\":1}}\n";

/// Whether `pid` stops running within [`GONE_WITHIN`] of real time, asked every 10 ms.
async fn gone_within(pid: u32) -> bool {
    let checks = GONE_WITHIN.as_millis() / 10;
    for _ in 0..checks {
        if !running(pid) {
            return true;
        }
        Timer::after(Duration::from_millis(10)).await;
    }
    !running(pid)
}

/// A cutoff taken once one covers `pid` (its start is before it), asked every 10 ms for at most
/// [`GONE_WITHIN`].
async fn cutoff_after(pid: u32) -> Cutoff {
    for _ in 0..GONE_WITHIN.as_millis() / 10 {
        if started_before_now(pid) {
            break;
        }
        Timer::after(Duration::from_millis(10)).await;
    }
    assert!(started_before_now(pid), "no cutoff covers child {pid}");
    Cutoff::now().unwrap()
}

/// Kills a child the test read, if a failed assert (or the end of a test) leaves it running.
struct Reap(Vec<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        for &pid in &self.0 {
            if running(pid) {
                kill(pid);
            }
        }
    }
}

/// The agent's conhost on Windows, and what a test asks about a process there, through
/// PowerShell's `Get-CimInstance`.
#[cfg(windows)]
mod console {
    use std::ffi::OsStr;
    use std::fmt;
    use std::path::{Path, PathBuf};

    /// The pid of the agent's conhost in a snapshot `listed` that holds the child `first`. The
    /// agent starts with a console that has no window, so Windows starts a `conhost.exe` for it
    /// in the job: the snapshot is `first` and at most one more process, which must run
    /// `%SystemRoot%\System32\conhost.exe` with the agent `agent` as its parent. A failure
    /// names every process besides `first`, with its image, parent and creation date.
    pub(super) async fn of_agent(listed: Vec<u32>, first: u32, agent: u32) -> Option<u32> {
        assert!(
            listed.contains(&first),
            "{listed:?} misses the child {first}"
        );
        let mut others = Vec::new();
        for pid in listed.into_iter().filter(|&pid| pid != first) {
            others.push(Info::of(pid).await);
        }
        let conhost = conhost();
        match others.as_slice() {
            [] => None,
            [only] if only.is_conhost_of(&conhost, agent) => Some(only.pid),
            _ => panic!(
                "besides the child {first}, the snapshot holds more than {conhost:?} with parent \
                 {agent}: {}",
                others
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }
    }

    /// A process as CIM reports it; each field is empty when CIM did not say.
    struct Info {
        pid: u32,
        image: String,
        parent: String,
        created: String,
    }

    impl Info {
        /// Process `pid`'s image, parent pid and creation date.
        async fn of(pid: u32) -> Info {
            let script = format!(
                "$p = Get-CimInstance Win32_Process -Filter 'ProcessId={pid}'; \
                 if ($p) {{ $p.ExecutablePath; $p.ParentProcessId; \
                 $p.CreationDate.ToString('o') }}"
            );
            let output = async_process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .output()
                .await
                .expect("run powershell");
            let text = String::from_utf8_lossy(&output.stdout);
            let mut lines = text.lines().map(|line| line.trim().to_owned());
            let mut next = || lines.next().unwrap_or_default();
            Info {
                pid,
                image: next(),
                parent: next(),
                created: next(),
            }
        }

        /// Whether it runs `conhost` (the full path, ASCII case aside) with `agent` as parent.
        fn is_conhost_of(&self, conhost: &Path, agent: u32) -> bool {
            OsStr::new(&self.image).eq_ignore_ascii_case(conhost.as_os_str())
                && self
                    .parent
                    .parse::<u32>()
                    .is_ok_and(|parent| parent == agent)
        }
    }

    impl fmt::Display for Info {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Info {
                pid,
                image,
                parent,
                created,
            } = self;
            write!(
                f,
                "pid {pid}: image {image:?}, parent {parent:?}, created {created:?}"
            )
        }
    }

    /// `%SystemRoot%\System32\conhost.exe`.
    fn conhost() -> PathBuf {
        let root = std::env::var_os("SystemRoot").expect("SystemRoot is set");
        PathBuf::from(root).join("System32").join("conhost.exe")
    }
}

/// Runs the sweep, firing its 2 s wait when it arms one (a sweep that found something new does).
async fn sweep(
    descendants: &Descendants,
    before: &Snapshot,
    cutoff: Cutoff,
    keep: &[PathBuf],
) -> Swept {
    let timer = ManualTimer::new();
    let swept = pin!(descendants.end_new(before, cutoff, keep, &timer));
    let fire = pin!(async {
        assert_eq!(timer.next_armed().await, Duration::from_secs(2));
        assert!(timer.fire());
    });
    match select(swept, fire).await {
        Either::Left((swept, _fire)) => swept,
        Either::Right(((), swept)) => swept.await,
    }
}

#[test]
fn the_sweep_ends_only_what_is_new() {
    let dir = RunDir::new();
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        handle,
        ..
    } = launch("children-on-requests", &dir).unwrap();
    let descendants = handle.descendants();
    let (pumped, mut lines) = lines(stderr);
    let script = async {
        let first = lines.grandchild().await;
        let mut reap = Reap(vec![first]);
        let before = descendants.snapshot();
        let listed = before.pids().expect("the snapshot listed");
        #[cfg(unix)]
        let console: Option<u32> = {
            assert_eq!(listed, vec![first]);
            None
        };
        #[cfg(windows)]
        let console = console::of_agent(listed, first, handle.pid()).await;
        stdin.write_all(INITIALIZE).await.unwrap();
        let second = lines.grandchild().await;
        reap.0.push(second);
        let cutoff = cutoff_after(second).await;
        let swept = sweep(&descendants, &before, cutoff, &[]).await;
        assert_eq!(swept, Swept::Ended { processes: 1 });
        assert!(gone_within(second).await, "the new child still runs");
        assert!(
            running(first),
            "the child from before the snapshot was ended"
        );
        if let Some(console) = console {
            assert!(running(console), "the agent's conhost {console} was ended");
            let after = descendants
                .snapshot()
                .pids()
                .expect("the listing after the sweep");
            assert!(
                after.contains(&console),
                "{after:?} lost the conhost {console}"
            );
        }
        // Nothing new is left, so a second sweep ends nothing.
        let again = sweep(&descendants, &before, cutoff, &[]).await;
        assert_eq!(again, Swept::NothingNew);
        drop(handle);
        drop(reap);
    };
    let (pumped, ()) = block_on(async { join!(pumped, script) });
    pumped.unwrap();
}

#[test]
fn the_sweep_spares_what_started_after_its_cutoff() {
    let dir = RunDir::new();
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        handle,
        ..
    } = launch("children-on-requests", &dir).unwrap();
    let descendants = handle.descendants();
    let (pumped, mut lines) = lines(stderr);
    let script = async {
        let first = lines.grandchild().await;
        let mut reap = Reap(vec![first]);
        let before = descendants.snapshot();
        stdin.write_all(INITIALIZE).await.unwrap();
        let cancelled = lines.grandchild().await;
        reap.0.push(cancelled);
        // The cancelled turn ends here; what starts next belongs to the next turn.
        let cutoff = cutoff_after(cancelled).await;
        let new = json!({"jsonrpc": "2.0", "id": 1, "method": "session/new",
            "params": {"cwd": dir.path(), "mcpServers": []}});
        stdin
            .write_all(format!("{new}\n").as_bytes())
            .await
            .unwrap();
        let next = lines.grandchild().await;
        reap.0.push(next);
        // Both passes run before the sweep returns.
        let swept = sweep(&descendants, &before, cutoff, &[]).await;
        assert_eq!(swept, Swept::Ended { processes: 1 });
        assert!(
            gone_within(cancelled).await,
            "the cancelled child still runs"
        );
        assert!(running(next), "the next turn's child was ended");
        drop(handle);
        drop(reap);
    };
    let (pumped, ()) = block_on(async { join!(pumped, script) });
    pumped.unwrap();
}

#[test]
fn the_sweep_keeps_a_kept_executable() {
    let dir = RunDir::new();
    let Launched {
        mut stdin,
        stdout: _stdout,
        stderr,
        handle,
        ..
    } = launch("children-on-requests", &dir).unwrap();
    let descendants = handle.descendants();
    let (pumped, mut lines) = lines(stderr);
    let script = async {
        let first = lines.grandchild().await;
        let mut reap = Reap(vec![first]);
        let before = descendants.snapshot();
        stdin.write_all(INITIALIZE).await.unwrap();
        let second = lines.grandchild().await;
        reap.0.push(second);
        let cutoff = cutoff_after(second).await;
        // The children are `fake_agent --hold`: keeping fake_agent's executable keeps them.
        let fake = clauseo_fake_agent::bin_path().unwrap();
        let swept = sweep(&descendants, &before, cutoff, &[fake]).await;
        assert_eq!(swept, Swept::NothingNew);
        assert!(running(second), "the kept child was ended");
        drop(handle);
        drop(reap);
    };
    let (pumped, ()) = block_on(async { join!(pumped, script) });
    pumped.unwrap();
}

#[test]
fn own_group_grandchild_dies_with_its_shutdown() {
    exit_watch::before_launch();
    let dir = RunDir::new();
    let Launched {
        stdout: _stdout,
        stderr,
        handle,
        ..
    } = launch("grandchild-own-group", &dir).unwrap();
    let timer = ManualTimer::new();
    let (pumped, mut lines) = lines(stderr);
    let script = async {
        let grandchild = Watch::new(lines.grandchild().await);
        // The agent holds when stdin closes; the 2 s wait moves the ladder on, and on Unix the
        // agent dies at the SIGTERM to its group, which the grandchild is not in.
        let fire = async {
            assert_eq!(timer.next_armed().await, Duration::from_secs(2));
            assert!(timer.fire());
        };
        // Listed here, after the grandchild started, as the host lists just before a stop.
        let listed = handle.descendants().snapshot();
        let (outcome, ()) = join!(handle.shutdown(&timer, Some(listed)), fire);
        (outcome, grandchild)
    };
    let ((outcome, grandchild), pumped) = block_on(async { join!(script, pumped) });
    pumped.unwrap();
    #[cfg(unix)]
    assert!(
        matches!(outcome, ShutdownOutcome::Terminated(_)),
        "{outcome:?}"
    );
    #[cfg(windows)]
    assert!(matches!(outcome, ShutdownOutcome::Killed(_)), "{outcome:?}");
    crate::tree::wait_for_the_tree_kill(grandchild);
}
