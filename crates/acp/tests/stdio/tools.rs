//! Tool calls: kinds, statuses, names, locations and content.

use clauseo_acp::{AgentUpdate, MessageRole, Origin, ToolCall};
use clauseo_protocol::{
    StopReason, TextOrBlob, ToolCallContent, ToolCallPatch, ToolCallStatus, ToolKind, ToolLocation,
};

use crate::harness::{self, Ctx, chunk, inside, sess1, text, turn};

/// A `tool_call` with no content and no raw input or output.
fn call(
    id: &str,
    title: &str,
    name: &str,
    kind: ToolKind,
    locations: Vec<ToolLocation>,
) -> AgentUpdate {
    AgentUpdate::ToolCall(ToolCall {
        acp_id: id.into(),
        title: title.into(),
        kind,
        tool_name: Some(name.into()),
        status: ToolCallStatus::Pending,
        locations,
        raw_input: None,
        raw_output: None,
        content: Vec::new(),
    })
}

/// A `tool_call_update` that changes its status and, when given, replaces its content.
fn patch(id: &str, status: ToolCallStatus, content: Option<Vec<ToolCallContent>>) -> AgentUpdate {
    AgentUpdate::ToolCallUpdate {
        acp_id: id.into(),
        patch: ToolCallPatch {
            status: Some(status),
            content,
            ..ToolCallPatch::default()
        },
    }
}

/// Text content of a tool call.
fn said(text: &str) -> ToolCallContent {
    ToolCallContent::Text {
        text: TextOrBlob::Inline(text.to_owned()),
    }
}

/// The first five tool calls of `tool-calls`: each with its start, its progress and its end.
fn file_calls(t1: Origin) -> Vec<(Origin, AgentUpdate)> {
    use ToolCallStatus::{Completed, InProgress};
    let (read, edit) = ("tc-read-1", "tc-edit-1");
    vec![
        (
            t1,
            call(
                read,
                "Read brief.txt",
                "Read",
                ToolKind::Read,
                vec![inside("brief.txt")],
            ),
        ),
        (t1, patch(read, InProgress, None)),
        (
            t1,
            patch(read, Completed, Some(vec![said("Read 42 lines.")])),
        ),
        (
            t1,
            call(
                edit,
                "Edit brief.txt",
                "Edit",
                ToolKind::Edit,
                vec![inside("brief.txt")],
            ),
        ),
        (t1, patch(edit, InProgress, None)),
        (
            t1,
            patch(
                edit,
                Completed,
                Some(vec![ToolCallContent::Diff {
                    path: inside("brief.txt"),
                    old: Some(TextOrBlob::Inline("Clause 4 draft\n".to_owned())),
                    new: TextOrBlob::Inline("Clause 4 final\n".to_owned()),
                }]),
            ),
        ),
        (
            t1,
            call(
                "tc-delete-1",
                "Delete old.txt",
                "Delete",
                ToolKind::Delete,
                vec![inside("old.txt")],
            ),
        ),
        (t1, patch("tc-delete-1", InProgress, None)),
        (
            t1,
            patch(
                "tc-delete-1",
                Completed,
                Some(vec![said("Deleted old.txt")]),
            ),
        ),
        (
            t1,
            call(
                "tc-move-1",
                "Move notes/b.txt to archive/b.txt",
                "Move",
                ToolKind::Move,
                vec![inside("notes/b.txt"), inside("archive/b.txt")],
            ),
        ),
        (t1, patch("tc-move-1", InProgress, None)),
        (
            t1,
            patch(
                "tc-move-1",
                Completed,
                Some(vec![said("Moved notes/b.txt to archive/b.txt")]),
            ),
        ),
        (
            t1,
            call(
                "tc-search-1",
                "Search for \"indemnity\"",
                "Grep",
                ToolKind::Search,
                vec![inside("notes")],
            ),
        ),
        (t1, patch("tc-search-1", InProgress, None)),
        (
            t1,
            patch("tc-search-1", Completed, Some(vec![said("3 matches")])),
        ),
    ]
}

/// The other tool calls of `tool-calls`, then the answer that follows them.
fn other_calls(t1: Origin) -> Vec<(Origin, AgentUpdate)> {
    use ToolCallStatus::{Completed, Failed, InProgress};
    vec![
        // The session folder itself is no location, so the list is empty.
        (
            t1,
            call(
                "tc-execute-1",
                "Run the checks",
                "Bash",
                ToolKind::Execute,
                Vec::new(),
            ),
        ),
        (
            t1,
            patch(
                "tc-execute-1",
                InProgress,
                Some(vec![said("Running the checks")]),
            ),
        ),
        (
            t1,
            patch("tc-execute-1", Completed, Some(vec![said("4 passed")])),
        ),
        (
            t1,
            call(
                "tc-think-1",
                "Weigh the cap against the fees",
                "Think",
                ToolKind::Think,
                vec![inside("brief.txt")],
            ),
        ),
        (t1, patch("tc-think-1", InProgress, None)),
        (
            t1,
            patch(
                "tc-think-1",
                Completed,
                Some(vec![said("The cap is below the fees.")]),
            ),
        ),
        (
            t1,
            call(
                "tc-switch-1",
                "Switch to plan mode",
                "SwitchMode",
                ToolKind::SwitchMode,
                Vec::new(),
            ),
        ),
        (t1, patch("tc-switch-1", InProgress, None)),
        (
            t1,
            patch(
                "tc-switch-1",
                Completed,
                Some(vec![said("Switched to plan mode")]),
            ),
        ),
        (
            t1,
            call(
                "tc-fetch-1",
                "Fetch the terms page",
                "WebFetch",
                ToolKind::Fetch,
                vec![inside("terms.html")],
            ),
        ),
        (t1, patch("tc-fetch-1", InProgress, None)),
        (
            t1,
            patch(
                "tc-fetch-1",
                Failed,
                Some(vec![said("Timed out after 30 s")]),
            ),
        ),
        (
            t1,
            call(
                "tc-other-1",
                "Ask the document service",
                "Mcp",
                ToolKind::Other,
                vec![inside("brief.txt")],
            ),
        ),
        (t1, patch("tc-other-1", InProgress, None)),
        (
            t1,
            patch(
                "tc-other-1",
                Completed,
                Some(vec![said("Service answered")]),
            ),
        ),
        (
            t1,
            chunk(MessageRole::Answer, "m-answer-1", "Ran all ten tool calls."),
        ),
    ]
}

#[test]
fn tool_calls_map_kinds_and_content() {
    let outcome = harness::run("tool-calls", async |ctx: Ctx| {
        ctx.handshake().await;
        let end = ctx
            .client
            .prompt(&sess1(), turn(1), text("Run the ten tools"))
            .await
            .unwrap();
        assert_eq!(end.stop, StopReason::EndTurn);
        let t1 = Origin::Turn(turn(1));
        let mut expected = file_calls(t1);
        expected.extend(other_calls(t1));
        assert_eq!(ctx.updates(), expected);
    });
    outcome.assert_clean();
}
