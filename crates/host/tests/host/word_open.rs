//! A permission request to write a file of the matter Word has open is refused for the lawyer:
//! the agent gets the reject-once option at once, and the log holds the request and its
//! resolution, naming the file. Requests for other files, for files outside the matter, and
//! naming no file wait for the lawyer as before.

use clauseo_protocol::{
    DomainEvent, MatterPath, PermissionChoice, PermissionKind, PermissionRequestId,
    PermissionResolved, StopReason, StreamId,
};

use crate::observe::Events;
use crate::permissions::answer_from;
use crate::support::{Dir, Scenario, matter_with_thread};

/// The agent id mapped to the scenario.
const AGENT: &str = "word-agent";

/// The requests of `permission-word-open` the host answers itself; the rest wait for the lawyer.
const REFUSED: usize = 6;

/// The resolution of `request`, which must have exactly one.
fn resolution(events: &Events, request: PermissionRequestId) -> PermissionResolved {
    let found: Vec<&PermissionResolved> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PermissionResolved(resolved) if resolved.request == request => {
                Some(resolved)
            }
            _ => None,
        })
        .collect();
    let [resolved] = found.as_slice() else {
        panic!("{request}: {} resolutions: {}", found.len(), events.dump());
    };
    (*resolved).clone()
}

/// The resolution of `request` follows its request directly, so one commit logged both.
fn resolved_with_its_request(events: &Events, request: PermissionRequestId) -> bool {
    let asked = events.position(
        |event| matches!(event, DomainEvent::PermissionRequested(r) if r.request == request),
    );
    let resolved = events.position(
        |event| matches!(event, DomainEvent::PermissionResolved(r) if r.request == request),
    );
    matches!((asked, resolved), (Some(asked), Some(resolved)) if resolved == asked + 1)
}

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

fn selected(option: &str, kind: PermissionKind) -> PermissionChoice {
    PermissionChoice::Selected {
        option_id: option.into(),
        kind,
    }
}

/// `permission-word-open`: Word's owner files sit beside `Invoice.docx` (`~$nvoice.docx`,
/// the 7-character rule) and `sub/Brief.docx`, and beside `Invoice.docx` in two folders next to
/// the matter (`outside`, and `m-elsewhere`, whose name starts with the matter's). The first
/// [`REFUSED`] requests are answered without the lawyer (the scenario's `expect_response` lines
/// check what the agent got); the rest only by the lawyer, whose `AnswerPermission` is refused
/// for a request that is no longer pending.
#[test]
fn write_to_a_document_word_has_open_is_refused() {
    let dir = Dir::new(
        "write_to_a_document_word_has_open_is_refused",
        &[(AGENT, Scenario::Bundled("permission-word-open"))],
    );
    let host = dir.start(0);
    let owner = "owner file: open in Word";
    let (_, thread, _) = matter_with_thread(
        &dir,
        &host,
        "m",
        &[
            ("Invoice.docx", "one"),
            ("~$nvoice.docx", owner),
            ("sub/Brief.docx", "brief"),
            ("sub/~$Brief.docx", owner),
            ("Free.docx", "free"),
        ],
        AGENT,
    );
    dir.folder(
        "outside",
        &[("Invoice.docx", "x"), ("~$nvoice.docx", owner)],
    );
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the invoice and the brief.");
    let lawyer = ["allow", "reject", "allow", "allow"];
    answer_from(&host, &mut events, thread, REFUSED, &lawyer);
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);

    let requests = events.permission_requests();
    assert_eq!(requests.len(), REFUSED + lawyer.len(), "{}", events.dump());
    let got: Vec<(PermissionChoice, Option<MatterPath>)> = requests
        .iter()
        .map(|request| {
            let resolved = resolution(&events, *request);
            (resolved.choice, resolved.held_open)
        })
        .collect();
    assert_eq!(
        got,
        [
            (
                selected("reject", PermissionKind::RejectOnce),
                Some(path("Invoice.docx"))
            ),
            (
                selected("abort", PermissionKind::RejectOnce),
                Some(path("sub/Brief.docx"))
            ),
            (
                selected("abort", PermissionKind::RejectOnce),
                Some(path("sub/Brief.docx"))
            ),
            (PermissionChoice::Cancelled, Some(path("Invoice.docx"))),
            (PermissionChoice::Cancelled, Some(path("Invoice.docx"))),
            (
                selected("reject", PermissionKind::RejectOnce),
                Some(path("Invoice.docx"))
            ),
            (selected("allow", PermissionKind::AllowOnce), None),
            (selected("reject", PermissionKind::RejectOnce), None),
            (selected("allow", PermissionKind::AllowOnce), None),
            (selected("allow", PermissionKind::AllowOnce), None),
        ]
    );
    for request in &requests[..REFUSED] {
        assert!(
            resolved_with_its_request(&events, *request),
            "{request} was not resolved in its own commit: {}",
            events.dump()
        );
    }
    host.shutdown();
}

/// Windows: a document held open with a share mode that denies writing, as Word holds one, and
/// no owner file beside it, is refused the same way.
#[cfg(windows)]
#[test]
fn write_to_a_document_held_open_without_sharing_is_refused() {
    let dir = Dir::new(
        "write_to_a_document_held_open_without_sharing_is_refused",
        &[(AGENT, Scenario::Bundled("permission-share-mode"))],
    );
    let host = dir.start(0);
    let (_, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("Locked.docx", "one")], AGENT);
    let held = crate::write::open_deny_write(&folder.join("Locked.docx"));
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the locked document.");
    let ended = events.until_turn_ended(turn);
    events.assert_no_mismatch();
    assert_eq!(ended.stop, StopReason::EndTurn);
    let requests = events.permission_requests();
    let [request] = requests.as_slice() else {
        panic!("one request expected: {}", events.dump());
    };
    let resolved = resolution(&events, *request);
    assert_eq!(
        (resolved.choice, resolved.held_open),
        (
            selected("reject", PermissionKind::RejectOnce),
            Some(path("Locked.docx"))
        )
    );
    drop(held);
    host.shutdown();
}
