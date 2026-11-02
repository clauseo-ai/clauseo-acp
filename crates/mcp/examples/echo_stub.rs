//! A stand-in for agent-browser in the guard's tests (`tests/guard/filter.rs`). Usage:
//! `echo_stub <log file>`. It appends every stdin line, as received, to the log file, and answers
//! each request (an object with an `id` and a `method`) with `{"result": {"received": <the
//! request>}}`. The method `stub/exit` makes it exit 7 at once, unanswered. It exits 0 at EOF.
//! Cargo builds it for the tests; it is never shipped.

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

fn main() -> std::io::Result<ExitCode> {
    let Some(path) = std::env::args_os().nth(1) else {
        return Ok(ExitCode::from(2));
    };
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
        if message.get("method").and_then(Value::as_str) == Some("stub/exit") {
            return Ok(ExitCode::from(7));
        }
        if let (Some(id), Some(_)) = (message.get("id"), message.get("method")) {
            let reply = json!({"jsonrpc": "2.0", "id": id, "result": {"received": message}});
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
