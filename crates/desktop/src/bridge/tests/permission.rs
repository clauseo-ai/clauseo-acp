//! The permission answers: which options `answer_permission` sends, refuses or confirms first.

use clauseo_protocol::{
    AgentChoiceId, DeliveryRequest, ErrorCode, HostCommand, HostEvent, PermissionKind,
    PermissionRequestId, PermissionState, PromptBlock, ViewEntry, ViewPatchBody,
};
use uuid::Uuid;

use super::{MAIN, block_on};
use crate::bridge::{MODE_OPTIONS, needs_confirm};
use crate::dialogs::{BoxFuture, Confirm};
use crate::test_support::{Messages, Scratch, collector, stream};

/// A confirm that answers `self.0`, and must not be asked when `self.0` is `None`.
struct Answers(Option<bool>);

impl Confirm for Answers {
    fn confirm(&self) -> BoxFuture<'_, bool> {
        let answer = self.0.expect("the confirm was not to be asked");
        Box::pin(async move { answer })
    }
}

/// Reads `messages` until `n` permission requests have shown as pending, recording each once in
/// `seen`, and returns the `n`th.
fn until_pending(
    messages: &Messages,
    seen: &mut Vec<PermissionRequestId>,
    n: usize,
) -> PermissionRequestId {
    while seen.len() < n {
        let text = messages.next_text();
        let Ok(HostEvent::Patches(batch)) = serde_json::from_str::<HostEvent>(&text) else {
            continue;
        };
        for patch in batch.patches {
            let entries = match patch.body {
                ViewPatchBody::Upsert { entry } => vec![*entry],
                ViewPatchBody::Reset { view_window } => view_window.entries,
                _ => Vec::new(),
            };
            for entry in entries {
                if let ViewEntry::Permission(permission) = entry
                    && matches!(permission.state, PermissionState::Pending)
                    && !seen.contains(&permission.request)
                {
                    seen.push(permission.request);
                }
            }
        }
    }
    seen[n - 1]
}

/// `permission-kinds` asks four times and expects allow-once, allow-always, reject-once and
/// reject-always, in that order; it stops on any other answer, so its closing text shows the
/// bridge sent exactly those.
#[test]
fn answer_permission_checks_the_option_and_confirms_always_allow() {
    let scratch = Scratch::with_fake_agent("answer-permission", "permission-kinds");
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let (channel, messages) = collector();
    bridge.open_stream(MAIN, channel);
    block_on(bridge.subscribe(MAIN, stream(thread), None)).unwrap();
    host.run(HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Edit the four clauses.".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
    });
    let answer = |confirm: &dyn Confirm, request, option: &str| {
        let option = AgentChoiceId::new(option).unwrap();
        block_on(bridge.answer_permission(confirm, thread, request, option))
    };
    let mut seen = Vec::new();

    // An option the request did not offer is refused before anything is asked or sent.
    let first = until_pending(&messages, &mut seen, 1);
    let refusal = answer(&Answers(None), first, "allow-forever").unwrap_err();
    assert_eq!(
        refusal.code(),
        ErrorCode::PermissionUnknownOption,
        "{refusal:?}"
    );
    answer(&Answers(None), first, "allow-once").unwrap();

    // Always allow, declined in the confirm: refused, and the request is still pending.
    let second = until_pending(&messages, &mut seen, 2);
    let refusal = answer(&Answers(Some(false)), second, "allow-always").unwrap_err();
    assert_eq!(
        refusal.code(),
        ErrorCode::TransportUnauthorized,
        "{refusal:?}"
    );
    block_on(bridge.pending_permission(thread, second)).unwrap();
    // Accepted: sent.
    answer(&Answers(Some(true)), second, "allow-always").unwrap();
    let refusal = block_on(bridge.pending_permission(thread, second)).unwrap_err();
    assert_eq!(
        refusal.code(),
        ErrorCode::PermissionNotPending,
        "{refusal:?}"
    );

    let third = until_pending(&messages, &mut seen, 3);
    answer(&Answers(None), third, "reject-once").unwrap();
    let fourth = until_pending(&messages, &mut seen, 4);
    answer(&Answers(None), fourth, "reject-always").unwrap();
    while !messages.next_text().contains("Four edits were decided.") {}
}

/// `plan-exit` (a scenario file of this crate, played from its path) offers claude-agent-acp
/// 0.89.1's plan-exit options and expects `default`; it stops on any other answer, so its
/// closing text shows none of the refused ones reached the agent.
#[test]
fn answer_permission_refuses_the_plan_exit_options_that_change_the_mode() {
    let scenario = concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/plan-exit.jsonl");
    let scratch = Scratch::with_fake_agent("plan-exit", scenario);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let (channel, messages) = collector();
    bridge.open_stream(MAIN, channel);
    block_on(bridge.subscribe(MAIN, stream(thread), None)).unwrap();
    host.run(HostCommand::SendPrompt {
        thread,
        content: vec![PromptBlock::Text {
            text: "Plan the edits, then make them.".to_owned(),
        }],
        delivery: DeliveryRequest::Queue,
    });
    let request = until_pending(&messages, &mut Vec::new(), 1);
    for option in MODE_OPTIONS {
        let option = AgentChoiceId::new(option).unwrap();
        // Refused before the confirm: `Answers(None)` panics when asked.
        let refusal = block_on(bridge.answer_permission(&Answers(None), thread, request, option))
            .unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");
        block_on(bridge.pending_permission(thread, request)).unwrap();
    }
    let default = AgentChoiceId::new("default").unwrap();
    block_on(bridge.answer_permission(&Answers(None), thread, request, default)).unwrap();
    while !messages.next_text().contains("Leaving plan mode") {}
}

/// Only the once kinds and `reject_always` skip the confirm; a kind this version does not know
/// (acp refuses such a request today, so no agent can offer one here) is refused.
#[test]
fn the_confirm_fails_closed_on_a_kind_it_does_not_know() {
    assert!(needs_confirm(PermissionKind::AllowAlways).unwrap());
    for kind in [
        PermissionKind::AllowOnce,
        PermissionKind::RejectOnce,
        PermissionKind::RejectAlways,
    ] {
        assert!(!needs_confirm(kind).unwrap(), "{kind:?}");
    }
    let refusal = needs_confirm(PermissionKind::Unknown).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid);
}

#[test]
fn answer_permission_refuses_a_request_that_is_not_pending() {
    let scratch = Scratch::new("permission-not-pending", &[]);
    let host = scratch.start(Vec::new());
    let thread = host.thread();
    let bridge = host.bridge();
    let request = PermissionRequestId::new(Uuid::now_v7()).unwrap();
    let option = AgentChoiceId::new("allow").unwrap();
    let refusal =
        block_on(bridge.answer_permission(&Answers(None), thread, request, option)).unwrap_err();
    assert_eq!(
        refusal.code(),
        ErrorCode::PermissionNotPending,
        "{refusal:?}"
    );
}
