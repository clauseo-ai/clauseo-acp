//! The pre-write guard's record through the hook command Codex runs on Windows (Windows CI): a data
//! folder under `…\Zoë Q\data`, a space and a non-ASCII name in its path; the record made as the
//! host's launch makes it (`clauseo_host::pre_write_guard::make_record`); the hook built as the
//! host builds it (`clauseo_host::codex_hook_config`), with the built app as the guard; its
//! `commandWindows` run as Codex runs it, `cmd.exe /C "<command>"`
//! (`hooks/src/engine/command_runner.rs`). The guard refuses a `.codex` write with its code 2 and
//! appends one line to the record, and the hook's trust hash is the one Codex computes for that
//! command (`config/src/fingerprint.rs`, worked out here again from its rule).
#![cfg(windows)]

use std::io::Write as _;
use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use sha2::{Digest as _, Sha256};

/// The binary under test.
const APP: &str = env!("CARGO_BIN_EXE_clauseo-desktop");

/// Runs `command` as Codex runs a hook's `commandWindows`, `input` on its stdin.
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts cmd.exe and the built app itself to read the guard's exit code; the \
              process crate is for agents and tools"
)]
#[cfg(test)]
fn through_cmd(command: &str, input: &[u8]) -> Output {
    let mut child = Command::new("cmd.exe")
        .arg("/C")
        .raw_arg(format!("\"{command}\""))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input).unwrap();
    drop(stdin);
    child.wait_with_output().unwrap()
}

/// Codex's trust hash of a `PreToolUse` command hook: the hook's identity as JSON with every
/// object's keys sorted, compact, SHA-256 (`version_for_toml`).
#[cfg(test)]
fn codex_hash(command: &str) -> String {
    let identity = format!(
        "{{\"event_name\":\"pre_tool_use\",\"hooks\":[{{\"async\":false,\"command\":{},\
         \"timeout\":10,\"type\":\"command\"}}],\"matcher\":\"apply_patch|Write|Edit|Bash\"}}",
        serde_json::to_string(command).unwrap()
    );
    let digest = Sha256::digest(identity.as_bytes());
    let mut hash = String::from("sha256:");
    for byte in digest {
        hash.push(char::from_digit(u32::from(byte >> 4), 16).unwrap());
        hash.push(char::from_digit(u32::from(byte & 0xf), 16).unwrap());
    }
    hash
}

/// A Codex hook input from `cwd`: a shell command writing into `.codex`, which the guard refuses
/// and records.
#[cfg(test)]
fn codex_write(cwd: &Path) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "cwd": cwd,
        "tool_name": "Bash",
        "tool_input": { "command": "touch .codex/config.toml" },
    }))
    .unwrap()
}

#[test]
fn the_real_hook_command_appends_through_cmd_from_a_spaced_non_ascii_data_folder() {
    let root = tempfile::Builder::new()
        .prefix("guard-cmd-")
        .tempdir()
        .unwrap();
    let user = tempfile::Builder::new()
        .prefix("Zoë Q")
        .rand_bytes(0)
        .tempdir_in(root.path())
        .unwrap();
    let data = tempfile::Builder::new()
        .prefix("data")
        .rand_bytes(0)
        .tempdir_in(user.path())
        .unwrap();
    let data = data.path();
    assert!(data.to_str().unwrap().contains(r"Zoë Q\data"), "{data:?}");
    let (record, id) = clauseo_host::pre_write_guard::make_record(data, "s1").unwrap();
    assert_eq!(record, data.join(r"browser\guard\s1\guard-blocks.jsonl"));

    let config = clauseo_host::codex_hook_config(Path::new(APP), Some((&record, &id))).unwrap();
    let hook = &config["PreToolUse"][0]["hooks"][0];
    let command = hook["commandWindows"].as_str().unwrap();
    assert_eq!(
        command,
        format!(
            "\"{APP}\" --pre-write-guard --record \"{}\" --record-id {id}",
            record.display()
        )
    );
    let state = &config["state"][r"C:\<session-flags>\config.toml:pre_tool_use:0:0"];
    assert_eq!(
        state["trusted_hash"],
        codex_hash(command),
        "the trust check"
    );

    let output = through_cmd(command, &codex_write(user.path()));
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(".codex"),
        "{output:?}"
    );
    let text = std::fs::read_to_string(&record).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let [line] = lines.as_slice() else {
        panic!("one line: {text:?}");
    };
    assert_eq!(line["reason"], "codex_folder", "{line}");
    assert_eq!(line["agent"], "codex", "{line}");
    assert_eq!(line["tool"], "Bash", "{line}");
}
