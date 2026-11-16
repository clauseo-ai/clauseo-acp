//! `TreeWatch` (Windows): a killed tree reads as gone only once each of its processes has
//! exited, not as soon as `TerminateJobObject` has set the job's count to 0 (each process is
//! still ending then, and holds its handles: its working folder, its sockets). The helpers carry
//! `#[cfg(test)]` so clippy treats them as test code.

#![cfg(windows)]

use std::collections::BTreeMap;
use std::pin::pin;
use std::time::Duration;

use async_io::{Timer, block_on};
use clauseo_process::{LaunchSpec, Launched, SpawnOptions, TreeWatch, running, spawn_in_tree};
use futures::future::{Either, select};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};

/// How often the helpers look again, and how many looks make the install's 10 s bound.
#[cfg(test)]
const POLL: Duration = Duration::from_millis(20);
#[cfg(test)]
const POLLS: u32 = 500;

/// Whether `watch` reads gone within the bound.
#[cfg(test)]
fn gone_within_bound(watch: &TreeWatch) -> bool {
    for _ in 0..POLLS {
        if watch.gone() {
            return true;
        }
        block_on(Timer::after(POLL));
    }
    watch.gone()
}

/// `cmd` starting a second `cmd` that reads commands from the tree's stdin; returned once the
/// second has answered one, so both run.
#[cfg(test)]
fn tree() -> Launched {
    let cmd = std::env::var_os("ComSpec").expect("ComSpec");
    let spec = LaunchSpec::new(
        cmd.into(),
        vec!["/d".into(), "/c".into(), "cmd /d /k".into()],
        BTreeMap::new(),
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap();
    let mut launched = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
    block_on(async {
        launched.stdin.write_all(b"echo started\r\n").await.unwrap();
        launched.stdin.flush().await.unwrap();
        let mut seen = Vec::new();
        let read = pin!(async {
            let mut chunk = [0_u8; 1024];
            while !String::from_utf8_lossy(&seen).contains("started") {
                let n = launched.stdout.read(&mut chunk).await.unwrap();
                assert!(n > 0, "cmd ended: {}", String::from_utf8_lossy(&seen));
                seen.extend_from_slice(&chunk[..n]);
            }
        });
        let limit = pin!(Timer::after(Duration::from_secs(30)));
        if let Either::Right(_) = select(read, limit).await {
            panic!("the inner cmd did not answer");
        }
    });
    launched
}

/// Once the watch of a killed tree says gone, no process of the tree runs: the kill's
/// `TerminateJobObject` alone does not make it gone.
#[test]
fn a_killed_tree_reads_gone_only_once_each_of_its_processes_has_exited() {
    let launched = tree();
    let watch = launched.handle.tree_watch();
    assert!(!watch.gone(), "the tree runs");
    let mut pids = launched
        .handle
        .descendants()
        .snapshot()
        .pids()
        .expect("the job's processes");
    pids.push(launched.handle.pid());
    assert!(pids.len() >= 2, "both cmds: {pids:?}");
    // The handle's drop sends `TerminateJobObject` and does not wait.
    drop(launched);
    assert!(gone_within_bound(&watch), "the tree did not end");
    for pid in pids {
        assert!(!running(pid), "{pid} still ran once the watch said gone");
    }
}
