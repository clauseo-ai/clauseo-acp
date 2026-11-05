//! The updater over the wire (decision record 0037), stubbed until R23's later PRs: the four
//! update commands answer `command.invalid`, and `GetUpdate` answers the view `NoUpdater` will
//! give, managed by the package, so a window can show the row before the updater exists.
//!
//! PR 4 replaces both with the state machine (`update/machine.rs`), the timers and
//! `Exit::Install`; nothing here touches the network or the disk.

use clauseo_protocol::{ErrorCode, ManagedBy, QueryResult, UpdateMode, UpdateState, UpdateView};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Reply;
use crate::error::Error;

/// The refusal message of every update command until the updater is wired.
const NOT_WIRED: &str = "the updater is not wired yet";

/// `CheckForUpdate`, `InstallUpdate`, `CancelScheduledUpdate` or `SetUpdateMode`: refused with
/// `command.invalid` until the updater is wired (R23 PR 4). No receipt, no event.
pub(super) fn not_wired(actor: &mut Actor, ctx: &CommandCtx) {
    let refusal = Error::refused(ErrorCode::CommandInvalid, NOT_WIRED);
    actor.finish_command(ctx.id, Err(refusal));
}

/// `GetUpdate`'s answer: what `NoUpdater` says, managed by the package in `auto` mode.
fn view() -> UpdateView {
    UpdateView {
        // The host is not told the app's version yet (PR 4's `with_updater` brings it from
        // desktop, which reads `tauri.conf.json`). Until then the host crate's own version
        // stands in; it is not the app's version, and no window acts on it while managed.
        current: env!("CARGO_PKG_VERSION").to_owned(),
        state: UpdateState::Managed {
            by: ManagedBy::Package,
        },
        // A placeholder until PR 4 reads the effective `update.mode`; nothing reads it yet.
        mode: UpdateMode::Auto,
        checked_at: None,
        updated_from: None,
    }
}

/// Answers `GetUpdate` with [`view`].
pub(super) fn answer(reply: Reply<QueryResult>) {
    let _gone = reply.send(Ok(QueryResult::Update(view())));
}
