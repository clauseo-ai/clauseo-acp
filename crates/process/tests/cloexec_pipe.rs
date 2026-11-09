//! [`cloexec_pipe`] against starts on other threads: a pipe is made, then marked close-on-exec
//! in a second call on macOS, and a child forked in between would keep it. Both run under the
//! crate's spawn lock, so no child started here sees one. A race shows only when it hits, so
//! this runs many rounds; its own file keeps other tests' descriptors out of the children.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_io::block_on;
use clauseo_process::{LaunchSpec, SpawnOptions, cloexec_pipe, spawn_in_tree};

/// Prints each descriptor from 3 to 63 open in the child.
const OPEN: &str =
    r#"for n in $(seq 3 63); do if { : <&"$n"; } 2>/dev/null; then echo "$n"; fi; done"#;

#[test]
fn a_pipe_made_while_children_start_never_reaches_one() {
    let spec = LaunchSpec::new(
        PathBuf::from("/bin/sh"),
        vec!["-c".into(), OPEN.into()],
        BTreeMap::new(),
        PathBuf::from("/"),
    )
    .unwrap();
    let open = || {
        let mut launched = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
        let mut out = String::new();
        block_on(futures::AsyncReadExt::read_to_string(
            &mut launched.stdout,
            &mut out,
        ))
        .unwrap();
        block_on(launched.handle.exited());
        out.lines().map(str::to_owned).collect::<Vec<_>>()
    };
    // What this process already holds open across exec (a jobserver pipe cargo passed down)
    // reaches every child; it is not this test's.
    let before = open();
    let done = Arc::new(AtomicBool::new(false));
    let making = {
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            let mut held = Vec::new();
            while !done.load(Ordering::SeqCst) {
                held.push(cloexec_pipe().unwrap());
                if held.len() > 8 {
                    held.remove(0);
                }
            }
        })
    };
    let mut seen = Vec::new();
    for _ in 0..100 {
        seen.extend(open().into_iter().filter(|n| !before.contains(n)));
    }
    done.store(true, Ordering::SeqCst);
    making.join().unwrap();
    assert_eq!(
        seen,
        Vec::<String>::new(),
        "a child held a pipe made meanwhile"
    );
}
