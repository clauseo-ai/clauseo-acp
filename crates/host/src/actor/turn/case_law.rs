//! The relay's case-law results on the thread (R24, PR6): what `sources/case_law.rs` joins to
//! the agent's own tool call when it completes; and the thread's notice that a call met an
//! out-of-credit refusal (`NoticeKind::CaseLawOutOfCredit`), from the relay, so every agent's
//! thread gets it alike.

use clauseo_protocol::{Notice, NoticeKind, NoticeLevel, ThreadId};

use super::super::Actor;
use super::state;
use super::updates::commit;
use crate::config::SessionKey;
use crate::log_file::Cause;

/// The out-of-credit notice's fallback words, for a view that does not know its kind.
pub(crate) const OUT_OF_CREDIT_TEXT: &str = "Case Law is out of credits, so it couldn't search. \
     Once credits are added, ask again to search case law.";

/// The relay found what a case-law call of `thread` returned: kept for the sources join
/// (`sources/case_law.rs`) until the agent's tool call completes, or five minutes pass. A
/// thread that is not loaded drops it: its call cannot complete in a live turn.
pub(crate) fn found(actor: &mut Actor, thread: ThreadId, found: crate::connectors::CaseLawFound) {
    let now = actor.now().mono;
    if let Some(state) = state(actor, thread) {
        state.sources.case_law.found(now, found);
    } else {
        tracing::debug!(%thread, "a case-law result for a thread not loaded was dropped");
    }
}

/// The relay refused a Case Law call of `session` for lack of credit: the session's thread logs
/// one `CaseLawOutOfCredit` notice, unless it already did since its turn started (a helper's
/// call goes through its lead's session, so it marks the lead's thread). A session that is not
/// the thread's current run (a stopped one), or a thread not loaded, logs nothing. The notice
/// carries no pay link: the app reads that from the connectors' state.
pub(crate) fn out_of_credit(actor: &mut Actor, session: SessionKey) {
    let thread = session.thread;
    let Some(turns) = state(actor, thread) else {
        tracing::debug!(%thread, "an out-of-credit refusal for a thread not loaded");
        return;
    };
    if turns.run.as_ref().map(|run| run.key) != Some(session)
        || std::mem::replace(&mut turns.credit_told, true)
    {
        return;
    }
    let notice = Notice {
        level: NoticeLevel::Warning,
        text: OUT_OF_CREDIT_TEXT.to_owned(),
        kind: Some(NoticeKind::CaseLawOutOfCredit),
    };
    if let Err(error) = commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "an out-of-credit notice was not logged");
        if let Some(turns) = state(actor, thread) {
            turns.credit_told = false;
        }
    }
}
