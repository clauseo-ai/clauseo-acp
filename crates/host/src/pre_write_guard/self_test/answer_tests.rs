//! The self-test's second run, on what the guard writes to stdout when it exits 0: read as Codex
//! 0.162.1 reads it (`codex_allows`), so plain text passes and broken or refusing JSON fails.

use std::ffi::OsString;
use std::path::Path;

use super::super::Failed;
use super::{Scratch, answering, entry_for};
use crate::write;

/// The second run passes on exit 0 with a stdout Codex reads as no refusal: nothing, plain
/// text, or a JSON object that does not block; it fails on JSON-looking text that does not
/// parse, on JSON that blocks or denies, and on any other exit code.
#[cfg(unix)]
#[test]
fn the_second_run_needs_exit_0_and_a_stdout_codex_reads_as_no_refusal() {
    let scratch = Scratch::new("second-run");
    let deny = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"no"}}"#;
    for (then, expected) in [
        ("exit 0".to_owned(), Ok(())),
        ("echo '{}'; exit 0".to_owned(), Ok(())),
        ("echo; exit 0".to_owned(), Ok(())),
        ("echo 'plain text'; exit 0".to_owned(), Ok(())),
        ("echo '{not json'; exit 0".to_owned(), Err(Failed::Refused)),
        ("echo '[1]'; exit 0".to_owned(), Err(Failed::Refused)),
        (
            r#"echo '{"decision":"block","reason":"no"}'; exit 0"#.to_owned(),
            Err(Failed::Refused),
        ),
        (format!("echo '{deny}'; exit 0"), Err(Failed::Refused)),
        ("exit 1".to_owned(), Err(Failed::Refused)),
        ("echo no >&2; exit 2".to_owned(), Err(Failed::Refused)),
    ] {
        let entry = entry_for(&scratch.guard(&answering(&then)));
        assert_eq!(scratch.test(entry).run("main"), expected, "{then}");
    }
}

/// A shell start-up file that prints a line (here zsh's `.zshenv`, which `zsh -c` reads, and a
/// guard that greets first for the other shells) does not fail the self-test: Codex ignores plain
/// stdout from a hook that exits 0. A guard that prints malformed JSON does fail it.
#[cfg(unix)]
#[test]
fn a_login_shell_that_prints_a_line_passes_and_malformed_json_fails() {
    let scratch = Scratch::new("shell-echo");
    let zdotdir = scratch.0.join("zdotdir");
    write::create_dir_all(&zdotdir).unwrap();
    write::write_file(&zdotdir.join(".zshenv"), b"echo 'Welcome back'\n").unwrap();
    let test = |then: &str, zdotdir: Option<&Path>| {
        let mut test = scratch.test(entry_for(&scratch.guard(&answering(then))));
        if let Some(zdotdir) = zdotdir {
            test.env
                .insert(OsString::from("ZDOTDIR"), zdotdir.as_os_str().to_owned());
        }
        test.run("main")
    };
    assert_eq!(test("exit 0", Some(&zdotdir)), Ok(()));
    assert_eq!(test("echo 'Welcome back'; exit 0", None), Ok(()));
    assert_eq!(
        test("echo '{\"decision\":'; exit 0", None),
        Err(Failed::Refused)
    );
}
