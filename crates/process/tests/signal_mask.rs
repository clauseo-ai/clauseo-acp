//! The hazard that forbids blocking a signal anywhere in Clauseo around a spawn: std's `Command`,
//! which `spawn_in_tree` uses, keeps the spawning thread's signal mask for the child (fork/exec
//! and `posix_spawn` alike: "Inherit the signal mask from the parent rather than resetting it").
//! An agent started from a thread with SIGTERM blocked starts with SIGTERM blocked, so the
//! ladder's group SIGTERM would do nothing and only its SIGKILL would end it. So the desktop
//! catches its quit signals with handlers, which a child's `exec` resets, and blocks none
//! (`crates/desktop/src/quit_signals.rs`; its `tests/quit_on_signal.rs` checks the agents start
//! with the defaults). Linux only: it reads `/proc/<pid>/status`.

#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::ffi::OsString;

use clauseo_process::{LaunchSpec, Launched, SpawnOptions, spawn_in_tree};
use nix::sys::signal::{SigSet, Signal};

#[test]
fn a_child_spawned_from_a_thread_with_sigterm_blocked_starts_with_it_blocked() {
    let launched = std::thread::spawn(|| {
        let mut blocked = SigSet::empty();
        blocked.add(Signal::SIGTERM);
        blocked.thread_block().expect("block SIGTERM");
        let bin = clauseo_fake_agent::bin_path()
            .unwrap_or_else(|e| panic!("{e} (run cargo build -p clauseo-fake_agent)"));
        let spec = LaunchSpec::new(
            bin,
            vec![OsString::from("--hold")],
            BTreeMap::new(),
            std::env::temp_dir(),
        )
        .expect("the spec");
        spawn_in_tree(&spec, &SpawnOptions::new()).expect("spawn")
    })
    .join()
    .expect("the spawning thread");
    let Launched { handle, .. } = launched;
    // The `SigBlk` mask of `pid`, from `/proc/<pid>/status`.
    let blocked_mask = |pid: u32| {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("read status");
        let hex = status
            .lines()
            .find_map(|line| line.strip_prefix("SigBlk:"))
            .expect("a SigBlk line")
            .trim()
            .to_owned();
        u64::from_str_radix(&hex, 16).expect("a hex mask")
    };
    let sigterm = 1u64 << (Signal::SIGTERM as i32 - 1);
    // Inherited at the fork or spawn and kept by the exec, so it reads the same at any moment.
    assert_ne!(
        blocked_mask(handle.pid()) & sigterm,
        0,
        "the child no longer inherits the spawning thread's mask: \
         crates/desktop/src/quit_signals.rs's reasoning needs a look"
    );
    // Dropped: the handle kills the tree (SIGKILL, which no mask holds back).
    drop(handle);
}
