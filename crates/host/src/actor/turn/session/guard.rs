//! A Codex session's guard work around its start: the session's id written for the guard's
//! session check as soon as `session/new`, `session/load` or `session/resume` returns
//! ([`record_session`], `pre_write_guard/sessions.rs`), and the guard hook's self-test before the
//! first prompt ([`self_test`], `pre_write_guard/self_test.rs`). Either failing fails the session
//! start with `agent.guard_selftest_failed`: the guard would refuse every call of the session, or
//! Codex would run them unchecked.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_acp::SessionStart;
use clauseo_protocol::{ErrorCode, MatterId, ThreadId};

use super::super::super::Actor;
use super::super::{AgentReply, state};
use crate::agent::codex;
use crate::clock::Clock;
use crate::error::Error;
use crate::log_file::Cause;
use crate::pre_write_guard::self_test::SelfTest;

/// The host's clock, the matter and the guard folder, for [`record_session`].
pub(super) type IdsTarget = (Arc<dyn Clock>, Option<MatterId>, PathBuf);

/// Where `thread`'s session id goes for the guard's session check: its run's guard folder, when
/// its agent is Codex with the guard's hook; with the host's clock and the matter, for the write
/// off the actor.
pub(super) fn ids_target(
    actor: &mut Actor,
    thread: ThreadId,
    matter: Option<MatterId>,
) -> Option<IdsTarget> {
    let clock = Arc::clone(&actor.clock);
    let guarded = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .and_then(|agent| agent.codex.as_ref())
        .is_some_and(|codex| codex.guard);
    let run = actor.threads.get(&thread)?.turn.run.as_ref()?;
    let folder = run.guard.as_ref()?.0.parent()?.to_path_buf();
    guarded.then_some((clock, matter, folder))
}

/// After `session/new`, `session/load` or `session/resume` returned and before anything is
/// prompted: the session's id added to the ids file in the guard folder (`sessions::add`), off
/// the actor. A write that fails, or does not finish in time, fails the session start.
async fn record_session(
    ids: Option<IdsTarget>,
    result: &Result<SessionStart, clauseo_acp::Error>,
) -> Result<(), Error> {
    let (Some((clock, matter, folder)), Ok(start)) = (ids, result) else {
        return Ok(());
    };
    let id = start.session.as_str().to_owned();
    let job = codex::prepare::Job::short("the guard's session ids", matter);
    codex::prepare::off_actor(clock, job, move || {
        crate::pre_write_guard::sessions::add(&folder, &id).map_err(|error| {
            tracing::error!(
                error = %Cause(&error),
                "the guard's session ids were not written; the session start fails"
            );
            guard_failed()
        })
    })
    .await
}

/// The reply of a session call that returned `result` (`loaded` for `session/load` and
/// `session/resume`), once [`record_session`] wrote its id; refused when it could not.
pub(super) async fn reply(
    ids: Option<IdsTarget>,
    loaded: bool,
    result: Result<SessionStart, clauseo_acp::Error>,
) -> AgentReply {
    match record_session(ids, &result).await {
        Ok(()) => AgentReply::Session { loaded, result },
        Err(error) => AgentReply::Refused(error),
    }
}

/// The failure of a Codex session start whose guard can't answer for the session.
fn guard_failed() -> Error {
    Error::refused(
        ErrorCode::AgentGuardSelftestFailed,
        "Clauseo couldn't start its file check, so ChatGPT didn't start. Quit and reopen Clauseo, \
         then try again.",
    )
}

/// A Codex session's guard check: the folder `_codex/hooks/list` names, and the hook's
/// self-test, run off the actor with the host's clock, for the matter.
pub(in crate::actor::turn) struct GuardCheck {
    pub(super) folder: PathBuf,
    pub(super) self_test: Option<SelfTest>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) matter: Option<MatterId>,
}

/// `thread`'s guard check when its agent is Codex and a hooks object went in at launch.
pub(super) fn check(
    actor: &mut Actor,
    thread: ThreadId,
    matter: Option<MatterId>,
) -> Option<GuardCheck> {
    let clock = Arc::clone(&actor.clock);
    state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .and_then(|agent| agent.codex.as_ref())
        .filter(|codex| codex.guard)
        .map(|codex| GuardCheck {
            folder: codex.folder.clone(),
            self_test: codex.self_test.clone(),
            clock,
            matter,
        })
}

/// The guard hook's self-test (`pre_write_guard/self_test.rs`) when `guard` has one, off the
/// actor: the session start fails unless the guard refused the self-test's call within its limit.
/// Its outcome and time go to the log on every OS.
pub(super) async fn self_test(guard: GuardCheck) -> Result<(), Error> {
    let GuardCheck {
        self_test: Some(test),
        clock,
        matter,
        ..
    } = guard
    else {
        return Ok(());
    };
    let job = codex::prepare::Job {
        what: "the guard's self-test",
        matter,
        // The run kills the guard at its limit; this is the wait for that answer.
        wait: crate::pre_write_guard::self_test::LIMIT.saturating_add(codex::prepare::PREPARE_WAIT),
    };
    let timer = Arc::clone(&clock);
    codex::prepare::off_actor(clock, job, move || {
        let start = timer.now().mono;
        let result = test.run();
        let took_ms = timer.now().mono.saturating_sub(start).as_millis();
        match result {
            Ok(()) => {
                tracing::info!(
                    took_ms,
                    outcome = "refused",
                    "the Codex guard hook passed its self-test"
                );
                Ok(())
            }
            Err(failure) => {
                tracing::error!(
                    took_ms,
                    ?failure,
                    "the Codex guard hook did not refuse its self-test; the session start fails"
                );
                Err(guard_failed())
            }
        }
    })
    .await
}
