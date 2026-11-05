//! The built app inside a real Seatbelt sandbox (macOS CI): started under `sandbox-exec`, it
//! refuses with exit 77 and exactly its one stderr line; the pre-write guard still runs, inside
//! the sandbox and outside it. The profiles are the ones run by hand on a Mac mini against a
//! debug build. A start that wrongly reached the app would open a window and hang until nextest's
//! timeout ends the test, which fails it.
#![cfg(target_os = "macos")]

use std::io::Write as _;
use std::process::{Command, Output, Stdio};

use clauseo_desktop::{REFUSAL, SANDBOXED_EXIT};

/// The binary under test.
const APP: &str = env!("CARGO_BIN_EXE_clauseo-desktop");
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Everything allowed: only the sandbox's presence is under test.
const ALLOW_DEFAULT: &str = "(version 1)(allow default)";
/// Everything denied but what the start needs: the Mac mini run's profile, verbatim.
const DENY_DEFAULT: &str = "(version 1)(deny default)(allow process-exec)(allow process-fork)(allow file-read*)(allow sysctl-read)(allow mach-lookup (global-name \"com.apple.logd\"))";
/// The deny-default profile the guard ran under on the Mac mini, verbatim.
const DENY_DEFAULT_GUARD: &str = "(version 1)(deny default)(allow process-exec)(allow process-fork)(allow file-read*)(allow sysctl-read)";

/// Runs `program` with `args` and `input` on stdin; returns how it ended.
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts the built app itself to read its exit code and stderr; the process \
              crate is for agents and tools"
)]
#[cfg(test)]
fn run(program: &str, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    if !input.is_empty() {
        stdin.write_all(input).unwrap();
    }
    drop(stdin);
    child.wait_with_output().unwrap()
}

/// The app with `args` under `profile`.
#[cfg(test)]
fn jailed(profile: &str, args: &[&str], input: &[u8]) -> Output {
    let mut all = vec!["-p", profile, APP];
    all.extend_from_slice(args);
    run(SANDBOX_EXEC, &all, input)
}

/// A hook input under Codex's hook (no `--matter`): a `Bash` command naming `.codex`, which the
/// guard refuses with its code 2.
#[cfg(test)]
fn codex_write() -> Vec<u8> {
    let cwd = std::env::current_dir().unwrap();
    serde_json::to_vec(&serde_json::json!({
        "cwd": cwd,
        "tool_name": "Bash",
        "tool_input": { "command": "touch .codex/config.toml" },
    }))
    .unwrap()
}

/// A hook input for a tool the guard does not check: exit 0, nothing written.
#[cfg(test)]
fn unchecked_tool() -> Vec<u8> {
    let cwd = std::env::current_dir().unwrap();
    serde_json::to_vec(&serde_json::json!({
        "cwd": cwd,
        "tool_name": "Read",
        "tool_input": {},
    }))
    .unwrap()
}

#[cfg(test)]
fn assert_refused(output: &Output) {
    assert_eq!(output.status.code(), Some(SANDBOXED_EXIT), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("{REFUSAL}\n")
    );
    assert!(output.stdout.is_empty(), "{output:?}");
}

/// The guard refused `codex_write`: its code 2 and a reason, not the refusal line.
#[cfg(test)]
fn assert_guard_refused(output: &Output) {
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty() && !stderr.contains(REFUSAL), "{stderr}");
}

#[test]
fn under_an_allow_default_profile_the_app_refuses() {
    assert_refused(&jailed(ALLOW_DEFAULT, &[], b""));
    // A deep link, which a second start would hand to the running window.
    assert_refused(&jailed(ALLOW_DEFAULT, &["clauseo://open"], b""));
}

#[test]
fn under_a_deny_default_profile_the_app_refuses() {
    assert_refused(&jailed(DENY_DEFAULT, &[], b""));
}

#[test]
fn the_guard_runs_inside_the_sandbox() {
    assert_guard_refused(&jailed(
        ALLOW_DEFAULT,
        &["--pre-write-guard"],
        &codex_write(),
    ));
    let output = jailed(
        DENY_DEFAULT_GUARD,
        &["--pre-write-guard"],
        &unchecked_tool(),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn the_guard_runs_outside_the_sandbox() {
    assert_guard_refused(&run(APP, &["--pre-write-guard"], &codex_write()));
    let output = run(APP, &["--pre-write-guard"], &unchecked_tool());
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}
