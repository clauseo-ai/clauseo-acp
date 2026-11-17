//! Codex's pre-card path check (`agent/data_fence/other_matters.rs`): a command whose paths lead
//! into another matter's folder (`cp ../Other/brief.txt`, or a link in the matter folder whose
//! target was swapped into another matter), and a file change whose tool call's diff names one
//! (its request names no path, as codex-acp 2.2.2 sends it), even by a path inside the matter
//! folder that leads through a link into another matter (`<matter>/link/x.md`), is refused
//! before any card, with the data fence's refusal and its one `OutsideMatterBlocked` notice; a
//! plain in-matter command still reaches the lawyer. A `multi_agent` child's approvals (the
//! parent's session id, an unannounced call id) go through the same check and the Codex option
//! filter. The scenarios check each answer the agent gets. Unix only: the link needs no Developer
//! Mode there.
#![cfg(unix)]

use clauseo_protocol::{DomainEvent, NoticeKind, StopReason, StreamId};

use crate::observe::Events;

use crate::permissions::answer_from;
use crate::support::{Dir, Scenario};
use crate::write;

const CODEX: &str = "codex-acp";

#[test]
fn a_codex_command_into_another_matter_is_refused_before_the_card() {
    let dir = Dir::new(
        "codex_precard_paths",
        &[(CODEX, Scenario::Local("codex-precard-paths"))],
    );
    let host = dir.start(0);
    let other = dir.folder("Other", &[("brief.txt", "the other client's brief\n")]);
    host.create_matter("Other", &other);
    let folder = dir.folder("m", &[("a.txt", "a\n")]);
    write::symlink(&other.join("brief.txt"), &folder.join("notes.txt"));
    write::symlink(&other, &folder.join("link"));
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Copy the brief.");
    answer_from(&host, &mut events, thread, 0, &["allow_once"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // Only the plain copy reached the lawyer.
    assert_eq!(events.permission_requests().len(), 1, "{}", events.dump());
    let kinds: Vec<_> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.kind.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [Some(NoticeKind::OutsideMatterBlocked)],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// The option ids of the first logged permission request.
fn offered(events: &Events) -> Vec<String> {
    events
        .events()
        .find_map(|event| match event {
            DomainEvent::PermissionRequested(asked) => Some(
                asked
                    .options
                    .iter()
                    .map(|option| option.option_id.to_string())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// A `multi_agent` child's approvals, as codex-acp sends them when Clauseo advertises no
/// subagents (under the parent's session id, with the child's own call id that no `tool_call`
/// announced), are judged as the parent's would be: a plain command reaches the card with only
/// allow once and cancel (no amendment, no session option); a file change of a document Word
/// holds open is refused for the lawyer (logged, never asked); a command reaching another matter
/// is refused unseen.
#[test]
fn a_codex_child_card_goes_through_the_same_checks() {
    let dir = Dir::new(
        "codex_child_card",
        &[(CODEX, Scenario::Local("codex-child-card"))],
    );
    let host = dir.start(0);
    let other = dir.folder("Other", &[("brief.txt", "the other client's brief\n")]);
    host.create_matter("Other", &other);
    let folder = dir.folder(
        "m",
        &[
            ("sub/a.txt", "a\n"),
            ("Invoice.docx", "one"),
            ("~$nvoice.docx", "owner file: open in Word"),
        ],
    );
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Check the other folder.");
    answer_from(&host, &mut events, thread, 0, &["allow_once"]);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    // The plain one, then the held one, logged with its refusal; the other-matter one unseen.
    assert_eq!(events.permission_requests().len(), 2, "{}", events.dump());
    assert_eq!(
        offered(&events),
        ["allow_once", "cancel"],
        "{}",
        events.dump()
    );
    let held: Vec<_> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionResolved(resolved) => resolved.held_open.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(
        held,
        [clauseo_protocol::MatterPath::new("Invoice.docx").unwrap()],
        "{}",
        events.dump()
    );
    host.shutdown();
}
