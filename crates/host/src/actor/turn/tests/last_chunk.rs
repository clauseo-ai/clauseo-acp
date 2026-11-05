//! The agent writes a chunk and exits before the host has read it: the chunk is logged before
//! the turn fails, however late the host gets round to its stdout.

use clauseo_protocol::DomainEvent;

use super::agents::{send_prompt, until_settled};
use super::rig::{Rig, kind};
use crate::actor::turn::TurnPhase;
use crate::agent::PUMP_END_BOUND;
use crate::agent::hold::ConnectionHold;

/// The agent answers the prompt with one chunk and exits 1 without a response.
const CHUNK_THEN_EXIT: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"update":{"update":{"sessionUpdate":"agent_message_chunk","messageId":"m-last","content":{"type":"text","text":"The last words."}}}}
{"exit":{"code":1}}
"#;

/// The lines the host writes to the agent's stdin up to the prompt: `initialize`,
/// `session/new` and `session/prompt`.
const LINES_TO_THE_PROMPT: usize = 3;

/// The connection task's wakes are held from the prompt on (the host thread has not got round
/// to that task: neither to the chunk's wake nor to the one the owner's mark sends) while the
/// owner sees the exit and its bound passes, as the window scenarios' clock mover moved the
/// clock 5 s at once (a launch check's timer) and lost `claude-exit-mid-turn`'s only chunk.
/// Before the owner checked that the agent's output was all posted, it posted `Ended` then: the
/// turn failed `agent.exited` with no `TextAppended`, and the chunk came after the agent was
/// gone.
#[test]
fn a_chunk_read_after_the_exit_is_logged_before_the_turn_fails() {
    let mut rig = Rig::start("turn-last-chunk", CHUNK_THEN_EXIT);
    let thread = rig.thread();
    let hold = ConnectionHold::after_written(LINES_TO_THE_PROMPT);
    let turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the prompt", |actor| {
        actor.threads[&thread]
            .turn
            .current
            .as_ref()
            .is_some_and(|current| current.phase == TurnPhase::Prompting)
    });
    let clock = rig.clock.clone();
    rig.run_tasks_until("the held chunk and the owner's wait for the pipes", async {
        let bound = async { while clock.next_armed().await != PUMP_END_BOUND {} };
        futures::future::join(hold.wake_held(), bound).await;
    });
    rig.clock.advance(PUMP_END_BOUND);
    rig.run_tasks_until_stalled();
    hold.release();
    until_settled(&mut rig, thread);
    let log = rig.log(thread);
    let kinds = || log.iter().map(kind).collect::<Vec<_>>();
    let text = log.iter().position(|event| {
        matches!(event, DomainEvent::TextAppended(appended) if appended.turn == turn
            && appended.text == "The last words.")
    });
    let failed = log.iter().position(|event| {
        matches!(event, DomainEvent::TurnFailed(failed) if failed.turn == turn
            && failed.code == clauseo_protocol::ErrorCode::AgentExited)
    });
    assert!(text.is_some(), "the chunk was lost: {:?}", kinds());
    assert!(text < failed, "{:?}", kinds());
    rig.finish();
}
