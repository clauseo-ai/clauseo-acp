//! A stand-in for agent-browser in the guard's tests (`tests/guard/filter.rs`). Usage:
//! `echo_stub <log file> [ignore-close] [hang] [domain-stuck-once]`. It appends every stdin line,
//! as received, to the log file, and answers each request (an object with an `id` and a
//! `method`) with `{"result": {"received": <the request>}}` (a `tools/list` also gets
//! `"tools"`: an open tool with part of agent-browser's input schema, and the eval tool,
//! `STUB_TOOLS`). It exits 0 at EOF. Methods that do otherwise:
//! - `stub/exit`: exit 7 at once, unanswered;
//! - `stub/partial`: write half a JSON line, no newline, and exit 7;
//! - `stub/flood`: write 64 MiB and one byte with no newline, then wait to be killed.
//!
//! `ignore-close` leaves the guard's close request unanswered; `hang` waits to be killed at EOF
//! instead of exiting. With `domain-stuck-once`, the first stub to get a tool call (the one that
//! writes first to the log file's name with `.stuck` for its extension) answers that call and
//! every later one with agent-browser's refusal of `--allowed-domains`, as v0.41.1 did once the
//! restriction stuck on its session; every later stub answers as usual. Cargo builds it for the
//! tests; it is never shipped.

use std::fs::File;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

/// The tools a `tools/list` reply names: one the guard allows and one it refuses.
const STUB_TOOLS: [&str; 2] = ["agent_browser_open", "agent_browser_eval"];

/// agent-browser v0.41.1's error once `--allowed-domains` stuck on its session.
const STUCK: &str = "--allowed-domains is not supported with --cdp because WebRTC containment \
                     cannot be installed before existing page scripts run";

/// agent-browser v0.41.1's reply once `--allowed-domains` stuck, as the release binary sends it.
fn stuck_reply(id: &Value) -> Value {
    let stdout = json!({"error": STUCK, "success": false}).to_string();
    json!({"jsonrpc": "2.0", "id": id, "result": {
        "content": [{"type": "text", "text": STUCK}], "isError": true,
        "structuredContent": {"exitCode": 1, "response": {"error": STUCK, "success": false},
            "stderr": "", "stdout": format!("{stdout}\n")}}})
}

/// Part of agent-browser v0.41.1's input schema for `agent_browser_open`: two keys that pass
/// and some the guard refuses.
fn open_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "url": {"type": "string"},
            "allowedDomains": {"type": "array", "items": {"type": "string"}},
            "session": {"type": "string"},
            "headed": {"type": "boolean"},
            "webgpu": {"type": "boolean"},
            "webmcp": {"type": "boolean"},
            "timeoutMs": {"type": "integer", "minimum": 1},
        },
    })
}

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

/// Whether this stub is the first to get here: `marker` was empty, and now is not. Stubs get
/// tool calls one at a time, so no two ask at once.
fn first_to_get(marker: &Path) -> bool {
    let Ok(mut file) = open_log(marker) else {
        return false;
    };
    let first = file.metadata().is_ok_and(|meta| meta.len() == 0);
    first && file.write_all(b"stuck\n").is_ok()
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
    let stuck_once = options.iter().any(|o| o == "domain-stuck-once");
    let marker = Path::new(&path).with_extension("stuck");
    // Decided at the first tool call: whether this stub is the stuck one.
    let mut stuck: Option<bool> = None;
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
        let call = message.get("method").and_then(Value::as_str) == Some("tools/call");
        if stuck_once && call && *stuck.get_or_insert_with(|| first_to_get(&marker)) {
            let id = message.get("id").cloned().unwrap_or(Value::Null);
            let reply = stuck_reply(&id);
            writeln!(out, "{reply}")?;
            out.flush()?;
            continue;
        }
        if let (Some(id), Some(_)) = (message.get("id"), message.get("method")) {
            let mut result = json!({"received": message});
            if message.get("method").and_then(Value::as_str) == Some("tools/list") {
                let tools: Vec<Value> = STUB_TOOLS
                    .iter()
                    .map(|name| match *name {
                        "agent_browser_open" => json!({"name": name, "inputSchema": open_schema()}),
                        _ => json!({"name": name}),
                    })
                    .collect();
                result["tools"] = Value::Array(tools);
            }
            let reply = json!({"jsonrpc": "2.0", "id": id, "result": result});
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    if hang {
        wait_to_be_killed();
    }
    Ok(ExitCode::SUCCESS)
}
