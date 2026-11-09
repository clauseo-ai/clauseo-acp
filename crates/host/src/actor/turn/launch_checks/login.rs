//! A Codex launch's sign-in check on the launch-prep thread (`agent/login_link.rs`): the hook
//! and the agent it asks about, and the check run once the rest of the prep succeeded.

use std::sync::Arc;

use clauseo_protocol::{AgentId, AgentKind, MatterId, ThreadId};

use super::super::super::Actor;
use super::Prepared;
use crate::agent::login_link::{self, LoginCheck};
use crate::clock::Clock;
use crate::error::Error;

/// The sign-in hook and the agent it asks about, for a Codex thread when a hook is set.
pub(super) fn hook(
    actor: &Actor,
    thread: ThreadId,
) -> Option<(Arc<dyn LoginCheck + Send + Sync>, AgentId)> {
    let hook = actor.wiring.sign_in.check.clone()?;
    let agent = actor.threads.get(&thread)?.row.agent.clone();
    (AgentKind::of(&agent) == Some(AgentKind::Codex)).then_some((hook, agent))
}

/// A Codex prep's sign-in check (`login`, when a hook is set), run once the rest succeeded; its
/// answer rides in the prep. Any other prep, or a failed one, is `result` as it is.
pub(super) async fn with_login(
    result: Result<Prepared, Error>,
    login: Option<(Arc<dyn LoginCheck + Send + Sync>, AgentId)>,
    clock: Arc<dyn Clock>,
    matter: MatterId,
) -> Result<Prepared, Error> {
    match (result, login) {
        (Ok(mut prepared), Some((hook, agent))) => {
            if let Some(codex) = prepared.codex.as_mut() {
                let home = codex.home.clone();
                let checked = login_link::check(clock, matter, hook, agent, home).await;
                codex.login = Some(checked);
            }
            Ok(prepared)
        }
        (result, _) => result,
    }
}
