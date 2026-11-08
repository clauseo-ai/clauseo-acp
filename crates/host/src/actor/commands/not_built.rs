//! The commands the host does not run yet: `RestartAgent`, and four of R18's
//! (`RequestEscalation`, `SwitchProtectionOn`, `ForgetAllowance`, `GoOnInsideMatter`) until
//! its later PRs bring the dialogs, the allowances and the prompt Clauseo sends. `DismissDenial`
//! runs (`turn/denials.rs`).

use clauseo_protocol::ErrorCode;

use super::CommandCtx;
use crate::actor::Actor;
use crate::error::Error;

/// Refuses the command with `command.invalid` ("not built yet"), with no receipt.
pub(super) fn not_built(actor: &mut Actor, ctx: &CommandCtx) {
    let refusal = Error::refused(ErrorCode::CommandInvalid, "not built yet");
    actor.finish_command(ctx.id, Err(refusal));
}
