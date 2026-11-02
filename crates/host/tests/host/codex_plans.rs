//! Codex's plans through the host.

use clauseo_protocol::{DomainEvent, PlanStatus, StopReason, StreamId};

use crate::codex::{CODEX, codex_dir, run_turn};

/// Codex's checklist plan arrives as a standard `plan` update and shows: each update is a
/// `PlanReplaced` with every step. Its markdown proposed plan logs nothing (no protocol type).
#[test]
fn codex_plans_show() {
    let dir = codex_dir("codex_plans_show", "codex-plan");
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = host.create_thread(matter, CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(turn).unwrap().stop, StopReason::EndTurn);
    assert_eq!(events.turn_text(turn), "Planned.");
    let plans: Vec<Vec<(String, PlanStatus)>> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::PlanReplaced(plan) => Some(
                plan.entries
                    .iter()
                    .map(|entry| (entry.content.clone(), entry.status))
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    let step = |content: &str, status| (content.to_owned(), status);
    assert_eq!(
        plans,
        [
            vec![
                step("Read the lease", PlanStatus::InProgress),
                step("Draft clause 4", PlanStatus::Pending),
            ],
            vec![
                step("Read the lease", PlanStatus::Completed),
                step("Draft clause 4", PlanStatus::InProgress),
            ],
        ],
        "{}",
        events.dump()
    );
    host.shutdown();
}
