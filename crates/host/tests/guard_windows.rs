//! Codex's hook on Windows (Windows CI) as Codex runs it, with the console program it names,
//! `clauseo-guard.exe` (`src/bin/clauseo-guard.rs`): the line read back from
//! `clauseo_host::codex_hook_config` for the program `clauseo_host::codex_guard_program` picks
//! beside the app, run as `<ps> -NoProfile -Command <commandWindows>` with the hook input on a
//! redirected stdin and nothing piped inside the line (codex-rs at rust-v0.162.1
//! `core/src/session/mod.rs:5141-5149`, `hooks/src/engine/command_runner.rs:384-433`), under each
//! PowerShell found, the one Codex picks first. A `.codex` write is refused with a record line,
//! from a data folder under `…\O'Brien & Sons Zoë Q\…` (a record path over 260 characters); a
//! harmless step ends with 0; with the session's no-copy list from that folder on the line, a
//! delete of a file it names is refused with a record line; a held file under a Devanagari name,
//! fifty times, and one in `Müller GmbH` are refused by name; a guard that isn't there ends with
//! 2 and the shared line.
//! Codex's home under a user name with `%`, `'`, `’` and `&` reaches the guard as it is.
//! The program is a console program, and its GUI twin (the same bytes, Subsystem 2) run by the
//! same line gives no answer of the guard's: the check that these tests catch a GUI target.
//! The trust hash is the one Codex computes (`config/src/fingerprint.rs`). Prints timings.
#![cfg(windows)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use sha2::{Digest as _, Sha256};

/// The program under test, as cargo built it.
#[cfg(test)]
const GUARD: &str = env!("CARGO_BIN_EXE_clauseo-guard");

/// What the line writes when the guard could not run.
#[cfg(test)]
const UNCHECKED: &str = clauseo_host::pre_write_guard::UNCHECKED;

/// The program Codex's hook names for an app installed beside the guard, as the host picks it.
#[cfg(test)]
fn guard_for_app() -> PathBuf {
    let program =
        clauseo_host::codex_guard_program(&Path::new(GUARD).with_file_name("Clauseo.exe"));
    assert_eq!(
        program,
        Path::new(GUARD),
        "the hook names the program beside the app"
    );
    program
}

/// The `commandWindows` line of the hook `codex_hook_config` builds for `program`.
#[cfg(test)]
fn line_for(program: &Path) -> String {
    let config = clauseo_host::codex_hook_config(program, None, None).unwrap();
    config["PreToolUse"][0]["hooks"][0]["commandWindows"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The shells to run the hook under, the one Codex 0.162.1 would pick on this machine first
/// (`shell-command/src/shell_detect.rs:257-297`: `pwsh` on `PATH`, then PowerShell 7 at its
/// install path, then `powershell` on `PATH`, then Windows PowerShell at its system path), then
/// every other one of those found. On CI both PowerShell 7 and
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

/// The first `name` on `PATH`, as `where.exe` finds it, a Store (`WindowsApps`) one included:
/// Codex's discovery takes the first `which` hit (`shell_detect.rs:205`) and swaps a Store one
/// only for its Windows sandbox's own runs (`fallback_powershell_shell_for_windows_sandbox`,
/// `:288-292`). `None` when there is none.
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
        .find(|line| !line.is_empty())
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
fn the_hook_line_refuses_and_records_and_lets_a_step_go_from_an_apostrophe_folder() {
    let root = tempfile::Builder::new()
        .prefix("guard-ps-")
        .tempdir()
        .unwrap();
    // A Windows user named as lawyers are, under a folder long enough that the record's path is
    // over 260 characters.
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

    let guard = guard_for_app();
    let config = clauseo_host::codex_hook_config(&guard, Some((&record, &id)), None).unwrap();
    let hook = &config["PreToolUse"][0]["hooks"][0];
    let command = hook["commandWindows"].as_str().unwrap();
    let quoted = |path: &str| format!("'{}'", path.replace('\'', "''"));
    assert!(
        command.starts_with(&format!(
            "$global:LASTEXITCODE = -1; & {} --pre-write-guard --record {} --record-id '{id}'; $c",
            quoted(GUARD),
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
    a_listed_file_is_refused_a_delete(user.path(), &guard, (&record, &id), &shells);
}

/// The same line with the session's no-copy list, under the same folder with a space and an
/// apostrophe: the list's path crosses PowerShell whole, so a delete of a file it names is
/// refused, with a record line, under each shell.
#[cfg(test)]
fn a_listed_file_is_refused_a_delete(
    cwd: &Path,
    guard: &Path,
    (record, id): (&Path, &str),
    shells: &[String],
) {
    let folder = record.parent().unwrap();
    let (list, list_id) =
        clauseo_host::pre_write_guard::make_no_copy_list(folder, "add cloud-only Contract.docx\n")
            .unwrap();
    assert!(
        list.to_str().unwrap().contains("O'Brien & Sons"),
        "{}",
        list.display()
    );
    let config =
        clauseo_host::codex_hook_config(guard, Some((record, id)), Some((&list, &list_id)))
            .unwrap();
    let command = config["PreToolUse"][0]["hooks"][0]["commandWindows"]
        .as_str()
        .unwrap();
    assert!(command.contains("--no-copy-list '"), "{command}");
    let before = std::fs::read_to_string(record).unwrap().lines().count();
    for shell in shells {
        let input = codex_input(cwd, "Bash", "rm Contract.docx");
        let (output, _) = as_codex_runs_it(shell, command, &input, cwd);
        let errors = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        assert!(
            errors.contains("Contract.docx can't be brought back"),
            "{shell}: {errors}"
        );
    }
    let text = std::fs::read_to_string(record).unwrap();
    let added: Vec<serde_json::Value> = text
        .lines()
        .skip(before)
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(added.len(), shells.len(), "one line per refusal: {text:?}");
    for line in &added {
        assert_eq!(line["reason"], "no_copy", "{line}");
    }
}

/// Codex's home under a Windows user named with `%`, `'`, `’`, `&` and spaces: the line
/// (trusted by its hash) hands the guard that folder exactly, so a patch into it by its full
/// path is refused as one into Codex's home, and the same patch into the matter goes on.
#[test]
fn codex_home_under_a_user_name_with_percent_and_quotes_reaches_the_guard_exactly() {
    let root = tempfile::Builder::new()
        .prefix("guard-home-")
        .tempdir()
        .unwrap();
    let user = folder(
        root.path(),
        "O'Brien \u{2019}Q\u{2019} 100% & %USERNAME% Co",
    );
    let matter = folder(user.path(), "matter");
    let home = folder(user.path(), "home");
    let guard = guard_for_app();
    let config =
        clauseo_host::codex_hook_config_with_home(&guard, None, None, Some(home.path())).unwrap();
    let command = config["PreToolUse"][0]["hooks"][0]["commandWindows"]
        .as_str()
        .unwrap();
    let state = &config["state"][r"C:\<session-flags>\config.toml:pre_tool_use:0:0"];
    assert_eq!(
        state["trusted_hash"],
        codex_hash(command),
        "the trust check"
    );
    let patch_into = |folder: &Path| {
        let target = folder.join("AGENTS.md");
        let patch = format!(
            "*** Begin Patch\n*** Add File: {}\n+x\n*** End Patch\n",
            target.display()
        );
        codex_input(matter.path(), "apply_patch", &patch)
    };
    for shell in &shells() {
        let (output, _) = as_codex_runs_it(shell, command, &patch_into(home.path()), user.path());
        let errors = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        assert!(errors.contains("(CODEX_HOME)"), "{shell}: {errors}");
        let (output, _) = as_codex_runs_it(shell, command, &patch_into(matter.path()), user.path());
        assert_eq!(output.status.code(), Some(0), "{shell}: {output:?}");
    }
}

/// A patch to a file another program holds open with no sharing, under a Devanagari name in a
/// folder with an apostrophe: the reason names the file, so the name crossed stdin into the
/// guard and its stderr back out byte for byte.
#[test]
fn a_held_file_with_a_devanagari_name_is_refused_with_its_name_byte_for_byte() {
    let root = tempfile::Builder::new()
        .prefix("guard-held-")
        .tempdir()
        .unwrap();
    let matter = folder(root.path(), "O'Brien मामला");
    let _held = held(&matter.path().join("अनुबंध.docx"));
    let command = line_for(&guard_for_app());
    let input = codex_input(matter.path(), "apply_patch", &patch("अनुबंध.docx"));
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
    // An app whose folder holds no guard: the line names the missing program.
    let app = root.path().join("O'Brien").join("Clauseo.exe");
    let command = line_for(&clauseo_host::codex_guard_program(&app));
    for shell in &shells() {
        let input = codex_input(root.path(), "Bash", "dir");
        let (output, _) = as_codex_runs_it(shell, &command, &input, root.path());
        let errors = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        assert!(errors.contains(UNCHECKED), "{shell}: {errors}");
    }
}

/// Holds `file` open with no sharing, as Word would, after writing it.
#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test holds a file open with no sharing, as Word would"
)]
fn held(file: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt as _;
    std::fs::write(file, b"x").unwrap();
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(file)
        .unwrap()
}

/// A patch updating `name`.
#[cfg(test)]
fn patch(name: &str) -> String {
    format!("*** Begin Patch\n*** Update File: {name}\n@@\n-x\n+y\n*** End Patch\n")
}

/// In a matter named `Müller GmbH`, a held `Müller Vertrag.docx` is refused by its name and a
/// listing of the folder goes on.
#[test]
fn a_matter_named_muller_is_checked_by_name() {
    let root = tempfile::Builder::new()
        .prefix("guard-muller-")
        .tempdir()
        .unwrap();
    let matter = folder(root.path(), "Müller GmbH");
    let _held = held(&matter.path().join("Müller Vertrag.docx"));
    let command = line_for(&guard_for_app());
    let want = "Müller Vertrag.docx is open in Word or another program on this computer.";
    for shell in &shells() {
        let input = codex_input(matter.path(), "apply_patch", &patch("Müller Vertrag.docx"));
        let (output, _) = as_codex_runs_it(shell, &command, &input, matter.path());
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        let errors = String::from_utf8(output.stderr).expect("stderr stays UTF-8");
        assert!(errors.contains(want), "{shell}: {errors:?}");
        let input = codex_input(matter.path(), "Bash", "Get-ChildItem");
        let (output, _) = as_codex_runs_it(shell, &command, &input, matter.path());
        assert_eq!(output.status.code(), Some(0), "{shell}: {output:?}");
    }
}

/// The offset of a PE image's `Subsystem` field: the optional header starts 24 bytes after the
/// `PE\0\0` signature (whose offset is at 0x3C), and the field is 68 bytes into it.
#[cfg(test)]
fn subsystem_at(image: &[u8]) -> usize {
    let at: [u8; 4] = image[0x3C..0x40].try_into().unwrap();
    let pe = usize::try_from(u32::from_le_bytes(at)).unwrap();
    assert_eq!(image.get(pe..pe + 4), Some(&b"PE\0\0"[..]), "a PE image");
    pe + 24 + 68
}

/// The PE `Subsystem` of the image at `path`: 2 for a GUI program, 3 for a console one.
#[cfg(test)]
fn subsystem(path: &Path) -> u16 {
    let image = std::fs::read(path).unwrap();
    let at = subsystem_at(&image);
    u16::from_le_bytes([image[at], image[at + 1]])
}

/// A copy of the console program `exe` in `folder` with its `Subsystem` set to 2 (GUI), named
/// as the hook names the guard.
#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test writes a GUI twin of a built program"
)]
fn gui_twin(exe: &Path, folder: &Path) -> PathBuf {
    let mut image = std::fs::read(exe).unwrap();
    let at = subsystem_at(&image);
    image[at..at + 2].copy_from_slice(&2_u16.to_le_bytes());
    let twin = folder.join(clauseo_host::GUARD_EXE);
    std::fs::write(&twin, &image).unwrap();
    twin
}

#[test]
fn the_guard_is_a_console_program() {
    assert_eq!(subsystem(Path::new(GUARD)), 3, "{GUARD}");
}

/// The control: the same line run against a GUI twin of the guard never takes the guard's exit
/// code (PowerShell sets no `$LASTEXITCODE` for a GUI program), so the tests above would fail for
/// a GUI target. Both shells were seen passing the GUI program's stderr through (the guard's
/// reason), but its exit code never reached the line: on every shell a listing doesn't end with
/// 0, and the held file's line ends with 2 and writes the shared could-not-check line. The twin
/// isn't waited for but holds the line's stderr, so its reason may land before or after that line.
#[test]
fn a_gui_twin_of_the_guard_gives_no_answer_of_the_guards() {
    let root = tempfile::Builder::new()
        .prefix("guard-gui-")
        .tempdir()
        .unwrap();
    let twin = gui_twin(Path::new(GUARD), root.path());
    assert_eq!(subsystem(&twin), 2);
    let matter = folder(root.path(), "O'Brien मामला");
    let _held = held(&matter.path().join("अनुबंध.docx"));
    let command = line_for(&twin);
    for shell in &shells() {
        let input = codex_input(matter.path(), "Bash", "dir");
        let (output, _) = as_codex_runs_it(shell, &command, &input, matter.path());
        assert_ne!(output.status.code(), Some(0), "{shell}: {output:?}");
        let input = codex_input(matter.path(), "apply_patch", &patch("अनुबंध.docx"));
        let (output, _) = as_codex_runs_it(shell, &command, &input, matter.path());
        let errors = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        assert!(errors.contains(UNCHECKED), "{shell}: {errors}");
    }
}
