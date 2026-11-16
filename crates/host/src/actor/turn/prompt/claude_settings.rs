//! A live Claude conversation stops when Claude Code's user settings stop being the launch's.
//!
//! Claude Code 2.1.293 reads `<CLAUDE_CONFIG_DIR>/settings.json` under `--setting-sources=user`
//! and merges its arrays, hooks and `env` with Clauseo's flag settings, which cannot take them
//! out (`agent::loads::pinned`). The launch writes that file with Clauseo's keys only
//! (`agent::home::write_claude_settings`), but Claude Code also re-reads it during the session: a
//! file watcher picks a change up within about a second (a 2 s poll as its fallback) and rebuilds
//! the permission rules and the sandbox, and hooks reload. So:
//!
//! - **Before each prompt** ([`check`], off the actor, beside the Codex `.codex` re-check): the
//!   file must still say what the launch wrote, give or take the few keys Claude Code writes
//!   there itself with safe values (`agent::home::claude_settings_intact`), and no policy may
//!   weaken the launch (the per-user Windows one, the firm's, its server's:
//!   `profile_loads::policy::user_policy_refusal`, on the actor, [`policy_refusal`]). Either
//!   failing stops the conversation: the turn fails (`agent.settings_changed`, or
//!   `agent.user_policy` or `agent.firm_policy_weakens`, the detail naming the setting) and the
//!   agent is stopped, so the next message launches it again, and that launch rewrites the file.
//! - **While the prompt runs** ([`watched`]): both checks again every [`POLL`], raced against
//!   the prompt call, so a change made mid-turn (by a tool the agent ran, or anything else)
//!   stops the turn within about two seconds, the same way. Claude Code may already have applied
//!   the change in that window: the accepted residual (it re-reads the user policy only every 30
//!   minutes). Between turns nothing watches; the next prompt's checks cover that time.
//!
//! Each file check runs on a short-lived thread of its own (`codex::prepare::off_actor`): a file
//! swapped for a FIFO or a slow disk could block the read, never the actor. The policy poll
//! (`policy::user_policy_reader`) runs on the actor's pool: a registry value and a few small
//! managed-settings files.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{ErrorCode, MatterId, ThreadId};
use futures::future::{Either, select};

use super::super::super::Actor;
use crate::agent::codex::prepare::{Job, off_actor};
use crate::clock::Clock;
use crate::error::Error;

/// How long a file check's caller waits. 10 s, not the 5 s of `Job::short`, which the turn's
/// cancel ladder also arms (a test moving the clock past a 5 s timer means the cancel's).
const CHECK_WAIT: Duration = Duration::from_secs(10);

/// How often [`watch`] checks the file while a prompt runs: Claude Code's own fallback poll.
pub(in super::super) const POLL: Duration = Duration::from_secs(2);

/// The failure when the file changed. Retryable: the next message relaunches the agent.
pub(super) fn changed() -> Error {
    Error::refused(
        ErrorCode::AgentSettingsChanged,
        "Claude's own settings file changed during the conversation, so Clauseo stopped it; the \
         next message starts Claude again with Clauseo's settings",
    )
}

/// Before a prompt of a Claude agent (`claude`): the refusal of a policy that weakens the launch
/// (the per-user one, the firm's or its server's), if any (`profile_loads::policy`). On the
/// actor; a warn line when it refuses.
pub(super) fn policy_refusal(actor: &Actor, thread: ThreadId, claude: bool) -> Option<Error> {
    if !claude {
        return None;
    }
    let error = super::super::profile_loads::policy::user_policy_refusal(actor, thread)?;
    tracing::warn!(
        %thread,
        "a Claude Code policy weakens the launch: the conversation is stopped before its prompt"
    );
    Some(error)
}

/// Whether the user settings in `home` (a Claude agent's; `None` passes at once) are still the
/// launch's, off the actor: [`changed`] when not, with a warn line.
pub(super) async fn check(
    clock: Arc<dyn Clock>,
    matter: Option<MatterId>,
    home: Option<PathBuf>,
) -> Result<(), Error> {
    let Some(home) = home else {
        return Ok(());
    };
    let job = Job {
        what: "Claude's settings check",
        matter,
        wait: CHECK_WAIT,
    };
    let intact = off_actor(clock, job, move || {
        Ok(crate::agent::home::claude_settings_intact(&home))
    })
    .await?;
    if intact {
        return Ok(());
    }
    tracing::warn!(
        ?matter,
        "Claude's user settings file is no longer the launch's: the conversation is stopped"
    );
    Err(changed())
}

/// The checks [`watched`] repeats: the agent's Claude home and the user policy's reader
/// (`profile_loads::policy::user_policy_reader`); `None` for any other agent.
pub(super) type Watch<P> = Option<(PathBuf, P)>;

/// What [`watched`] polls for a prompt of the agent with Claude home `home`, made on the actor.
pub(super) fn checks(
    actor: &Actor,
    thread: ThreadId,
    home: Option<&PathBuf>,
) -> Watch<impl Fn() -> Option<Error> + 'static> {
    let reader = |home: &PathBuf| {
        let policy = super::super::profile_loads::policy::user_policy_reader(actor, thread);
        (home.clone(), policy)
    };
    home.map(reader)
}

/// `call` (the prompt call) raced against [`watch`]: the call's own outcome, or the watch's
/// error, which drops the call.
pub(super) async fn watched<T, P: Fn() -> Option<Error>>(
    call: impl std::future::Future<Output = T>,
    clock: Arc<dyn Clock>,
    matter: Option<MatterId>,
    checks: Watch<P>,
) -> Result<T, Error> {
    let watch = watch(clock, matter, checks);
    match select(std::pin::pin!(call), std::pin::pin!(watch)).await {
        Either::Left((done, _)) => Ok(done),
        Either::Right((error, _)) => Err(error),
    }
}

/// The user policy's refusal and [`check`], every [`POLL`] until one fails, then its error;
/// never resolves for an agent with no Claude home. A file check that does not answer in time
/// ends the turn too, with its own error (it fails closed).
async fn watch<P: Fn() -> Option<Error>>(
    clock: Arc<dyn Clock>,
    matter: Option<MatterId>,
    checks: Watch<P>,
) -> Error {
    let Some((home, policy)) = checks else {
        return std::future::pending().await;
    };
    loop {
        clock.after(POLL).await;
        if let Some(error) = policy() {
            tracing::warn!(
                ?matter,
                "a user-writable Claude Code policy appeared during a turn: the conversation is \
                 stopped"
            );
            return error;
        }
        if let Err(error) = check(Arc::clone(&clock), matter, Some(home.clone())).await {
            return error;
        }
    }
}
