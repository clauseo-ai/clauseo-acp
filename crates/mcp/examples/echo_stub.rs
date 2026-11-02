//! A stand-in for agent-browser in the guard's tests (`tests/guard/filter.rs`). Usage:
//! `echo_stub <log file> [ignore-close] [hang]`. It appends every stdin line, as received, to the
//! log file, and answers each request (an object with an `id` and a `method`) with
//! `{"result": {"received": <the request>}}`. It exits 0 at EOF. Methods that do otherwise:
//! - `stub/exit`: exit 7 at once, unanswered;
//! - `stub/partial`: write half a JSON line, no newline, and exit 7;
//! - `stub/flood`: write 64 MiB and one byte with no newline, then wait to be killed.
//!
//! `ignore-close` leaves the guard's close request unanswered; `hang` waits to be killed at EOF
//! instead of exiting. Cargo builds it for the tests; it is never shipped.

use std::fs::File;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

#[expect(
    clippy::disallowed_methods,
    reason = "the test stub's log is its only output file; it has no write module to go through"
)]
fn open_log(path: &Path) -> std::io::Result<File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// Blocks until the process is killed.
fn wait_to_be_killed() -> ! {
    loop {
        std::thread::park();
    }
}

fn main() -> std::io::Result<ExitCode> {
    let Some(path) = std::env::args_os().nth(1) else {
        return Ok(ExitCode::from(2));
    };
    let options: Vec<String> = std::env::args().skip(2).collect();
    let ignore_close = options.iter().any(|o| o == "ignore-close");
    let hang = options.iter().any(|o| o == "hang");
    let mut log = open_log(Path::new(&path))?;
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        log.write_all(line.as_bytes())?;
        log.write_all(b"\n")?;
        log.flush()?;
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match message.get("method").and_then(Value::as_str) {
            Some("stub/exit") => return Ok(ExitCode::from(7)),
            Some("stub/partial") => {
                out.write_all(br#"{"jsonrpc":"2.0","id":1,"res"#)?;
                out.flush()?;
                return Ok(ExitCode::from(7));
            }
            Some("stub/flood") => {
                let chunk = vec![b'x'; 1024 * 1024];
                for _ in 0..64 {
                    out.write_all(&chunk)?;
                }
                out.write_all(b"x")?;
                out.flush()?;
                wait_to_be_killed();
            }
            _ => {}
        }
        if ignore_close && message.get("id").and_then(Value::as_str) == Some("clauseo-guard-close")
        {
            continue;
        }
        if let (Some(id), Some(_)) = (message.get("id"), message.get("method")) {
            let reply = json!({"jsonrpc": "2.0", "id": id, "result": {"received": message}});
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    if hang {
        wait_to_be_killed();
    }
    Ok(ExitCode::SUCCESS)
}
