//! A shell command the reader cannot finish within its depth and size caps
//! (`words::MAX_NESTING`, `words::MAX_NESTED_BYTES`) is refused, by every reading of it, quickly
//! and on a small stack.

use std::time::Duration;

use serde_json::{Value, json};

use super::{Fixture, guard_with};
use crate::clock::{Clock, SystemClock};

/// How long a refusal may take: the reading is linear in the command, at about a microsecond a
/// byte in a test build (debug) and well under a tenth of that optimised.
const BOUND: Duration = Duration::from_secs(if cfg!(debug_assertions) { 10 } else { 1 });

/// The exact stderr line refusing a command the reader cannot finish.
fn unread() -> String {
    "Clauseo can't read this command: it is nested too deep or is too long. Split it into \
     smaller commands.\n"
        .to_owned()
}

/// `open` nested `depth` times around `x`.
fn nested(open: &str, depth: usize) -> String {
    format!("{}x{}", open.repeat(depth), ")".repeat(depth))
}

/// The deep and long commands: 20,000 levels of each opening, and a 1 MiB body two levels in.
fn past_the_caps() -> Vec<String> {
    vec![
        nested("$(", 20_000),
        nested("<<$(", 20_000),
        format!("$($({}))", "x".repeat(1 << 20)),
    ]
}

/// What the guard says to `tool` running `command`, as Claude Code's entry (with `--matter`)
/// or Codex's, run on a 1 MiB stack within [`BOUND`].
fn judged(fx: &Fixture, claude: bool, tool: &str, command: &Value) -> String {
    let matter = fx.matter();
    let matter = matter.to_str().unwrap();
    let args: &[&str] = if claude { &["--matter", matter] } else { &[] };
    let input = fx.hook(tool, &json!({ "command": command })).to_string();
    std::thread::scope(|scope| {
        let judge = std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn_scoped(scope, || {
                let started = SystemClock.now().mono;
                let said = guard_with(args, input.as_bytes());
                (said, SystemClock.now().mono.saturating_sub(started))
            })
            .unwrap();
        let (said, took) = judge.join().unwrap();
        assert!(took < BOUND, "{tool}: {took:?}");
        said
    })
}

fn claude(fx: &Fixture, tool: &str, command: &Value) -> String {
    judged(fx, true, tool, command)
}

#[test]
fn a_powershell_command_nested_past_the_cap_is_refused() {
    let fx = Fixture::new("nested-ps");
    for deep in past_the_caps() {
        let command = json!(format!("Remove-Item Free.docx; {deep}"));
        assert_eq!(claude(&fx, "PowerShell", &command), unread());
        let parts = json!(["Remove-Item", "Free.docx", deep]);
        assert_eq!(claude(&fx, "PowerShell", &parts), unread());
    }
}

#[test]
fn a_script_nested_past_the_cap_inside_a_bash_command_is_refused() {
    let fx = Fixture::new("nested-script");
    for deep in past_the_caps() {
        for command in [
            json!(format!("rm Free.docx; pwsh -c '{deep}'")),
            json!(format!(
                "rm Free.docx && powershell.exe -Command \"{deep}\""
            )),
        ] {
            assert_eq!(claude(&fx, "Bash", &command), unread());
            // Codex's hook, no `--matter`.
            assert_eq!(judged(&fx, false, "Bash", &command), unread());
        }
    }
}

/// Under Codex's hook on Windows a `Bash` command is read as `PowerShell` too.
#[cfg(windows)]
#[test]
fn a_codex_command_nested_past_the_cap_is_refused_on_windows() {
    let fx = Fixture::new("nested-codex");
    for deep in past_the_caps() {
        let command = json!(format!("Remove-Item Free.docx; {deep}"));
        assert_eq!(judged(&fx, false, "Bash", &command), unread());
    }
}

/// Past the first words the targets are read from, the names of the held files are read from
/// more of them: a cut there is refused too.
#[test]
fn a_command_cut_past_its_first_words_is_refused() {
    let fx = Fixture::new("nested-late");
    let words = vec!["a.txt"; 80].join(" ");
    for deep in past_the_caps() {
        let command = json!(format!("Get-Item {words}; {deep}"));
        assert_eq!(claude(&fx, "PowerShell", &command), unread());
    }
}

/// Within the caps nothing changes: a nested command naming no held file goes on, one naming
/// one is refused for it, and a quote left open is read as before.
#[test]
fn a_command_nested_within_the_cap_is_judged_as_before() {
    let fx = Fixture::new("nested-within");
    for open in ["$(", "<<$("] {
        let deep = nested(open, 30);
        let free = json!(format!("Remove-Item Free.docx; {deep}"));
        assert_eq!(claude(&fx, "PowerShell", &free), "", "{open}");
        let held = json!(format!("Remove-Item Contract.docx; {deep}"));
        assert_eq!(
            claude(&fx, "PowerShell", &held),
            super::deny("Contract.docx"),
            "{open}"
        );
    }
    assert_eq!(
        claude(&fx, "PowerShell", &json!("Remove-Item 'Free.docx")),
        ""
    );
}
