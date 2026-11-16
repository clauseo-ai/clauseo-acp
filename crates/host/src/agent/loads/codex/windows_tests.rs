//! The Windows hook line run as Codex runs it (Windows CI): `<shell> -NoProfile -Command <line>`
//! with Windows PowerShell 5.1 (`powershell.exe`) and PowerShell 7 (the `pwsh` on `PATH`),
//! the environment cleared and set again, stdin piped (codex-rs at rust-v0.162.1
//! `hooks/src/engine/command_runner.rs:384-433`). Each of the line's endings: the program's 0 and
//! 2 come through unchanged, with its stdout and stderr byte for byte (a Devanagari JSON
//! input echoed back); any other code, an NTSTATUS, or no program at the path ends with 2 and the
//! plain words. A program path with an apostrophe and non-ASCII names stays one word. Prints the
//! time the line takes (PowerShell's start included).

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use super::{UNCHECKED, ps_command, ps_quoted};

/// The variable that makes [`echo_child`] echo its stdin.
const ECHO: &str = "CLAUSEO_PS_LINE_ECHO_CHILD";

/// This test's own name, for the echo child's command line.
const ECHO_TEST: &str = "agent::loads::codex::windows_tests::echo_child";

/// The shells to run the hook under, the one Codex 0.162.1 would pick on this machine first
/// (`shell-command/src/shell_detect.rs:257-297`: `pwsh` on `PATH`, then PowerShell 7 at its
/// install path, then `powershell` on `PATH`, then Windows PowerShell at its system path), then
/// every other one of those found. On CI both PowerShell 7 and
/// Windows PowerShell must be there; a local run takes what it finds.
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

/// Runs `line` as Codex runs a hook under `shell`, `input` on stdin, from `cwd`; the output and
/// how long it took.
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts PowerShell as Codex starts a hook, to read the line's exit code and \
              its time; the process crate is for agents and tools"
)]
fn as_codex_runs_it(shell: &str, line: &str, input: &[u8], cwd: &Path) -> (Output, Duration) {
    let started = std::time::Instant::now();
    let mut child = Command::new(shell)
        .args(["-NoProfile", "-Command", line])
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

fn cmd(args: &str) -> String {
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    let program = PathBuf::from(system).join(r"System32\cmd.exe");
    ps_command(&[
        ps_quoted(&program.to_string_lossy()),
        "/c".to_owned(),
        ps_quoted(args),
    ])
}

/// Not a test of its own: with [`ECHO`] set, copies its stdin to its stdout and its stderr,
/// byte for byte, and exits 2, as a guard that refuses would.
#[test]
fn echo_child() {
    if std::env::var_os(ECHO).is_none() {
        return;
    }
    let mut input = Vec::new();
    std::io::stdin().lock().read_to_end(&mut input).unwrap();
    std::io::stdout().write_all(&input).unwrap();
    std::io::stdout().flush().unwrap();
    std::io::stderr().write_all(&input).unwrap();
    std::io::stderr().flush().unwrap();
    exit_with(2);
}

#[expect(
    clippy::exit,
    reason = "the child stands in for the guard, which ends with its code"
)]
fn exit_with(code: i32) {
    std::process::exit(code);
}

/// 0 comes through with nothing written; 1, an NTSTATUS and a program that isn't there end with
/// 2 and the plain words on stderr. Prints the line's time, cold and warm.
#[test]
fn the_line_passes_0_through_and_turns_every_other_ending_but_2_into_a_refusal() {
    let cwd = std::env::temp_dir();
    for shell in &shells() {
        let mut times = Vec::new();
        for _ in 0..5 {
            let (output, took) = as_codex_runs_it(shell, &cmd("exit 0"), b"{}", &cwd);
            assert_eq!(output.status.code(), Some(0), "{shell}: {output:?}");
            assert!(
                output.stdout.is_empty() && output.stderr.is_empty(),
                "{output:?}"
            );
            times.push(took);
        }
        eprintln!("TIMING {shell}: the line around cmd /c exit 0, runs in order: {times:?}");
        let missing = ps_command(&[
            ps_quoted(r"C:\no such folder\clauseo.exe"),
            "--pre-write-guard".to_owned(),
        ]);
        for line in [cmd("exit 1"), cmd("exit -1073741819"), missing] {
            let (output, _) = as_codex_runs_it(shell, &line, b"{}", &cwd);
            let errors = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(2), "{shell} {line}: {errors}");
            assert!(errors.contains(UNCHECKED), "{shell} {line}: {errors}");
        }
    }
}

/// A program at a path with an apostrophe, a space and non-ASCII names runs, gets the hook's
/// stdin byte for byte (a JSON input naming a Devanagari file), and its stdout and stderr and its
/// 2 come back unchanged, without the plain words.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test copies its own binary to a path with an apostrophe, an ampersand, a \
              semicolon and non-ASCII names"
)]
fn the_programs_2_stdin_stdout_and_stderr_pass_byte_for_byte() {
    let root = crate::pipeline::tests::support::TempDir::new("ps-line");
    let folder = root.path().join("O'Brien & Sons; Zoë Ångström मामला");
    crate::write::create_dir_all(&folder).unwrap();
    let program = folder.join("clauseo guard.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    let line = ps_command(&[
        ps_quoted(&program.to_string_lossy()),
        ps_quoted(ECHO_TEST),
        "--exact".to_owned(),
        "--nocapture".to_owned(),
        "--test-threads=1".to_owned(),
    ]);
    assert!(line.contains("O''Brien"), "{line}");
    let input = r#"{"tool_name":"apply_patch","tool_input":{"command":"*** Update File: अनुबंध.docx"},"cwd":"C:\\मामला"}"#;
    for shell in &shells() {
        let output = Command::new(shell)
            .args(["-NoProfile", "-Command", &line])
            .env_clear()
            .envs(std::env::vars_os())
            .env(ECHO, "1")
            .current_dir(&folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                let mut stdin = child.stdin.take().unwrap();
                stdin.write_all(input.as_bytes())?;
                drop(stdin);
                child.wait_with_output()
            })
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        assert_eq!(
            output.stderr,
            input.as_bytes(),
            "{shell}: stderr byte for byte"
        );
        let stdout = output.stdout;
        let found = stdout
            .windows(input.len())
            .any(|part| part == input.as_bytes());
        assert!(
            found,
            "{shell}: stdout {:?}",
            String::from_utf8_lossy(&stdout)
        );
    }
}

/// A program at a path over 260 characters: the line ends with 2 either way, the program's own 2
/// with its stdin echoed when the shell can start it, else the plain words; never 0.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test copies its own binary to a path over 260 characters"
)]
fn a_program_at_a_path_over_260_characters_never_ends_with_0() {
    let root = crate::pipeline::tests::support::TempDir::new("ps-long");
    // No folder name ends in a space: Windows drops one from a path it doesn't take verbatim.
    let folder = root
        .path()
        .join("O'Brien & Sons ".repeat(9).trim_end())
        .join("Sharma v State 2024 ".repeat(7).trim_end());
    crate::write::create_dir_all(&folder).unwrap();
    let program = folder.join("clauseo guard.exe");
    assert!(program.as_os_str().len() > 260, "{program:?}");
    std::fs::copy(std::env::current_exe().unwrap(), &program).unwrap();
    let line = ps_command(&[
        ps_quoted(&program.to_string_lossy()),
        ps_quoted(ECHO_TEST),
        "--exact".to_owned(),
        "--nocapture".to_owned(),
        "--test-threads=1".to_owned(),
    ]);
    let input = r#"{"tool_name":"Bash","tool_input":{"command":"dir"},"cwd":"C:\\x"}"#;
    for shell in &shells() {
        let output = Command::new(shell)
            .args(["-NoProfile", "-Command", &line])
            .env_clear()
            .envs(std::env::vars_os())
            .env(ECHO, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                let mut stdin = child.stdin.take().unwrap();
                stdin.write_all(input.as_bytes())?;
                drop(stdin);
                child.wait_with_output()
            })
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{shell}: {output:?}");
        let errors = String::from_utf8_lossy(&output.stderr);
        assert!(
            errors == input || errors.contains(UNCHECKED),
            "{shell}: {errors}"
        );
    }
}
