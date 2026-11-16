//! The pre-write guard through the hook command Codex runs on Windows (Windows CI), as Codex runs
//! it: the session's shell, PowerShell, `<ps> -NoProfile -Command <commandWindows>`, the line
//! passed as one argument, the environment cleared and set again, the hook input on stdin, from
//! the turn's folder (codex-rs at rust-v0.162.1 `core/src/session/mod.rs:5141-5149`,
//! `hooks/src/engine/command_runner.rs:384-433`), under Windows PowerShell 5.1 and, when
//! installed, PowerShell 7. A data folder under `…\O'Brien & Sons Zoë Q\…\data` (an apostrophe,
//! an ampersand, spaces, non-ASCII names, a record path over 200 characters); the record made as the host's launch makes it
//! (`clauseo_host::pre_write_guard::make_record`); the hook built as the host builds it
//! (`clauseo_host::codex_hook_config`), with the built app as the guard. The guard refuses a
//! `.codex` write with its code 2 and its reason, and appends one line to the record; a file held
//! open under a Devanagari name is refused with that name in the reason, byte for byte (the
//! input reached the guard and its stderr came back unchanged); a guard that isn't at its path
//! ends with 2 and plain words. The hook's trust hash is the one Codex computes for the command
//! (`config/src/fingerprint.rs`, worked out here again from its rule). Prints the hook's time.
#![cfg(windows)]

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use sha2::{Digest as _, Sha256};

/// The binary under test.
const APP: &str = env!("CARGO_BIN_EXE_clauseo-desktop");

/// What the line writes when the guard could not run (`agent/loads/codex.rs`, `UNCHECKED`).
const UNCHECKED: &str = "Clauseo could not check this step, so it did not run.";

/// The shells Codex may run a hook under: Windows PowerShell, and PowerShell 7 where it is
/// installed in its usual place (Codex prefers it when it is on `PATH`).
#[cfg(test)]
fn shells() -> Vec<String> {
    let mut shells = vec!["powershell.exe".to_owned()];
    let pwsh = std::env::var_os("ProgramFiles")
        .map(|folder| Path::new(&folder).join(r"PowerShell\7\pwsh.exe"))
        .filter(|pwsh| pwsh.is_file());
    // Without PowerShell 7 a local run is Windows PowerShell's alone; on CI it must be there.
    let on_ci = std::env::var_os("CI").is_some_and(|value| !value.is_empty());
    assert!(
        pwsh.is_some() || !on_ci,
        "PowerShell 7 is not at %ProgramFiles%\\PowerShell\\7\\pwsh.exe on this CI runner"
    );
    shells.extend(pwsh.map(|pwsh| pwsh.to_string_lossy().into_owned()));
    shells
}

/// Runs `command` as Codex runs a hook's `commandWindows` under `shell`, `input` on its stdin,
/// from `cwd`; the output and how long it took.
#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts PowerShell and the built app itself, as Codex does, to read the \
              guard's exit code and time; the process crate is for agents and tools"
)]
fn as_codex_runs_it(shell: &str, command: &str, input: &[u8], cwd: &Path) -> (Output, Duration) {
    let started = std::time::Instant::now();
    let mut child = Command::new(shell)
        .arg("-NoProfile")
        .arg("-Command")
        .arg(command)
        .env_clear()
        .envs(std::env::vars_os())
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    (output, started.elapsed())
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

/// A Codex hook input from `cwd` for `tool` with `command`.
#[cfg(test)]
fn codex_input(cwd: &Path, tool: &str, command: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "cwd": cwd,
        "tool_name": tool,
        "tool_input": { "command": command },
    }))
    .unwrap()
}

/// A folder `name` under `parent`, made now.
#[cfg(test)]
fn folder(parent: &Path, name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(name)
        .rand_bytes(0)
        .tempdir_in(parent)
        .unwrap()
}

#[test]
fn the_real_hook_command_refuses_and_records_under_powershell_from_an_apostrophe_folder() {
    let root = tempfile::Builder::new()
        .prefix("guard-ps-")
        .tempdir()
        .unwrap();
    // A Windows user named as lawyers are, under a folder long enough that the record's path is
    // over 200 characters.
    let user = folder(root.path(), "O'Brien & Sons Zoë Q");
    let long = folder(user.path(), &"Sharma v State 2024 papers ".repeat(5));
    let data = folder(long.path(), "data");
    let data = data.path();
    assert!(
        data.to_str().unwrap().contains(r"O'Brien & Sons Zoë Q\"),
        "{data:?}"
    );
    let (record, id) = clauseo_host::pre_write_guard::make_record(data, "s1").unwrap();
    assert_eq!(record, data.join(r"browser\guard\s1\guard-blocks.jsonl"));
    assert!(record.as_os_str().len() > 200, "{record:?}");

    let config = clauseo_host::codex_hook_config(Path::new(APP), Some((&record, &id))).unwrap();
    let hook = &config["PreToolUse"][0]["hooks"][0];
    let command = hook["commandWindows"].as_str().unwrap();
    let quoted = |path: &str| format!("'{}'", path.replace('\'', "''"));
    assert!(
        command.starts_with(&format!(
            "$global:LASTEXITCODE = -1; & {} --pre-write-guard --record {} --record-id {id}; $c",
            quoted(APP),
            quoted(&record.display().to_string()),
        )),
        "{command}"
    );
    assert!(
        !command.contains('"'),
        "no double quote to escape: {command}"
    );
    let state = &config["state"][r"C:\<session-flags>\config.toml:pre_tool_use:0:0"];
    assert_eq!(
        state["trusted_hash"],
        codex_hash(command),
        "the trust check"
    );

    let shells = shells();
    for shell in &shells {
        let input = codex_input(user.path(), "Bash", "touch .codex/config.toml");
        let (output, took) = as_codex_runs_it(shell, command, &input, user.path());
        eprintln!("TIMING {shell}: the hook, PowerShell's start and the guard: {took:?}");
        let errors = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        assert!(errors.contains(".codex"), "{shell}: {errors}");
        assert!(!errors.contains(UNCHECKED), "{shell}: {errors}");
        assert!(output.stdout.is_empty(), "{shell}: {output:?}");
        // A step the guard lets go: exit 0, nothing written.
        let input = codex_input(user.path(), "Bash", "dir");
        let (output, took) = as_codex_runs_it(shell, command, &input, user.path());
        eprintln!("TIMING {shell}: the hook letting a step go: {took:?}");
        assert_eq!(output.status.code(), Some(0), "{shell}: {output:?}");
        assert!(
            output.stdout.is_empty() && output.stderr.is_empty(),
            "{output:?}"
        );
    }
    let text = std::fs::read_to_string(&record).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), shells.len(), "one line per refusal: {text:?}");
    for line in &lines {
        assert_eq!(line["reason"], "codex_folder", "{line}");
        assert_eq!(line["agent"], "codex", "{line}");
        assert_eq!(line["tool"], "Bash", "{line}");
    }
}

/// A patch to a file another program holds open with no sharing, under a Devanagari name in a
/// folder with an apostrophe: the reason names the file, so the name crossed stdin into the
/// guard and its stderr back out byte for byte.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test holds a file open with no sharing, as Word would"
)]
fn a_held_file_with_a_devanagari_name_is_refused_with_its_name_byte_for_byte() {
    use std::os::windows::fs::OpenOptionsExt as _;
    let root = tempfile::Builder::new()
        .prefix("guard-held-")
        .tempdir()
        .unwrap();
    let matter = folder(root.path(), "O'Brien मामला");
    let file = matter.path().join("अनुबंध.docx");
    std::fs::write(&file, b"x").unwrap();
    let _held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    let command = clauseo_host::codex_hook_config(Path::new(APP), None).unwrap();
    let command = command["PreToolUse"][0]["hooks"][0]["commandWindows"]
        .as_str()
        .unwrap()
        .to_owned();
    let patch = "*** Begin Patch\n*** Update File: अनुबंध.docx\n@@\n-x\n+y\n*** End Patch\n";
    let input = codex_input(matter.path(), "apply_patch", patch);
    let want = "अनुबंध.docx is open in Word or another program on this computer.";
    for shell in &shells() {
        let (output, _) = as_codex_runs_it(shell, &command, &input, matter.path());
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        let errors = String::from_utf8(output.stderr).expect("stderr stays UTF-8");
        assert!(errors.contains(want), "{shell}: {errors:?}");
    }
}

/// A guard that isn't at its path: the hook ends with 2 and the plain words, so Codex blocks.
#[test]
fn a_guard_that_cannot_start_blocks_with_plain_words() {
    let root = tempfile::Builder::new()
        .prefix("guard-gone-")
        .tempdir()
        .unwrap();
    let gone = root.path().join("O'Brien").join("clauseo.exe");
    let config = clauseo_host::codex_hook_config(&gone, None).unwrap();
    let command = config["PreToolUse"][0]["hooks"][0]["commandWindows"]
        .as_str()
        .unwrap();
    for shell in &shells() {
        let input = codex_input(root.path(), "Bash", "dir");
        let (output, _) = as_codex_runs_it(shell, command, &input, root.path());
        let errors = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        assert!(errors.contains(UNCHECKED), "{shell}: {errors}");
    }
}
