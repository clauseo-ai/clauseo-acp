//! `TreeWatch` (Unix): the agent's group reads as gone only once no process is left in it, a
//! zombie included, and stays readable after the handle has dropped. The helpers carry
//! `#[cfg(test)]` so clippy treats them as test code.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::time::Duration;

use async_io::{Timer, block_on};
use clauseo_process::{LaunchSpec, Launched, SpawnOptions, TreeWatch, spawn_in_tree};

/// How often the helpers look again.
#[cfg(test)]
const POLL: Duration = Duration::from_millis(20);

/// Starts `sh -c script` in a tree of its own.
#[cfg(test)]
fn launch(script: &str) -> Launched {
    let spec = LaunchSpec::new(
        "/bin/sh".into(),
        vec!["-c".into(), script.into()],
        BTreeMap::new(),
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap();
    spawn_in_tree(&spec, &SpawnOptions::new()).unwrap()
}

/// Whether `watch` reads gone within about `polls` polls.
#[cfg(test)]
fn gone_within(watch: &TreeWatch, polls: u32) -> bool {
    for _ in 0..polls {
        if watch.gone() {
            return true;
        }
        block_on(Timer::after(POLL));
    }
    watch.gone()
}

/// A helper left in the group is killed with it once the agent exits; the watch reads gone
/// after that, though the handle is gone.
#[test]
fn the_group_reads_gone_once_the_agent_and_its_helper_have_gone() {
    let Launched { mut handle, .. } = launch("sleep 30 & sleep 0.3");
    let watch = handle.tree_watch();
    assert!(!watch.gone(), "the agent runs");
    block_on(handle.exited());
    drop(handle);
    assert!(gone_within(&watch, 500));
}

/// A process of the group that is killed but not yet reaped keeps the group: the watch reads
/// gone only once it is reaped. This test becomes the orphans' reaper (Linux's child
/// subreaper) and leaves the helper unreaped until it reaps it by pid.
#[cfg(target_os = "linux")]
#[test]
fn an_unreaped_process_of_the_group_keeps_it() {
    use futures::AsyncReadExt as _;
    use nix::sys::wait::waitpid;
    use nix::unistd::Pid;

    nix::sys::prctl::set_child_subreaper(true).unwrap();
    let Launched {
        mut handle,
        mut stdout,
        ..
    } = launch("sleep 30 & echo $!; sleep 0.5");
    let watch = handle.tree_watch();
    block_on(handle.exited());
    drop(handle);
    let mut out = String::new();
    block_on(stdout.read_to_string(&mut out)).unwrap();
    let helper = Pid::from_raw(out.trim().parse().unwrap());
    // The tree's kill reached the helper, now this test's orphan, which it has not reaped.
    assert!(!gone_within(&watch, 25));
    waitpid(helper, None).unwrap();
    assert!(gone_within(&watch, 500));
}
