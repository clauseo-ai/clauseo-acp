//! [`SpawnOptions::own_session`] and [`SpawnOptions::max_processes`] on macOS, seen from inside
//! the child: it leads its own session and its own group, and its `RLIMIT_NPROC` is the one
//! asked, soft and hard.
#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use async_io::block_on;
use clauseo_process::{LaunchSpec, SpawnOptions, spawn_in_tree};
use futures::io::AsyncReadExt as _;

/// Prints the shell's pid, group and state (`s`: a session leader), and its limits.
const PROBE: &str = r#"echo "$$ $(/bin/ps -o pgid= -o stat= -p $$) $(ulimit -u) $(ulimit -Hu)""#;

/// What the probe printed when started with `opts`.
#[cfg(test)]
fn probe(opts: &SpawnOptions) -> Vec<String> {
    let spec = LaunchSpec::new(
        PathBuf::from("/bin/sh"),
        vec!["-c".into(), PROBE.into()],
        BTreeMap::new(),
        PathBuf::from("/"),
    )
    .unwrap();
    let mut launched = spawn_in_tree(&spec, opts).unwrap();
    let mut out = String::new();
    block_on(async {
        let (read, drained, report) = futures::join!(
            launched.stdout.read_to_string(&mut out),
            launched.stderr.drain(),
            launched.handle.exited()
        );
        read.unwrap();
        drained.unwrap();
        assert_eq!(report.code(), Some(0), "{out}");
    });
    out.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn the_child_leads_its_own_session_under_the_limit() {
    let seen = probe(&SpawnOptions::new().own_session().max_processes(3_000));
    let [pid, group, state, soft, hard] = seen.as_slice() else {
        panic!("{seen:?}");
    };
    assert_eq!(pid, group, "{seen:?}");
    assert!(state.contains('s'), "not a session leader: {seen:?}");
    assert_eq!((soft.as_str(), hard.as_str()), ("3000", "3000"), "{seen:?}");
}

#[test]
fn without_the_options_the_child_leads_a_group_only() {
    let seen = probe(&SpawnOptions::new());
    let [pid, group, state, ..] = seen.as_slice() else {
        panic!("{seen:?}");
    };
    assert_eq!(pid, group, "{seen:?}");
    assert!(!state.contains('s'), "a session leader: {seen:?}");
}
