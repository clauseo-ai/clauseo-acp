//! The handle's shutdown ends the whole tree of a jailed agent: a shell that starts children.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use clauseo_process::{LaunchSpec, Launched, Launcher, ManualTimer, ShutdownOutcome};
use futures::StreamExt;
use futures::io::{AsyncBufReadExt, BufReader};
use nix::sys::event::{EvFlags, EventFilter, FilterFlag, KEvent, Kqueue};

use crate::support::{World, launcher};

/// An exit watch on `pid`, registered while it runs (kqueue `NOTE_EXIT`), as
/// `crates/process/tests/support/exit_watch.rs` does.
fn exit_event(pid: u32, flags: EvFlags) -> KEvent {
    KEvent::new(
        usize::try_from(pid).unwrap(),
        EventFilter::EVFILT_PROC,
        flags,
        FilterFlag::NOTE_EXIT,
        0,
        0,
    )
}

#[test]
fn shutdown_ends_the_jailed_tree() {
    let w = World::new();
    let (launcher, _) = launcher(w.builder().build().unwrap());
    // The shell ignores stdin closing, so the ladder must signal the group; each child is a
    // process of its own in the jail.
    let script = "/bin/sleep 600 & echo $!; /bin/sleep 600 & echo $!; echo started; wait";
    let args = vec![OsString::from("-c"), OsString::from(script)];
    let spec =
        LaunchSpec::new(PathBuf::from("/bin/sh"), args, BTreeMap::new(), w.at("m1")).unwrap();
    let Launched {
        stdout,
        stderr,
        handle,
        ..
    } = launcher.launch(&spec).unwrap();
    let shell = handle.pid();
    let mut lines = BufReader::new(stdout).lines();
    let children: Vec<u32> = async_io::block_on(async {
        let mut pids = Vec::new();
        while let Some(line) = lines.next().await {
            let line = line.unwrap();
            if line == "started" {
                break;
            }
            pids.push(line.parse().unwrap());
        }
        pids
    });
    assert_eq!(children.len(), 2);
    let queue = Kqueue::new().unwrap();
    for pid in children.iter().copied().chain([shell]) {
        let add = exit_event(pid, EvFlags::EV_ADD | EvFlags::EV_ONESHOT);
        queue
            .kevent(&[add], &mut [], None)
            .expect("the process is running");
    }
    let timer = ManualTimer::new();
    let (outcome, pumped, ()) = async_io::block_on(async {
        futures::join!(handle.shutdown(&timer, None), stderr.drain(), async {
            // Stdin closed and nothing exited: the 2 s wait ends and SIGTERM goes to the group.
            timer.next_armed().await;
            assert!(timer.fire());
        })
    });
    pumped.unwrap();
    assert!(
        matches!(outcome, ShutdownOutcome::Terminated(_)),
        "{outcome:?}"
    );
    // Every process of the tree exits; a survivor blocks here until nextest's timeout.
    let mut seen = Vec::new();
    while seen.len() < 3 {
        let mut events = [exit_event(0, EvFlags::empty())];
        let n = queue.kevent(&[], &mut events, None).unwrap();
        assert_eq!(n, 1);
        seen.push(u32::try_from(events[0].ident()).unwrap());
    }
    seen.sort_unstable();
    let mut all: Vec<u32> = children.iter().copied().chain([shell]).collect();
    all.sort_unstable();
    assert_eq!(seen, all);
}
