//! `SetDefaultLocation` (the app's one-click move of the matters folder out of a protected folder
//! such as Documents): the folder ref `pick_folder` issued becomes `matters.default_location`.
//! Only the setting changes. Existing matters keep their folders, and nothing on disk is made or
//! moved; the next `CreateMatter` with `DefaultLocation` makes its folder there.
//!
//! The order: the folder ref is resolved (`command.invalid` for one the host never gave), then
//! the policy lock and the write (`Effective::set_default_location`), then `SettingsChanged` to
//! every app subscriber, then the receipt. A refusal changes nothing and sends nothing. A write
//! that fails (the settings file cannot be written or does not parse) is `store.io`.
//!
//! Like the other settings commands, it logs only its receipt.

use clauseo_protocol::{ErrorCode, FolderRefId, HostEvent};
use clauseo_settings as settings;
use clauseo_settings::SystemFiles;

use super::super::Actor;
use super::super::commands::CommandCtx;
use crate::error::Error;

/// `SetDefaultLocation`: see the module doc. A refusal is `settings.locked` for a policy that
/// sets `matters.default_location`, `command.invalid` for a folder ref the host did not issue,
/// `store.io` for a failed write.
pub(in crate::actor) fn set_default_location(
    actor: &mut Actor,
    ctx: &CommandCtx,
    folder: FolderRefId,
) {
    if let Err(error) = apply(actor, ctx, folder) {
        actor.finish_command(ctx.id, Err(error));
    }
}

fn apply(actor: &mut Actor, ctx: &CommandCtx, folder: FolderRefId) -> Result<(), Error> {
    let path = super::super::matters::registered(actor, folder)?;
    actor
        .effective
        .set_default_location(&SystemFiles, &path)
        .map_err(saving)?;
    let changed = HostEvent::SettingsChanged(super::view(actor));
    super::super::app::publish(actor, &changed);
    super::receipt(actor, ctx)
}

/// `store.io` for a settings write that failed: the file could not be written, or it does not
/// parse, so it cannot be written in place. Any other settings failure keeps its own code.
fn saving(error: settings::Error) -> Error {
    let reason = if let settings::Error::Io { source, .. } = &error {
        source.to_string()
    } else if matches!(error, settings::Error::Parse { .. }) {
        "settings.json does not parse".to_owned()
    } else {
        return Error::Settings(error);
    };
    Error::refused(
        ErrorCode::StoreIo,
        format!("the default location could not be saved: {reason}"),
    )
}
