//! A stand-in for agent-browser in the guard's tests (`tests/guard/env.rs`): prints one JSON
//! line with its arguments, environment and working folder, then exits 0. Cargo builds it for
//! the tests; it is never shipped.

use std::io::Write;

use serde_json::{Map, Value, json};

fn main() -> std::io::Result<()> {
    let env: Map<String, Value> = std::env::vars_os()
        .map(|(name, value)| {
            let value = Value::String(value.to_string_lossy().into_owned());
            (name.to_string_lossy().into_owned(), value)
        })
        .collect();
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let cwd = std::env::current_dir()?;
    let line = json!({"args": args, "env": env, "cwd": cwd.to_string_lossy()});
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}")?;
    out.flush()
}
