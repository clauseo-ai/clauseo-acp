//! Forced ask's hook layer (`ask.rs`): with `--ask-all` every tool but the local reads gets
//! Claude Code's `ask` decision on stdout, MCP tools and the web tools included; without it the
//! guard writes nothing to stdout, as before.

use std::ffi::OsString;

use serde_json::{Value, json};

use super::super::run_timed;

/// The guard's exit code and stdout for a `tool` call with `args`.
fn guard(args: &[&str], tool: &str) -> (i32, String) {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let hook = json!({
        "cwd": std::env::temp_dir(),
        "tool_name": tool,
        "tool_input": {"url": "https://example.com", "pattern": "x"},
    });
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let code = run_timed(
        &args,
        hook.to_string().as_bytes(),
        (&mut output, &mut errors),
        false,
    );
    (code, String::from_utf8(output).unwrap())
}

fn forced() -> Vec<String> {
    let matter = std::env::temp_dir().display().to_string();
    vec!["--matter".to_owned(), matter, "--ask-all".to_owned()]
}

fn asked(output: &str) -> bool {
    let decision: Value = serde_json::from_str(output.trim()).unwrap();
    decision["hookSpecificOutput"]["permissionDecision"] == "ask"
        && decision["hookSpecificOutput"]["hookEventName"] == "PreToolUse"
}

#[test]
fn an_mcp_tool_and_the_web_tools_ask() {
    let args = forced();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    for tool in [
        "mcp__browser__navigate",
        "WebFetch",
        "WebSearch",
        "Bash",
        "Edit",
        "Agent",
    ] {
        let (code, output) = guard(&args, tool);
        assert_eq!(code, 0, "{tool}");
        assert!(asked(&output), "{tool}: {output}");
    }
}

#[test]
fn the_local_reads_go_on_unasked() {
    let args = forced();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    for tool in [
        "Read",
        "Glob",
        "Grep",
        "LS",
        "NotebookRead",
        "TodoWrite",
        "Task",
    ] {
        assert_eq!(guard(&args, tool), (0, String::new()), "{tool}");
    }
}

#[test]
fn input_it_cannot_read_asks() {
    let args: Vec<OsString> = forced().into_iter().map(OsString::from).collect();
    let mut output = Vec::new();
    let code = run_timed(&args, &b"not json"[..], (&mut output, Vec::new()), false);
    assert_eq!(code, 0);
    assert!(asked(&String::from_utf8(output).unwrap()));
}

#[test]
fn without_the_flag_stdout_stays_empty() {
    let matter = std::env::temp_dir().display().to_string();
    for tool in ["mcp__browser__navigate", "WebFetch", "Bash"] {
        assert_eq!(
            guard(&["--matter", &matter], tool),
            (0, String::new()),
            "{tool}"
        );
    }
}
