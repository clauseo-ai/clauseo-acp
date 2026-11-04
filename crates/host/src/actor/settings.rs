//! The display settings over the wire: `GetSettings`'s [`view`], and `SetUiSettings`, which
//! turns the wire's `UiPatch` into the settings crate's and runs `Effective::apply_ui_patch` on
//! the actor. The actor is the one owner of the settings, so it is the lock their writes need:
//! commands run one at a time, and a reader never sees half a patch.
//!
//! The wire types mirror `clauseo-settings`' `ui` section. Every conversion here is an
//! exhaustive match, so a value added to one side and not the other does not compile; the
//! mirror test in `tests/host/settings.rs` also holds the keys and the JSON names together.
//! The tool switches (`tools`, `SetToolSettings`) are in `tools.rs`, beside this file's UI code;
//! the privacy switches (`privacy`, `SetPrivacySettings`, `FirstAct`) in `privacy.rs`.

mod privacy;
mod tools;

pub(in crate::actor) use privacy::{first_act, set_privacy_settings};
pub(super) use tools::set_tool_settings;

use std::collections::BTreeMap;

use clauseo_protocol::{
    CommandOutcome, ErrorCode, HostEvent, Language, Lite, Motion, SettingsView, Theme, UiPatch,
    UiSettings,
};
use clauseo_settings as settings;
use clauseo_settings::{FontSize, SystemFiles};

use super::commands::{AfterCommand, CommandCtx};
use super::{Actor, Tag};
use crate::error::Error;
use crate::pipeline::Draft;

/// The display settings in force, the tool and privacy switches in force, and every key policy
/// locks, sorted.
pub(crate) fn view(actor: &Actor) -> SettingsView {
    let effective = &actor.effective;
    let ui = &effective.settings().ui;
    SettingsView {
        ui: UiSettings {
            theme: theme_out(ui.theme),
            font_size: ui.font_size.get(),
            language: language_out(ui.language),
            lite: lite_out(ui.lite),
            motion: motion_out(ui.motion),
        },
        tools: tools::view(effective),
        privacy: privacy::view(actor),
        locked: effective.locked().map(str::to_owned).collect(),
        // The firm plans come with R20's `plans` settings section (decision record 0048).
        plans: BTreeMap::new(),
    }
}

/// `SetUiSettings`: the patch's values, the lock check and the write (`apply_ui_patch`), then
/// `SettingsChanged` to every app subscriber, then the receipt. A refusal changes nothing and
/// sends nothing: a font size out of range is `command.invalid`, a locked key
/// `settings.locked`, any other settings failure `internal`.
pub(in crate::actor) fn set_ui_settings(actor: &mut Actor, ctx: &CommandCtx, patch: &UiPatch) {
    if let Err(error) = apply(actor, ctx, patch) {
        actor.finish_command(ctx.id, Err(error));
    }
}

fn apply(actor: &mut Actor, ctx: &CommandCtx, patch: &UiPatch) -> Result<(), Error> {
    let patch = patch_in(patch)?;
    let before = actor.effective.settings().ui.clone();
    actor.effective.apply_ui_patch(&SystemFiles, &patch)?;
    // An empty patch writes nothing, so nothing changed to tell anyone about.
    if patch != settings::UiPatch::default() {
        super::counting::ui_changed(actor, &before);
        let changed = HostEvent::SettingsChanged(view(actor));
        super::app::publish(actor, &changed);
    }
    receipt(actor, ctx)
}

/// The receipt of a settings command: a repeat of the command answers `Done` without writing
/// again. The settings live in `settings.json`, not the log, so the commit holds the receipt alone.
pub(super) fn receipt(actor: &mut Actor, ctx: &CommandCtx) -> Result<(), Error> {
    let outcome = CommandOutcome::Done;
    let mut draft = Draft::new();
    draft.receipt(ctx.receipt(None, outcome, actor.now().wall));
    let tag = Tag::Command {
        id: ctx.id,
        outcome,
        after: AfterCommand::Nothing,
    };
    actor.send_commit(draft, tag).map(|_| ())
}

/// The settings crate's patch for the wire's. A font size out of range is `command.invalid`,
/// with the settings crate's message.
fn patch_in(patch: &UiPatch) -> Result<settings::UiPatch, Error> {
    // Every field is named, so a field added to the wire's patch does not compile until here.
    let UiPatch {
        theme,
        font_size,
        language,
        lite,
        motion,
    } = patch;
    let font_size = font_size
        .map(FontSize::try_from)
        .transpose()
        .map_err(|message| Error::refused(ErrorCode::CommandInvalid, message))?;
    Ok(settings::UiPatch {
        theme: theme.map(theme_in),
        font_size,
        // The app no longer has a density setting; the key stays in the settings crate.
        density: None,
        language: language.map(language_in),
        lite: lite.map(lite_in),
        motion: motion.map(motion_in),
    })
}

fn theme_out(theme: settings::Theme) -> Theme {
    match theme {
        settings::Theme::System => Theme::System,
        settings::Theme::Light => Theme::Light,
        settings::Theme::Dark => Theme::Dark,
    }
}

fn theme_in(theme: Theme) -> settings::Theme {
    match theme {
        Theme::System => settings::Theme::System,
        Theme::Light => settings::Theme::Light,
        Theme::Dark => settings::Theme::Dark,
    }
}

fn language_out(language: settings::Language) -> Language {
    match language {
        settings::Language::En => Language::En,
        settings::Language::Hi => Language::Hi,
    }
}

fn language_in(language: Language) -> settings::Language {
    match language {
        Language::En => settings::Language::En,
        Language::Hi => settings::Language::Hi,
    }
}

fn lite_out(lite: settings::Lite) -> Lite {
    match lite {
        settings::Lite::Automatic => Lite::Automatic,
        settings::Lite::On => Lite::On,
        settings::Lite::Off => Lite::Off,
    }
}

fn lite_in(lite: Lite) -> settings::Lite {
    match lite {
        Lite::Automatic => settings::Lite::Automatic,
        Lite::On => settings::Lite::On,
        Lite::Off => settings::Lite::Off,
    }
}

fn motion_out(motion: settings::Motion) -> Motion {
    match motion {
        settings::Motion::System => Motion::System,
        settings::Motion::Reduce => Motion::Reduce,
        settings::Motion::Full => Motion::Full,
    }
}

fn motion_in(motion: Motion) -> settings::Motion {
    match motion {
        Motion::System => settings::Motion::System,
        Motion::Reduce => settings::Motion::Reduce,
        Motion::Full => settings::Motion::Full,
    }
}
