//! `KeychainReadAgain` on the actor: desktop sends it once the lawyer pressed Allow for the
//! keychain (its after-Allow step; the desktop bridge refuses it from every window, `admit.rs`).
//! Each read the keychain refused before (an error that is not "no item") is made again, off
//! the actor, and each part says what changed its own way:
//!
//! - case law's credential, when it reads as unreadable: `Job::Status` reads it again and the
//!   state goes out (`ConnectorsChanged`, and `AddonsChanged` when its row moves);
//! - the lawyer's MCP values a launch was refused (`mcp/read_again.rs`): `McpToolsChanged`,
//!   and `AddonsChanged` when the list moves;
//! - the unsafe-site allow list's hash, when its start-up read was refused
//!   (`allowed/saving.rs`): once it reads, this run's allows are saved.
//!
//! It answers `Done` at once, with no receipt; the events follow the reads.

use clauseo_protocol::CommandOutcome;

use super::Actor;
use super::commands::CommandCtx;

/// `KeychainReadAgain`: starts each read again, then answers.
pub(super) fn read_again(actor: &mut Actor, ctx: &CommandCtx) {
    super::connectors::read_again(actor);
    super::mcp::read_again::ask(actor);
    super::allowed::read_again(actor);
    actor.finish_command(ctx.id, Ok(CommandOutcome::Done));
}
