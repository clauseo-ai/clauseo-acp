//! The ladder's kill on Windows when the job will not end: the agent's own process is killed
//! instead. A crate test, since only here can the handle's tree be swapped for one whose kill
//! Windows refuses.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::pin::pin;
use std::time::Duration;

use async_io::{Timer as IoTimer, block_on};
use futures::channel::mpsc;
use futures::future::{Either, select};
use futures::{AsyncReadExt, StreamExt, join};

use crate::os::Tree;
use crate::timer::ManualTimer;
use crate::{LaunchSpec, Launched, ShutdownOutcome, SpawnOptions, spawn_in_tree};

use super::{KILL_GRACE, STDIN_GRACE};

/// How long the test waits for the agent to die after the kill before it calls the agent
/// unresponsive: a bound, so a kill that never comes fails the test instead of hanging it.
const DEATH_DEADLINE: Duration = Duration::from_secs(10);

/// When `TerminateJobObject` fails, the kill step kills the agent's own process (std's
/// `Child::kill`, exit code 1), so shutdown still ends `Killed`. Without that kill the agent
/// would hold on, and the ladder would end `Unresponsive`.
#[test]
fn the_agent_is_killed_when_its_job_will_not_end() {
    let bin = clauseo_fake_agent::bin_path().expect(
        "fake_agent binary; `-p clauseo-process` does not build it: run `cargo build -p clauseo-fake_agent` first",
    );
    let spec = LaunchSpec::new(
        bin,
        vec!["--scenario".into(), "ignore-stdin-close".into()],
        BTreeMap::new(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    )
    .unwrap();
    let Launched {
        mut stdout,
        stderr,
        mut handle,
        ..
    } = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
    // The real job stays open until the test ends: closing it would end the agent. If the test
    // fails, dropping it ends the agent then.
    let job = std::mem::replace(&mut handle.tree, Tree::that_will_not_end());
    assert!(
        !handle.tree.kill(),
        "Windows ended a job through a handle with no rights"
    );
    let (tx, mut lines) = mpsc::unbounded();
    let pumped = stderr.lines(move |line| drop(tx.unbounded_send(line.to_owned())));
    let timer = ManualTimer::new();
    let script = async {
        while let Some(line) = lines.next().await {
            if line == "fake_agent: stdin closed, holding" {
                break;
            }
        }
        assert_eq!(timer.next_armed().await, STDIN_GRACE);
        assert!(timer.fire());
        assert_eq!(timer.next_armed().await, KILL_GRACE);
        // Stdout closes when the agent dies; if it has not by the deadline, firing ends the
        // ladder as Unresponsive.
        let mut out = Vec::new();
        let closed = pin!(stdout.read_to_end(&mut out));
        if let Either::Right(_) = select(closed, IoTimer::after(DEATH_DEADLINE)).await {
            timer.fire();
        }
    };
    let (outcome, (), pumped) =
        block_on(async { join!(handle.shutdown(&timer, None), script, pumped) });
    let ShutdownOutcome::Killed(report) = outcome else {
        panic!("expected Killed, got {outcome:?}");
    };
    assert_eq!(report.code(), Some(1), "std's Child::kill gives 1");
    pumped.unwrap();
    drop(job);
}
