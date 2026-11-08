//! The commands the host does not run yet: `RestartAgent`, and R18's four
//! (`RequestEscalation`, `SwitchProtectionOn`, `ForgetAllowance`, `DismissDenial`) until its
//! PR 3 brings the denial logic.

use clauseo_protocol::ErrorCode;

use super::CommandCtx;
use crate::actor::Actor;
use crate::error::Error;

/// Refuses the command with `command.invalid` ("not built yet"), with no receipt.
pub(super) fn not_built(actor: &mut Actor, ctx: &CommandCtx) {
    let refusal = Error::refused(ErrorCode::CommandInvalid, "not built yet");
    actor.finish_command(ctx.id, Err(refusal));
}
