//! The privacy switches over the wire (decision record 0036): `privacy` in `GetSettings`'s view
//! ([`view`]), `SetPrivacySettings`, and `FirstAct`. The switches live in `settings.json`; the
//! counts themselves in `clauseo-counts`, which this file turns on, off or started around each
//! settings write.
//!
//! The rule 0036 sets for the host, in two halves:
//! 1. Off is saved first. Turning counts off writes `privacy.counts: false` to `settings.json`
//!    and, at the first act, the first-act marker before it deletes `<data_dir>/counts/`, and
//!    stands when the delete fails: the next start opens counts off and tries the delete again.
//!    When the write itself fails, the command is refused (`internal`), no marker is written and
//!    the setting has not changed, so the switch still shows on; counts stop anyway, but only for
//!    this session: the next start reads `true` (in memory only, before a first act). Off is
//!    otherwise never refused, but by a policy lock, which is off already.
//! 2. On is refused while the old counts cannot go. `Counts::set_enabled(true)` deletes a folder
//!    whose delete failed before it makes a new id; while that fails, the command is refused with
//!    `store.io` and `privacy.counts` stays `false`, so the old id is never resumed.
//!
//! Before the lawyer's first act (`<data_dir>/first_act_done`, which `Counts::first_act` writes),
//! a patch naming `counts` is that first act: the lawyer changed the switch in view. `FirstAct`
//! after the marker exists changes nothing and answers `Done`. Without counts
//! (`HostConfig::with_counts` not called) the switches are still saved, `FirstAct` changes
//! nothing, and `counts_started` reads `false`.
//!
//! Every app subscriber gets `SettingsChanged` whenever the view changed, a refused command's
//! included (a first act whose settings write failed has still started counts).
//!
//! Like `SetUiSettings`, both commands log only their receipt: the switches are not log state.

use clauseo_counts::Counts;
use clauseo_protocol::{ErrorCode, HostEvent, PrivacyPatch, PrivacySettings};
use clauseo_settings as settings;
use clauseo_settings::SystemFiles;

use super::super::commands::CommandCtx;
use super::super::{Actor, counting};
use crate::error::Error;

/// The privacy switches in force, and whether the first act has happened.
pub(super) fn view(actor: &Actor) -> PrivacySettings {
    let privacy = &actor.effective.settings().privacy;
    PrivacySettings {
        counts: privacy.counts,
        crash_reports: privacy.crash_reports,
        counts_started: actor.counts.as_ref().is_some_and(Counts::consented),
    }
}

/// `SetPrivacySettings`: the lock check, the counts and the write in the order the module doc
/// gives, `setting_changed` for crash reports, then `SettingsChanged` to every app subscriber,
/// then the receipt. A refusal is `settings.locked` for a locked key, `store.io` for counts
/// that cannot turn on, `internal` for any other settings failure; it sends `SettingsChanged`
/// only when the view changed anyway.
pub(in crate::actor) fn set_privacy_settings(
    actor: &mut Actor,
    ctx: &CommandCtx,
    patch: &PrivacyPatch,
) {
    let patch = patch_in(patch);
    let before = view(actor);
    let settings_before = actor.effective.settings().privacy.clone();
    let applied = apply(actor, &patch);
    counting::privacy_changed(actor, &settings_before);
    // An empty patch writes nothing; a refused one tells only what changed regardless.
    let sent = applied.is_ok() && patch != settings::PrivacyPatch::default();
    answer(actor, ctx, before, sent, applied);
}

fn apply(actor: &mut Actor, patch: &settings::PrivacyPatch) -> Result<(), Error> {
    actor.effective.check_privacy_patch(patch)?;
    match patch.counts {
        Some(on) => switch(actor, on, patch),
        None => Ok(actor.effective.apply_privacy_patch(&SystemFiles, patch)?),
    }
}

/// `SettingsChanged` when the view moved from `before` (or `publish_anyway` says so), on
/// success and failure alike, then the receipt or the refusal.
fn answer(
    actor: &mut Actor,
    ctx: &CommandCtx,
    before: PrivacySettings,
    publish_anyway: bool,
    result: Result<(), Error>,
) {
    if publish_anyway || view(actor) != before {
        publish(actor);
    }
    if let Err(error) = result.and_then(|()| super::receipt(actor, ctx)) {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// `FirstAct`: the welcome's button, with the switch as the lawyer left it. Once the first act
/// has happened, or with no counts, it changes nothing. Before it, it is a change of the switch
/// to `on` ([`switch`]); a policy lock on `privacy.counts` stands over `on`, and only the marker
/// is written.
pub(in crate::actor) fn first_act(actor: &mut Actor, ctx: &CommandCtx, on: bool) {
    let before = view(actor);
    let result = first_act_in(actor, on);
    answer(actor, ctx, before, false, result);
}

fn first_act_in(actor: &mut Actor, on: bool) -> Result<(), Error> {
    let Some(counts) = actor.counts.clone() else {
        tracing::debug!("no usage counts in this host: the first act changes nothing");
        return Ok(());
    };
    if counts.consented() {
        return Ok(());
    }
    if actor.effective.is_locked("privacy.counts") {
        // Policy keeps counts off: the marker alone, so the welcome is not shown again.
        counts
            .first_act(on)
            .map_err(|error| refused_io("the first act could not be saved", &error))?;
    } else {
        let patch = settings::PrivacyPatch {
            counts: Some(on),
            crash_reports: None,
        };
        switch(actor, on, &patch)?;
    }
    Ok(())
}

/// Sets `privacy.counts` to `on`, with the rest of `patch`, and counts with it, as the module
/// doc says: off is saved first; on is refused while the old counts cannot be deleted. Before
/// the marker exists, the change is also the lawyer's first act.
fn switch(actor: &mut Actor, on: bool, patch: &settings::PrivacyPatch) -> Result<(), Error> {
    let Some(counts) = actor.counts.clone() else {
        actor.effective.apply_privacy_patch(&SystemFiles, patch)?;
        return Ok(());
    };
    if on {
        if !counts.consented()
            && let Err(error) = counts.first_act(true)
        {
            if !counts.consented() {
                return Err(refused_io("the first act could not be saved", &error));
            }
            tracing::debug!(%error, "the first act's counts were not all saved; the next tick tries again");
        }
        turn_on(actor, &counts, patch)
    } else {
        turn_off(actor, &counts, patch)
    }
}

/// The rule's first half: the setting is saved first, then the first-act marker (only once the
/// setting is saved), then the counts are deleted; counts turn off even when the write, the
/// marker or the delete fails. A failed write refuses the command and writes no marker: the
/// setting is unchanged and the first act has not happened, so this off lasts only until the
/// next start, which counts in memory again until the lawyer's first act.
fn turn_off(
    actor: &mut Actor,
    counts: &Counts,
    patch: &settings::PrivacyPatch,
) -> Result<(), Error> {
    let written = actor.effective.apply_privacy_patch(&SystemFiles, patch);
    // The marker only once off is saved: with `privacy.counts` still true in the file, a marker
    // would turn this refused off into consent at the next start (code owner's review of #344).
    if written.is_ok()
        && !counts.consented()
        && let Err(error) = counts.first_act(false)
    {
        // Only means the next change of the switch is the first act again.
        tracing::debug!(%error, "the first act's marker could not be saved");
    }
    keep_off(counts);
    written.map_err(Error::from)
}

/// The rule's second half: counts turn on (a new id) before the setting is saved, and a failure
/// leaves them off and the setting as it was. A failed write turns them back off.
fn turn_on(
    actor: &mut Actor,
    counts: &Counts,
    patch: &settings::PrivacyPatch,
) -> Result<(), Error> {
    let was_on = actor.effective.settings().privacy.counts;
    if let Err(error) = counts.set_enabled(true) {
        if !was_on {
            keep_off(counts);
        }
        return Err(refused_io(
            "the usage counts from before they were turned off could not be deleted",
            &error,
        ));
    }
    let written = actor.effective.apply_privacy_patch(&SystemFiles, patch);
    if written.is_err() && !was_on {
        keep_off(counts);
    }
    written.map_err(Error::from)
}

/// Counts back off after a refused on.
fn keep_off(counts: &Counts) {
    if let Err(error) = counts.set_enabled(false) {
        tracing::warn!(%error, "the usage counts folder could not be deleted; each tick and the next start try again");
    }
}

/// `store.io`: the counts folder or the first-act marker could not be changed.
fn refused_io(what: &str, error: &std::io::Error) -> Error {
    Error::refused(ErrorCode::StoreIo, format!("{what}: {error}"))
}

fn publish(actor: &mut Actor) {
    let changed = HostEvent::SettingsChanged(super::view(actor));
    super::super::app::publish(actor, &changed);
}

/// The settings crate's patch for the wire's. Every field is named, so a field added to the
/// wire's patch does not compile until here.
fn patch_in(patch: &PrivacyPatch) -> settings::PrivacyPatch {
    let PrivacyPatch {
        counts,
        crash_reports,
    } = patch;
    settings::PrivacyPatch {
        counts: *counts,
        crash_reports: *crash_reports,
    }
}
