//! The VM rows of S15 PR7 (tier B's network), run on a copy of `clauseo-win-01` after set-up, in
//! the desktop session through `run2.sh` (`CreateProcessWithLogonW` fails from session 0):
//!
//! ```text
//! cargo build -p clauseo-sandbox --example s15_wfp_vm
//! copy s15_wfp_vm.exe C:\s15\bin\        (a folder the pool users can read and run)
//! icacls C:\s15\bin /grant *S-1-15-2-1:(OI)(CI)RX   (for the AppContainer row)
//! C:\s15\bin\s15_wfp_vm.exe status
//! C:\s15\bin\s15_wfp_vm.exe rows [--share \\host\guest] [--inbound <port>] [--smb-host <ip>]
//!     [--webdav-host <host>]
//! C:\s15\bin\s15_wfp_vm.exe listen 46150  (as a second Windows user), then `foreign check`
//! C:\s15\bin\s15_wfp_vm.exe foreign add    (elevated), `foreign check` (the desktop user),
//!     then `foreign remove` (elevated)
//! C:\s15\bin\s15_wfp_vm.exe ports  (the desktop user; `ports_rows.rs` has the IPv6 row)
//! ```
//!
//! `status` runs a launch's check for each pool user the machine record lists. `rows` (`rows.rs`)
//! takes the record's first two pool users as matters A and B, starts an S16 proxy for each and
//! some listeners, checks each launch, runs the probe (`probe.rs`) as each pool user under the
//! (B) token (`launch.rs`), and checks what the owner may do to WFP. Each row is one JSON line on
//! stdout with what was expected and what happened; the last line counts the passes.
//!
//! Not tested: a connection opened before a launch ended and used after (ALE classifies at
//! connect; the static permit doesn't end with a launch anyway), and link-local IPv6 (it needs
//! an interface index). Off Windows it says so.

#[cfg(windows)]
use std::error::Error;
use std::io::{self, Write};

#[cfg(windows)]
#[path = "s15_wfp_vm/derived.rs"]
mod derived;
#[cfg(windows)]
#[path = "s15_wfp_vm/launch.rs"]
mod launch;
#[cfg(windows)]
#[path = "s15_wfp_vm/ports_rows.rs"]
mod ports_rows;
#[cfg(windows)]
#[path = "s15_wfp_vm/probe.rs"]
mod probe;
#[cfg(windows)]
#[path = "s15_wfp_vm/rows.rs"]
mod rows;

/// Writes `line` and a newline to stdout.
fn say(line: &str) {
    let mut out = io::stdout();
    if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
        drop(writeln!(io::stderr(), "s15_wfp_vm: stdout is gone"));
    }
}

#[cfg(not(windows))]
fn main() {
    say("s15_wfp_vm runs on Windows only");
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (command, rest) = args
        .split_first()
        .ok_or("usage: status | rows | listen <port>")?;
    let exe = std::env::current_exe()?;
    match command.as_str() {
        // The agent's side and its runner give the probe's bits as their exit code.
        "probe" => {
            let code = probe::run(&exe, rest);
            if let Some(why) = derived::last_error() {
                drop(writeln!(
                    io::stderr(),
                    "probe: a derived child didn't start: {why}"
                ));
            }
            std::process::exit(as_exit(code))
        }
        "runner" => {
            let code = launch::runner(&exe, rest).unwrap_or_else(|why| {
                drop(writeln!(io::stderr(), "runner: {why}"));
                2
            });
            std::process::exit(as_exit(code))
        }
        "status" => rows::status(),
        "rows" => rows::rows(&exe, rest),
        "listen" => rows::listen(rest),
        "foreign" => rows::foreign(rest),
        "ports" => {
            ports_rows::ports();
            Ok(())
        }
        other => Err(format!("unknown command {other:?}").into()),
    }
}

/// A `u32` exit code as `exit` takes it.
#[cfg(windows)]
fn as_exit(code: u32) -> i32 {
    i32::from_ne_bytes(code.to_ne_bytes())
}
