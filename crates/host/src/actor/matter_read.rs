//! A conversation's read invite into another matter (`AllowMatterRead`, `WithdrawMatterRead`):
//! each logs its event in the thread's stream, which the reducer keeps on the header
//! (`read_matters`), so it survives a restart. The agent gets it at its next launch
//! (`agent::matter_fence`); a live agent whose fence it changes is stopped once idle
//! (`turn::outside::fence`), and a withdrawal while a turn runs stops that turn at once
//! (`turn/idle/fence.rs`).

use clauseo_protocol::{ErrorCode, MatterId, MatterReadAllowed, MatterReadWithdrawn, ThreadId};

use super::Actor;
use super::commands::CommandCtx;
use super::triage::log_thread_event;
use crate::error::Error;

/// `AllowMatterRead`: the thread's own matter is `command.invalid`, a matter not on the list
/// `command.unknown_matter`; otherwise `matter_read_allowed` and `Done`.
pub(super) fn allow(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId, matter: MatterId) {
    let own = match actor.check_thread(thread) {
        Ok(entry) => entry.row.matter,
        Err(error) => {
            actor.finish_command(ctx.id, Err(error));
            return;
        }
    };
    let refused = if matter == own {
        Some(Error::refused(
            ErrorCode::CommandInvalid,
            "a conversation reads its own matter already",
        ))
    } else if !actor.matters.contains_key(&matter) {
        Some(Error::refused(
            ErrorCode::CommandUnknownMatter,
            "no such matter",
        ))
    } else {
        None
    };
    if let Some(error) = refused {
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    log_thread_event(actor, ctx, thread, MatterReadAllowed { matter }.into());
}

/// `WithdrawMatterRead`: `matter_read_withdrawn` and `Done`, whether or not it was allowed
/// (a forgotten matter's invite can be withdrawn too).
pub(super) fn withdraw(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId, matter: MatterId) {
    log_thread_event(actor, ctx, thread, MatterReadWithdrawn { matter }.into());
}
