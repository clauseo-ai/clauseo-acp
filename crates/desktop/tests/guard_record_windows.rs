//! The pre-write guard through the hook command Codex runs on Windows (Windows CI), as Codex runs
//! it: the session's shell, PowerShell, `<ps> -NoProfile -Command <commandWindows>`, the line
//! passed as one argument, the environment cleared and set again, the hook input on stdin, from
//! the turn's folder (codex-rs at rust-v0.162.1 `core/src/session/mod.rs:5141-5149`,
//! `hooks/src/engine/command_runner.rs:384-433`), under Windows PowerShell 5.1 and, when
//! installed, PowerShell 7. A data folder under `…\O'Brien & Sons Zoë Q\…\data` (an apostrophe,
//! an ampersand, spaces, non-ASCII names, a record path over 260 characters); the record made as the host's launch makes it
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

/// What the line writes when the guard could not run (`pre_write_guard.rs`, `UNCHECKED`).
const UNCHECKED: &str = "Clauseo could not check this step, so it did not run.";

/// The shells to run the hook under, the one Codex 0.162.1 would pick on this machine first
/// (`shell-command/src/shell_detect.rs:257-297`: `pwsh` on `PATH`, then PowerShell 7 at its
/// install path, then `powershell` on `PATH`, then Windows PowerShell at its system path; a
/// `WindowsApps` hit skipped), then every other one of those found. On CI both PowerShell 7 and
/// Windows PowerShell must be there; a local run takes what it finds.
#[cfg(test)]
fn shells() -> Vec<String> {
    let at = |base: &str, rest: &str| {
        std::env::var_os(base)
            .map(|folder| Path::new(&folder).join(rest))
            .filter(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned())
    };
    let found = [
        on_path("pwsh"),
        at("ProgramFiles", r"PowerShell\7\pwsh.exe"),
        on_path("powershell"),
        at(
            "SystemRoot",
            r"System32\WindowsPowerShell\v1.0\powershell.exe",
        ),
    ];
    let on_ci = std::env::var_os("CI").is_some_and(|value| !value.is_empty());
    let has = |name: &str| {
        found
            .iter()
            .flatten()
            .any(|shell| shell.to_ascii_lowercase().ends_with(name))
    };
    assert!(
        (has("pwsh.exe") && has("powershell.exe")) || !on_ci,
        "this CI runner lacks PowerShell 7 or Windows PowerShell: {found:?}"
    );
    let mut shells: Vec<String> = Vec::new();
    for shell in found.into_iter().flatten() {
        if !shells
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&shell))
        {
            shells.push(shell);
        }
    }
    if shells.is_empty() {
        shells.push("powershell.exe".to_owned());
    }
    shells
}

/// The first `name` on `PATH` outside `WindowsApps`, as `where.exe` finds it; `None` when
/// there is none.
#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test asks where.exe for the PowerShell Codex would find on PATH"
)]
fn on_path(name: &str) -> Option<String> {
    let output = Command::new("where.exe").arg(name).output().ok()?;
    let found = String::from_utf8(output.stdout).ok()?;
    found
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.to_ascii_lowercase().contains(r"\windowsapps\"))
        .map(str::to_owned)
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
         \"timeout\":30,\"type\":\"command\"}}],\"matcher\":\"apply_patch|Write|Edit|Bash\"}}",
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
    // No folder name ends in a space or a dot: Windows drops those from a path it doesn't take
    // verbatim, and no lawyer's folder is named so.
    let long = folder(
        user.path(),
        "Sharma v State 2024 papers ".repeat(5).trim_end(),
    );
    let longer = folder(
        long.path(),
        "Müller GmbH correspondence ".repeat(4).trim_end(),
    );
    let data = folder(longer.path(), "data");
    let data = data.path();
    assert!(
        data.to_str().unwrap().contains(r"O'Brien & Sons Zoë Q\"),
        "{data:?}"
    );
    let (record, id) = clauseo_host::pre_write_guard::make_record(data, "s1").unwrap();
    assert_eq!(record, data.join(r"browser\guard\s1\guard-blocks.jsonl"));
    assert!(record.as_os_str().len() > 260, "{record:?}");

    let config = clauseo_host::codex_hook_config(Path::new(APP), Some((&record, &id))).unwrap();
    let hook = &config["PreToolUse"][0]["hooks"][0];
    let command = hook["commandWindows"].as_str().unwrap();
    let quoted = |path: &str| format!("'{}'", path.replace('\'', "''"));
    assert!(
        command.starts_with(&format!(
            "$global:LASTEXITCODE = -1; & {} --pre-write-guard --record {} --record-id '{id}'; $c",
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
    // Fifty runs under each shell: every one gets the whole input on stdin (no record here, so an
    // input lost on the way would let the step go and fail the run).
    for shell in &shells() {
        for run in 0..50 {
            let (output, _) = as_codex_runs_it(shell, &command, &input, matter.path());
            assert_eq!(
                output.status.code(),
                Some(2),
                "{shell} run {run}: {output:?}"
            );
            let errors = String::from_utf8(output.stderr).expect("stderr stays UTF-8");
            assert!(errors.contains(want), "{shell} run {run}: {errors:?}");
        }
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
