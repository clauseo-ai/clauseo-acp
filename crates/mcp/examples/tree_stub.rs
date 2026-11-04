//! A stand-in for agent-browser in the guard's Windows tree test (`tests/guard/tree.rs`). Usage:
//! `tree_stub`. It starts itself again as `tree_stub --grandchild` the way agent-browser starts
//! its daemon on Windows (`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`), with the guard's stderr
//! inherited, then prints one JSON line and waits to be killed. The grandchild only waits: while
//! it lives it holds the guard's stderr open, so a test that reads stderr to its end sees the end
//! only once the grandchild is gone. Elsewhere it prints the line and waits, so it builds on
//! every OS. Cargo builds it for the tests; it is never shipped.

use std::io::Write;

/// `DETACHED_PROCESS`.
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;
/// `CREATE_NEW_PROCESS_GROUP`.
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Blocks until the process is killed.
fn wait_forever() -> ! {
    loop {
        std::thread::park();
    }
}

#[cfg(windows)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test stub starts its own detached grandchild, as agent-browser starts its daemon"
)]
fn start_grandchild() -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    let child = Command::new(std::env::current_exe()?)
        .arg("--grandchild")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    // Its pid, for the test's last look.
    writeln!(std::io::stderr(), "tree_stub grandchild {}", child.id())
}

fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--grandchild") {
        wait_forever();
    }
    #[cfg(windows)]
    start_grandchild()?;
    let mut out = std::io::stdout().lock();
    writeln!(out, r#"{{"jsonrpc":"2.0","id":1,"result":{{}}}}"#)?;
    out.flush()?;
    drop(out);
    wait_forever();
}
