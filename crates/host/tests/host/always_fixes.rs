//! What a kept command never covers (decision record 0063, "Commands"): a command that writes,
//! kept or not; a command when a file named like its program sits in Claude's working folder;
//! and a command when Claude's policy lets its shell keep the folder a `cd` reached.

use clauseo_protocol::{PermissionKind, StopReason, StreamId};
use serde_json::json;

use crate::always::{
    CLAUDE, ask, bash_options, claude_scenario, keep, offers, resolutions, resolved_always,
    wc_offer,
};
use crate::permissions::answer_from;
use crate::support::{Dir, matter_with_thread};

/// The chosen option of each request.
fn chosen(events: &crate::observe::Events) -> Vec<Option<(String, PermissionKind)>> {
    resolutions(events)
        .into_iter()
        .map(|(_, chosen)| chosen)
        .collect()
}

fn reject() -> (String, PermissionKind) {
    ("reject".to_owned(), PermissionKind::RejectOnce)
}

/// A write is never kept: `cp` kept in an earlier version is not applied and its card offers no
/// "Always"; a kept read in the same turn is still answered by the host.
#[test]
fn a_kept_write_is_neither_offered_nor_answered() {
    let dir = Dir::new("always_no_write", &[]);
    let cp = json!({"command": "cp notes.txt Drafts"});
    let cat = json!({"command": "cat notes.txt"});
    let mut turn = ask("cp", "Bash", &cp, &bash_options(), "reject").to_vec();
    turn.extend(ask("cat", "Bash", &cat, &bash_options(), "allow-once"));
    let scenario = claude_scenario(&dir, "claude-no-write", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let files = [("notes.txt", "x\n"), ("Drafts/a.txt", "a\n")];
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    keep(
        &dir,
        matter,
        &["Bash(cp notes.txt Drafts)", "Bash(cat notes.txt)"],
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "File the notes.");
    answer_from(&host, &mut events, thread, 0, &["reject"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    let once = Some(("allow-once".to_owned(), PermissionKind::AllowOnce));
    assert_eq!(chosen(&events), [Some(reject()), once], "{}", events.dump());
    assert_eq!(offers(&events), [None, None], "{}", events.dump());
    assert_eq!(
        resolved_always(&events),
        [None, Some(wc_offer("cat notes.txt"))],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// A kept `Bash(wc -l notes.txt)` with a file named `wc` in Claude's working folder: a `.` on
/// its shell's search path would run that file, so the lawyer is asked.
#[test]
fn a_file_named_like_the_program_in_claudes_folder_stops_a_kept_command() {
    let dir = Dir::new("always_absent_claude", &[]);
    let wc = json!({"command": "wc -l notes.txt"});
    let turn = ask("wc", "Bash", &wc, &bash_options(), "reject").to_vec();
    let scenario = claude_scenario(&dir, "claude-absent", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let files = [("notes.txt", "x\n"), ("wc", "echo hi\n")];
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    keep(&dir, matter, &["Bash(wc -l notes.txt)"]);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Count the lines.");
    answer_from(&host, &mut events, thread, 0, &["reject"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(chosen(&events), [Some(reject())], "{}", events.dump());
    assert_eq!(offers(&events), [None], "{}", events.dump());
    host.shutdown();
}

/// Claude Code's policy turns `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR` off, so its shell may
/// start in another folder: a kept command is not applied and no card offers "Always" for one.
#[test]
fn no_command_is_kept_or_applied_when_claudes_folder_is_unknown() {
    let dir = Dir::new("always_shell_unknown", &[]);
    let managed = dir.data().parent().unwrap().join("managed-settings.json");
    crate::write::write(
        &managed,
        br#"{"env": {"CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR": "0"}}"#,
    );
    let wc = json!({"command": "wc -l notes.txt"});
    let cat = json!({"command": "cat notes.txt"});
    let mut turn = ask("wc", "Bash", &wc, &bash_options(), "reject").to_vec();
    turn.extend(ask("cat", "Bash", &cat, &bash_options(), "reject"));
    let scenario = claude_scenario(&dir, "claude-shell-unknown", false, turn);
    dir.set_agents(&[(CLAUDE, scenario)]);
    let host = dir.start(0);
    let files = [("notes.txt", "x\n")];
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &files, CLAUDE);
    keep(&dir, matter, &["Bash(wc -l notes.txt)"]);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Count the lines.");
    answer_from(&host, &mut events, thread, 0, &["reject", "reject"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(
        chosen(&events),
        [Some(reject()), Some(reject())],
        "{}",
        events.dump()
    );
    assert_eq!(offers(&events), [None, None], "{}", events.dump());
    host.shutdown();
}
