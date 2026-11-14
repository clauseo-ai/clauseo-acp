//! Each shell call starts in the matter's folder: Clauseo launches Claude with
//! `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1`, which puts its shell back there after every
//! call, so a `cd` in one call never moves where the next call's relative write lands.

use std::path::Path;

use crate::lawyer_file::{Answer, answered_turn, flag};
use clauseo_protocol::StreamId;

use crate::support::{Dir, FAKE, START_MS, Scenario, matter_with_thread};
use crate::write;

/// Dates `file` a minute before the turn's end, as the agent's own new file is.
fn before_end(file: &Path) {
    let at = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis(u64::try_from(START_MS - 60_000).unwrap());
    write::set_mtime(file, at);
}

/// `cd "<matter>/Sub"` in one call, `pandoc a.md -o x.docx` in the next: the next call starts
/// in the matter's folder again, so the root `x.docx` is the agent's and the `Sub/x.docx` the
/// lawyer edited meanwhile stays flagged.
#[test]
fn a_cd_in_one_call_does_not_move_the_next_calls_write() {
    let files = [("a.md", "# Reply\n"), ("Sub/x.docx", "the lawyer's x\n")];
    let (_dir, host, _, _, _, flags) = answered_turn(
        "shell_folder_reset",
        "shell-folder-reset",
        &files,
        |folder| {
            write::write(&folder.join("x.docx"), b"the agent's reply\n");
            before_end(&folder.join("x.docx"));
            write::write(&folder.join("Sub/x.docx"), b"the lawyer's x, edited\n");
        },
        Answer::Allow,
    );
    assert_eq!(flags, [flag("Sub/x.docx", true), flag("x.docx", false)]);
    host.shutdown();
}

/// `files`'s matter, the agent playing `scenario`: `Sub/x.docx` the lawyer edits while the
/// agent writes the root `x.docx` relative to a call that starts in the matter's folder.
fn root_write_after_cd(test: &str, scenario: &'static str) -> Vec<(String, bool)> {
    let files = [("a.md", "# Reply\n"), ("Sub/x.docx", "the lawyer's x\n")];
    let (_dir, host, _, _, _, flags) = answered_turn(
        test,
        scenario,
        &files,
        |folder| {
            write::write(&folder.join("x.docx"), b"the agent's reply\n");
            before_end(&folder.join("x.docx"));
            write::write(&folder.join("Sub/x.docx"), b"the lawyer's x, edited\n");
        },
        Answer::Allow,
    );
    host.shutdown();
    flags
}

/// Bash's `cd` in one call, PowerShell's relative write in the next: Claude Code's two shells
/// share one folder, back in the matter's after each call.
#[test]
fn a_bash_cd_does_not_move_a_later_powershell_write() {
    let flags = root_write_after_cd("shell_folder_powershell", "shell-folder-powershell");
    assert_eq!(flags, [flag("Sub/x.docx", true), flag("x.docx", false)]);
}

/// A `run_in_background` call's `cd` never moves Claude Code's folder.
#[test]
fn a_background_cd_does_not_move_the_next_calls_write() {
    let flags = root_write_after_cd("shell_folder_background", "shell-folder-background");
    assert_eq!(flags, [flag("Sub/x.docx", true), flag("x.docx", false)]);
}

/// Claude Code's policy settings turn the variable off (`"0"`): its shell keeps the folder a
/// call's `cd` reached, so the next call's relative `-o x.docx` is unclear and the root
/// `x.docx` the lawyer edited stays flagged.
#[test]
fn a_policy_that_turns_the_variable_off_leaves_relative_writes_unclear() {
    let dir = Dir::new(
        "shell_folder_policy",
        &[(FAKE, Scenario::Local("shell-folder-reset"))],
    );
    let managed = dir.data().parent().unwrap().join("managed-settings.json");
    write::write(
        &managed,
        br#"{"env": {"CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR": "0"}}"#,
    );
    let host = dir.start(0);
    let files = [("a.md", "# Reply\n"), ("x.docx", "the lawyer's x\n")];
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &files, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Draft the reply.");
    events.until("the read's permission request", |e| {
        !e.permission_requests().is_empty()
    });
    write::write(&folder.join("x.docx"), b"the lawyer's x, edited\n");
    host.ok(clauseo_protocol::HostCommand::AnswerPermission {
        thread,
        request: events.permission_requests()[0],
        option_id: clauseo_protocol::AgentChoiceId::new("allow-once").unwrap(),
    });
    events.until_turn_ended(turn);
    let changed = events
        .files_changed(turn)
        .unwrap_or_else(|| panic!("no FilesChanged: {}", events.dump()));
    let flags: Vec<(String, bool)> = changed
        .files
        .iter()
        .map(|file| {
            (
                file.path().as_str().to_owned(),
                file.may_include_lawyer_edits(),
            )
        })
        .collect();
    assert_eq!(flags, [flag("x.docx", true)]);
    host.shutdown();
}
