//! The handover's label on a command one of the agent's helpers ran (#1289's helpers): the
//! helper's step is still handed over, under T3's selection and caps, but named as the helper's.

use clauseo_protocol::HelperId;

use super::*;

/// A helper's `Execute` call is labelled as one of the agent's helpers' commands, and the lead
/// agent's own keeps the plain label.
#[test]
fn a_helpers_command_is_labelled_as_the_helpers_and_the_agents_own_keeps_its_label() {
    let t1 = turn(1);
    let mut by_helper = tool_started(t1, 2, "grep", ToolKind::Execute, None, None, Vec::new());
    if let DomainEvent::ToolCallStarted(started) = &mut by_helper {
        started.helper = Some(id::<HelperId>(60));
    }
    let log = envelopes(vec![
        user(t1, 101, vec![text_block("find clause 4")]),
        started(t1),
        tool_started(t1, 1, "ls", ToolKind::Execute, None, None, Vec::new()),
        by_helper,
        ended(t1, StopReason::EndTurn),
    ]);
    let items = items_from_log(&log, None, &by_claude);
    let text = build(&items, AGENT, 16_000).expect("everything fits").text;
    assert!(
        text.contains("[Earlier: a command Claude ran]\n> Command: ls\n"),
        "{text}"
    );
    assert!(
        text.contains("[Earlier: a command one of Claude's helpers ran]\n> Command: grep\n"),
        "{text}"
    );
    assert!(
        !text.contains("[Earlier: a command Claude ran]\n> Command: grep"),
        "{text}"
    );
    // Both are in the handover: a helper's command is selected as the agent's own are.
    assert_eq!(items.len(), 3, "{items:?}");
}
