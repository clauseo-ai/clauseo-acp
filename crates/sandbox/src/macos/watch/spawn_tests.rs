//! The watcher's starts (`log stream` and the probe) take `clauseo-process`'s spawn lock: a
//! child they start while Chrome's `DevTools` pipes are being made
//! ([`clauseo_process::cloexec_pipe`]) holds none of them.

use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_process::Command;
use futures::io::AsyncReadExt;

use super::spawn;

/// Prints each descriptor from 3 to 63 open in the shell; `test -e` on `/dev/fd` opens none.
const OPEN_FDS: &str =
    r#"for n in $(seq 3 63); do if [ -e "/dev/fd/$n" ]; then echo "$n"; fi; done"#;

/// The descriptors a shell started through the watcher's [`spawn`] holds.
fn open() -> Vec<String> {
    let mut child = spawn(
        Command::new("/bin/sh")
            .args(["-c", OPEN_FDS])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    )
    .unwrap();
    let mut out = String::new();
    let mut stdout = child.stdout.take().unwrap();
    async_io::block_on(async {
        stdout.read_to_string(&mut out).await.unwrap();
        child.status().await.unwrap();
    });
    out.lines().map(str::to_owned).collect()
}

#[test]
fn a_watcher_start_holds_no_pipe_made_meanwhile() {
    // What this process already holds open across exec (a jobserver pipe cargo passed down)
    // reaches every child; it is not a pipe made here.
    let before = open();
    let done = Arc::new(AtomicBool::new(false));
    let making = {
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            let mut held = Vec::new();
            while !done.load(Ordering::SeqCst) {
                held.push(clauseo_process::cloexec_pipe().unwrap());
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
        "a watcher start held a pipe made meanwhile"
    );
}
