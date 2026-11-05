//! codex-acp's own `logout` (decision record 0048; R20 brief, "codex-acp's own `logout`").
//! codex-acp advertises a `logout` command, which revokes the tokens in the login Codex keeps
//! (research 43). Signing out goes only through Clauseo's own `SignOut`, which knows whether
//! the login is Clauseo's or the lawyer's reused one and runs the leave order, so the host never
//! lets the agent's own command run: [`hide`] drops it from the commands a Codex agent
//! advertises before they are logged as `CommandsAvailable`, and [`refuse`] refuses a prompt
//! to a Codex agent whose text starts `/logout` with `command.invalid`, before anything is
//! queued or sent. Any other agent's commands and prompts pass as they are.
//!
//! S18's guard: while Clauseo's Codex home shares the lawyer's login file (the sign-in hook
//! says so, or gave no answer, or desktop says the login is reused; `agent/login_link.rs`), a
//! prompt starting `/login` is refused too, with `signin.login_shared`: the lawyer changes it
//! with "Stop using it here". The host itself never calls `login` or `authenticate`.

use clauseo_protocol::{AgentKind, AvailableCommand, ContentBlock, ErrorCode, ThreadId};

use super::super::Actor;
use crate::error::Error;

/// The command codex-acp advertises that signs the agent out.
const LOGOUT: &str = "logout";

/// The command that signs the agent in.
const LOGIN: &str = "login";

/// The kind of `thread`'s agent, when the host knows the thread and the agent has one.
fn kind(actor: &Actor, thread: ThreadId) -> Option<AgentKind> {
    let entry = actor.threads.get(&thread)?;
    AgentKind::of(&entry.row.agent)
}

/// `commands` as the window gets them: without `logout` for a Codex agent.
pub(super) fn hide(
    actor: &Actor,
    thread: ThreadId,
    mut commands: Vec<AvailableCommand>,
) -> Vec<AvailableCommand> {
    if kind(actor, thread) == Some(AgentKind::Codex) {
        commands.retain(|command| !is_logout(&command.name));
    }
    commands
}

/// Refuses `blocks`, a prompt to `thread`, with `command.invalid` when its agent is Codex and a
/// text block starts `/logout`, alone or followed by a space.
pub(super) fn refuse(
    actor: &Actor,
    thread: ThreadId,
    blocks: &[ContentBlock],
) -> Result<(), Error> {
    if kind(actor, thread) != Some(AgentKind::Codex) {
        return Ok(());
    }
    let typed = |name: &str| {
        blocks.iter().any(|block| match block {
            ContentBlock::Text { text } => starts_command(text, name),
            ContentBlock::Image { .. }
            | ContentBlock::Audio { .. }
            | ContentBlock::ResourceLink { .. }
            | ContentBlock::Resource { .. }
            | ContentBlock::Unsupported { .. } => false,
        })
    };
    let guarded = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| actor.wiring.sign_in.guards(&entry.row.agent));
    if guarded && typed(LOGIN) {
        return Err(Error::refused(
            ErrorCode::SigninLoginShared,
            "/login is not sent to the agent: Clauseo is using the ChatGPT login on this computer",
        ));
    }
    if typed(LOGOUT) {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            "/logout is not sent to the agent: sign out from Clauseo's Plans instead",
        ));
    }
    Ok(())
}

/// Whether `name` is `logout`, ignoring ASCII case.
fn is_logout(name: &str) -> bool {
    name.eq_ignore_ascii_case(LOGOUT)
}

/// Whether `text`, past leading white space, is the command `name`: a slash, `name` in any
/// case, then the end or white space.
fn starts_command(text: &str, name: &str) -> bool {
    let Some(rest) = text.trim_start().strip_prefix('/') else {
        return false;
    };
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    rest.get(..name_end)
        .is_some_and(|typed| typed.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests;
