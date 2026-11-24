//! Tests of the handover: T3's budget arithmetic and selection, the rendering, and the items
//! the log gives.

use clauseo_protocol::{
    ChangeKind, CheckpointId, ContentBlock, ContentId, Count, Delivery, DomainEvent, Envelope,
    ErrorCode, EventId, FileChange, FilesChanged, MatterPath, MessageId, RestoreScope, Seq,
    StepUndone, StopReason, StreamId, TextAppended, TextKind, TextOrBlob, ThreadId,
    ToolCallContent, ToolCallId, ToolCallPatch, ToolCallStarted, ToolCallStatus, ToolCallUpdated,
    ToolKind, TurnEnded, TurnFailed, TurnId, TurnStarted, UserMessage, VersionRestored,
};

use super::*;
use crate::pipeline::tests::support::{id, ts};

const AGENT: &str = "Claude";

fn lawyer(text: &str) -> Item {
    Item::Lawyer(text.to_owned())
}

fn answer(text: &str) -> Item {
    Item::Answer {
        by: AGENT.to_owned(),
        text: text.to_owned(),
    }
}

fn papers(paths: &[&str]) -> Item {
    Item::Papers(paths.iter().map(|path| (*path).to_owned()).collect())
}

/// What `build` counts for `item`: its text and the blank line before it.
fn cost(item: &Item) -> i64 {
    SEPARATOR + size(&render(item))
}

/// The header at the width `build` reserves for `total` items.
fn reserve(total: usize) -> i64 {
    size(&header(AGENT, total, total, total))
}

#[test]
fn budget_is_the_window_less_native_use_request_and_work_reserve() {
    // No window: 128,000. Reserve max(16,000, 32,000) = 32,000, so 128,000 - 90,000 - 2,000 -
    // 32,000 = 4,000.
    assert_eq!(budget(None, 90_000, 2_000), 4_000);
    // 200,000 - 140,000 - 1,000 - 50,000 (a quarter of the window) = 9,000.
    assert_eq!(budget(Some(200_000), 140_000, 1_000), 9_000);
    // Reserve max(16,000, 8,000) = 16,000: 32,000 - 10,000 - 2,000 - 16,000 = 4,000.
    assert_eq!(budget(Some(32_000), 10_000, 2_000), 4_000);
}

#[test]
fn budget_reserve_for_work_is_at_most_half_the_window() {
    // A quarter is 4,000 and the floor 16,000, but half the window (8,000) is the most kept.
    assert_eq!(budget(Some(16_000), 0, 0), 8_000);
}

#[test]
fn budget_is_zero_once_the_window_is_spent() {
    assert_eq!(budget(Some(100_000), 90_000, 0), 0);
    assert_eq!(budget(None, 200_000, 0), 0);
}

#[test]
fn budget_is_capped_at_16_000_tokens_for_a_huge_window() {
    // The token cap binds below the 64,000-byte cap.
    assert_eq!(budget(Some(u64::MAX), 0, 0), 16_000);
    assert_eq!(budget(None, 0, 0), 16_000);
}

#[test]
fn selection_takes_the_three_priority_items_when_only_they_fit() {
    let items = vec![
        lawyer("first question"),
        answer("first answer"),
        lawyer("middle question"),
        answer("middle answer"),
        lawyer("last question"),
        answer("last answer"),
    ];
    // Exactly the header plus the first lawyer message, the last lawyer message and the last
    // answer: each middle item is then too big for what is left.
    let budget = reserve(6) + cost(&items[0]) + cost(&items[4]) + cost(&items[5]);
    let handover = build(&items, AGENT, budget).expect("the priority items fit");
    assert_eq!(handover.included, 3);
    assert_eq!(handover.left_out, 3);
    let expected = format!(
        "{}\n\n[Earlier: the lawyer]\nfirst question\n\n[Earlier: the lawyer]\nlast question\n\n\
         [Earlier: Claude's answer]\nlast answer",
        header(AGENT, 3, 6, 3)
    );
    assert_eq!(handover.text, expected);
}

#[test]
fn selection_drops_an_item_that_does_not_fit_and_keeps_a_smaller_older_one() {
    let big = "x".repeat(1_000);
    let items = vec![
        Item::Command {
            by: AGENT.to_owned(),
            command: "ls".to_owned(),
            exit_code: None,
            output: "ok".to_owned(),
        },
        papers(&[big.as_str()]),
        lawyer("first question"),
        answer("last answer"),
        lawyer("last question"),
    ];
    // The priority items go first: the last question, the last answer, the first question. The
    // walk from the newest then skips the big papers, and takes the older command, which fits
    // to the byte.
    let budget = reserve(5) + cost(&items[0]) + cost(&items[2]) + cost(&items[3]) + cost(&items[4]);
    let handover = build(&items, AGENT, budget).expect("the items fit");
    assert_eq!(handover.included, 4);
    assert_eq!(handover.left_out, 1);
    assert_eq!(
        handover.text,
        format!(
            "{}\n\n[Earlier: a command Claude ran]\nCommand: ls\nok\n\n\
             [Earlier: the lawyer]\nfirst question\n\n[Earlier: Claude's answer]\nlast answer\n\n\
             [Earlier: the lawyer]\nlast question",
            header(AGENT, 4, 5, 1)
        )
    );
}

#[test]
fn selection_keeps_the_log_order_when_everything_fits() {
    let items = vec![
        lawyer("one"),
        answer("two"),
        Item::Error("three".to_owned()),
        lawyer("four"),
        papers(&["five.docx"]),
        Item::Restored,
    ];
    let handover = build(&items, AGENT, 16_000).expect("everything fits");
    assert_eq!(handover.included, 6);
    assert_eq!(handover.left_out, 0);
    let order: Vec<usize> = ["one", "two", "three", "four", "five.docx", "restored to an"]
        .iter()
        .map(|needle| {
            handover
                .text
                .find(needle)
                .expect("each item is in the text")
        })
        .collect();
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
}

#[test]
fn byte_cap_holds_whatever_budget_is_passed() {
    // 60 lawyer messages of 2,000 bytes: 120,000 bytes, well over the cap.
    let long = "q".repeat(2_000);
    let items: Vec<Item> = (0..60).map(|_| lawyer(&long)).collect();
    assert!(budget(Some(u64::MAX), 0, 0) <= BYTE_CAP);
    let handover = build(&items, AGENT, 1_000_000).expect("some items fit");
    assert!(handover.text.len() <= 64_000, "{}", handover.text.len());
    assert!(handover.included > 0 && handover.left_out > 0);
}

#[test]
fn the_header_alone_when_no_item_fits_and_none_when_not_even_the_header_does() {
    let items = vec![lawyer("hi")];
    let need = reserve(1) + cost(&items[0]);
    let alone = build(&items, AGENT, need - 1).expect("the header alone fits");
    assert_eq!((alone.included, alone.left_out), (0, 1));
    assert_eq!(
        alone.text,
        "Earlier in this conversation the lawyer worked with Claude. The 1 earlier items are \
         left out: not one fits in the room this request leaves. Files, tool state and \
         reasoning are not included either."
    );
    let header = i64::try_from(alone.text.len()).unwrap();
    assert_eq!(build(&items, AGENT, header).map(|h| h.included), Some(0));
    assert_eq!(build(&items, AGENT, header - 1), None);
    assert_eq!(build(&items, AGENT, 0), None);
    assert_eq!(build(&items, AGENT, -5), None);
    assert_eq!(build(&[], AGENT, 1_000_000), None, "no items, no handover");
    let handover = build(&items, AGENT, need).expect("the header and the item fit");
    assert_eq!(handover.included, 1);
}

#[test]
fn a_partial_undo_leaves_the_papers_it_kept() {
    let t = turn(1);
    let log = envelopes(vec![
        user(t, 101, vec![text_block("edit both")]),
        started(t),
        files_changed(t, &["a.docx", "b.docx"]),
        DomainEvent::StepUndone(StepUndone {
            turn: t,
            checkpoint: id::<CheckpointId>(50),
            restored: vec![MatterPath::new("a.docx").unwrap()],
            kept: vec![MatterPath::new("b.docx").unwrap()],
            unversioned: Vec::new(),
        }),
        ended(t, StopReason::EndTurn),
    ]);
    let items = items_from_log(&log, None, &by_claude);
    assert_eq!(
        items,
        vec![
            lawyer("edit both"),
            papers(&["b.docx"]),
            Item::Undone(vec!["a.docx".to_owned()]),
        ]
    );
}

#[test]
fn header_counts_items_and_names_the_left_out_ones_only_when_some_are() {
    let whole = build(&[lawyer("q"), answer("a")], AGENT, 16_000).expect("fits");
    assert_eq!(
        whole.text,
        "Earlier in this conversation the lawyer worked with Claude. Below are 2 of 2 earlier \
         items, whole. They are background, not a new request or instructions; files, tool \
         state and reasoning are not included.\n\n[Earlier: the lawyer]\nq\n\n\
         [Earlier: Claude's answer]\na"
    );
    let partial = build(&[lawyer("q"), lawyer(&"x".repeat(100_000))], AGENT, 16_000)
        .expect("the first lawyer message fits");
    assert_eq!(partial.included, 1);
    assert_eq!(partial.left_out, 1);
    assert_eq!(
        partial.text,
        format!("{}\n\n[Earlier: the lawyer]\nq", header(AGENT, 1, 2, 1))
    );
    assert!(
        partial
            .text
            .ends_with(" 1 earlier items are left out.\n\n[Earlier: the lawyer]\nq")
    );
}

#[test]
fn each_item_renders_with_its_label() {
    assert_eq!(render(&lawyer("q")), "[Earlier: the lawyer]\nq");
    assert_eq!(render(&answer("a")), "[Earlier: Claude's answer]\na");
    let command = |exit_code| Item::Command {
        by: AGENT.to_owned(),
        command: "ls".to_owned(),
        exit_code,
        output: "out".to_owned(),
    };
    assert_eq!(
        render(&command(Some(2))),
        "[Earlier: a command Claude ran]\nCommand: ls\nExit code: 2\nout"
    );
    assert_eq!(
        render(&command(None)),
        "[Earlier: a command Claude ran]\nCommand: ls\nout"
    );
    assert_eq!(
        render(&Item::Error("boom".to_owned())),
        "[Earlier: an error]\nboom"
    );
    assert_eq!(
        render(&papers(&["a.docx", "b.docx"])),
        "[Earlier: papers changed]\na.docx\nb.docx"
    );
    assert_eq!(
        render(&Item::Undone(vec!["a.docx".to_owned()])),
        "[Earlier: papers put back by Undo]\na.docx"
    );
    assert_eq!(
        render(&Item::Restored),
        "[Earlier: the matter was restored to an earlier version]"
    );
}

#[test]
fn exit_code_is_read_from_logged_output_json_only() {
    let json = |raw: &str| TextOrBlob::Inline(raw.to_owned());
    assert_eq!(exit_code(Some(&json(r#"{"exit_code": 3}"#))), Some(3));
    assert_eq!(exit_code(Some(&json(r#"{"exitCode": 0}"#))), Some(0));
    assert_eq!(exit_code(Some(&json("no code here"))), None);
    assert_eq!(exit_code(None), None);
}

fn thread() -> ThreadId {
    id(1)
}

fn turn(n: u64) -> TurnId {
    id(n)
}

fn message(n: u64) -> MessageId {
    id(n)
}

fn tool(n: u64) -> ToolCallId {
    id(n)
}

fn text_block(text: &str) -> ContentBlock {
    ContentBlock::Text {
        text: text.to_owned(),
    }
}

fn file_link(name: &str) -> ContentBlock {
    ContentBlock::ResourceLink {
        uri: format!("file:///{name}"),
        name: name.to_owned(),
        mime_type: None,
        title: None,
        description: None,
        size: None,
    }
}

/// Envelopes of the test thread from seq 1, one per event.
fn envelopes(events: Vec<DomainEvent>) -> Vec<Envelope> {
    events
        .into_iter()
        .zip(1_u64..)
        .map(|(event, n)| {
            let stream = StreamId::Thread(thread());
            let seq = Seq::new(n).unwrap();
            Envelope::new(id::<EventId>(1_000 + n), stream, seq, ts(0), None, event)
        })
        .collect()
}

fn user(turn_id: TurnId, message_id: u64, content: Vec<ContentBlock>) -> DomainEvent {
    DomainEvent::UserMessage(UserMessage {
        turn: turn_id,
        message: message(message_id),
        acp_message_id: None,
        content,
        copy_of: None,
        delivery: Delivery::Queue,
        config: std::collections::BTreeMap::new(),
        agent: None,
    })
}

fn started(turn_id: TurnId) -> DomainEvent {
    DomainEvent::TurnStarted(TurnStarted {
        turn: turn_id,
        checkpoint_before: id::<CheckpointId>(30),
        model: None,
        effort: None,
    })
}

fn chunk(turn_id: TurnId, message_id: u64, kind: TextKind, text: &str) -> DomainEvent {
    DomainEvent::TextAppended(TextAppended {
        turn: turn_id,
        message: message(message_id),
        acp_message_id: None,
        kind,
        text: text.to_owned(),
    })
}

fn tool_started(
    turn_id: TurnId,
    call: u64,
    title: &str,
    kind: ToolKind,
    raw_input: Option<&str>,
    raw_output: Option<&str>,
    content: Vec<ToolCallContent>,
) -> DomainEvent {
    DomainEvent::ToolCallStarted(Box::new(ToolCallStarted {
        turn: turn_id,
        tool_call: tool(call),
        acp_id: format!("t{call}").into(),
        title: title.into(),
        kind,
        tool_name: None,
        status: ToolCallStatus::Pending,
        locations: Vec::new(),
        raw_input: raw_input.map(|raw| TextOrBlob::Inline(raw.to_owned())),
        raw_output: raw_output.map(|raw| TextOrBlob::Inline(raw.to_owned())),
        content,
        helper: None,
    }))
}

fn file(path: &str) -> FileChange {
    FileChange::new(
        MatterPath::new(path).unwrap(),
        ChangeKind::Modified,
        ContentId::new("ab12").ok(),
        ContentId::new("cd34").ok(),
        Count::new(2_048).unwrap(),
    )
    .unwrap()
}

fn files_changed(turn_id: TurnId, paths: &[&str]) -> DomainEvent {
    DomainEvent::FilesChanged(FilesChanged {
        turn: turn_id,
        files: paths.iter().map(|path| file(path)).collect(),
        skipped: Vec::new(),
        beside: Vec::new(),
    })
}

fn ended(turn_id: TurnId, stop: StopReason) -> DomainEvent {
    DomainEvent::TurnEnded(TurnEnded {
        turn: turn_id,
        stop,
        usage: None,
        checkpoint_after: None,
        code_mode: false,
        cause: None,
    })
}

/// Two finished turns, one with a command, a thought, a non-command tool call, a file link, an
/// image and changed files; the second with a command and an undone step; a third that failed;
/// and a fourth still running.
fn conversation() -> Vec<Envelope> {
    let (t1, t2, t3, t4) = (turn(1), turn(2), turn(3), turn(4));
    envelopes(vec![
        user(
            t1,
            101,
            vec![text_block("first question"), file_link("lease.docx")],
        ),
        started(t1),
        chunk(t1, 201, TextKind::Thought, "thinking"),
        chunk(t1, 202, TextKind::Answer, "Clause 4 "),
        chunk(t1, 202, TextKind::Answer, "is void."),
        tool_started(
            t1,
            301,
            "`ls`",
            ToolKind::Execute,
            Some(r#"{"command":"ls"}"#),
            Some(r#"{"exit_code":0}"#),
            vec![ToolCallContent::Text {
                text: TextOrBlob::Inline("a.docx\nb.docx".to_owned()),
            }],
        ),
        tool_started(t1, 302, "Read", ToolKind::Read, None, None, Vec::new()),
        DomainEvent::ToolCallUpdated(Box::new(ToolCallUpdated {
            tool_call: tool(302),
            patch: ToolCallPatch {
                title: Some("Read lease.docx".into()),
                ..ToolCallPatch::default()
            },
        })),
        files_changed(t1, &["lease.docx"]),
        ended(t1, StopReason::EndTurn),
        user(
            t2,
            102,
            vec![
                text_block("second question"),
                ContentBlock::Image {
                    data: TextOrBlob::Inline("iVBOR".to_owned()),
                    mime_type: "image/png".into(),
                    uri: None,
                },
            ],
        ),
        started(t2),
        chunk(t2, 203, TextKind::Answer, "Second answer."),
        tool_started(
            t2,
            303,
            "rm draft",
            ToolKind::Execute,
            None,
            None,
            Vec::new(),
        ),
        files_changed(t2, &["draft.docx"]),
        DomainEvent::StepUndone(StepUndone {
            turn: t2,
            checkpoint: id::<CheckpointId>(50),
            restored: vec![MatterPath::new("draft.docx").unwrap()],
            kept: Vec::new(),
            unversioned: Vec::new(),
        }),
        ended(t2, StopReason::EndTurn),
        user(t3, 103, vec![text_block("third question")]),
        started(t3),
        DomainEvent::TurnFailed(TurnFailed {
            turn: t3,
            code: ErrorCode::AgentServiceError,
            detail: Some("the agent service stopped".to_owned()),
        }),
        ended(t3, StopReason::Failed),
        user(t4, 104, vec![text_block("still running")]),
    ])
}

fn by_claude(_: TurnId) -> String {
    AGENT.to_owned()
}

#[test]
fn log_items_cover_every_ended_turn_in_order() {
    let items = items_from_log(&conversation(), None, &by_claude);
    let expected = vec![
        lawyer("first question\nlease.docx"),
        answer("Clause 4 is void."),
        Item::Command {
            by: AGENT.to_owned(),
            command: "ls".to_owned(),
            exit_code: Some(0),
            output: "a.docx\nb.docx".to_owned(),
        },
        papers(&["lease.docx"]),
        lawyer("second question"),
        answer("Second answer."),
        Item::Command {
            by: AGENT.to_owned(),
            command: "rm draft".to_owned(),
            exit_code: None,
            output: String::new(),
        },
        Item::Undone(vec!["draft.docx".to_owned()]),
        lawyer("third question"),
        Item::Error("the agent service stopped".to_owned()),
    ];
    assert_eq!(items, expected);
}

#[test]
fn log_items_after_a_turn_leave_out_the_turns_before_it() {
    let items = items_from_log(&conversation(), Some(turn(1)), &by_claude);
    assert_eq!(items.first(), Some(&lawyer("second question")));
    assert_eq!(items.len(), 6);
    assert_eq!(
        items.last(),
        Some(&Item::Error("the agent service stopped".to_owned()))
    );
}

#[test]
fn log_items_after_a_turn_the_log_never_ended_cover_everything() {
    let all = items_from_log(&conversation(), None, &by_claude);
    assert_eq!(
        items_from_log(&conversation(), Some(turn(9)), &by_claude),
        all
    );
}

#[test]
fn log_items_of_a_restore_fall_where_it_was_logged() {
    let (t1, t2) = (turn(1), turn(2));
    let restore = DomainEvent::VersionRestored(Box::new(VersionRestored {
        undoes: None,
        from: id::<CheckpointId>(40),
        checkpoint: id::<CheckpointId>(41),
        scope: RestoreScope::Files {
            paths: vec![MatterPath::new("lease.docx").unwrap()],
        },
        written: vec![MatterPath::new("lease.docx").unwrap()],
        removed: Vec::new(),
        deleted: Vec::new(),
        more: None,
    }));
    let log = envelopes(vec![
        user(t1, 101, vec![text_block("first")]),
        ended(t1, StopReason::EndTurn),
        restore,
        user(t2, 102, vec![text_block("second")]),
        ended(t2, StopReason::EndTurn),
    ]);
    assert_eq!(
        items_from_log(&log, None, &by_claude),
        vec![lawyer("first"), Item::Restored, lawyer("second")]
    );
    assert_eq!(
        items_from_log(&log, Some(t1), &by_claude),
        vec![Item::Restored, lawyer("second")]
    );
}

#[test]
fn log_items_of_a_turn_that_has_not_ended_are_left_out() {
    let log = envelopes(vec![
        user(turn(1), 101, vec![text_block("running")]),
        chunk(turn(1), 202, TextKind::Answer, "half an answer"),
    ]);
    assert_eq!(items_from_log(&log, None, &by_claude), Vec::new());
}
