//! The host's check of Codex's own input cap before a `ChatGPT` question is sent (decision 0057):
//! a prompt to a Codex thread whose text Codex would refuse (`crate::codex_input`, counted as
//! Codex counts it) is refused with `prompt.too_long`, the words the lawyer gets for Clauseo's
//! own cap, before anything is queued or sent. Codex's refusal itself reaches the wire only as
//! "Codex encountered an internal error.", so it cannot be told apart once sent.
//!
//! It runs where the thread's agent is known: `SendPrompt` (start now, queue, send now, steer)
//! and `EditQueuedPrompt`, on the question alone; and again at step 5 (`prompt.rs`), on the
//! text as it will be sent, since the undo note is added only then and has no length cap. That
//! refusal ends the turn failed with `prompt.too_long` before anything is sent, the queue held.
//! A steer carries no note. Any other agent's prompts pass; Claude's own refusal ends its turn
//! `TooLong`.

use clauseo_protocol::{AgentKind, ContentBlock, ErrorCode, ThreadId};

use super::super::Actor;
use crate::codex_input::{CODEX_MAX_INPUT_CHARS, codex_input_chars};
use crate::error::Error;

/// Refuses `blocks`, a prompt to `thread`, with `prompt.too_long` when its agent is Codex and
/// Codex would refuse its text.
pub(super) fn refuse(
    actor: &Actor,
    thread: ThreadId,
    blocks: &[ContentBlock],
) -> Result<(), Error> {
    let codex = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Codex));
    if !codex {
        return Ok(());
    }
    let chars = codex_input_chars(blocks);
    if chars <= CODEX_MAX_INPUT_CHARS {
        return Ok(());
    }
    Err(Error::refused(
        ErrorCode::PromptTooLong,
        format!(
            "not sent to the agent: Codex counts {chars} characters, over its \
             {CODEX_MAX_INPUT_CHARS}"
        ),
    ))
}
