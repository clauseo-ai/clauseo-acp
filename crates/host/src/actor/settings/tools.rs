//! The tool switches over the wire: `tools` in `GetSettings`'s view ([`view`]), and
//! `SetToolSettings`, which turns the wire's `ToolsPatch` into the settings crate's and runs
//! `Effective::apply_tools_patch` on the actor. Kept apart from the UI code in `settings.rs`, so
//! the next section added the same way sits beside it, not in it.
//!
//! When it takes effect: a session reads the switches once, at its start
//! (`turn/profile_loads.rs`, `switches`), and nothing else reads them afterwards. So a change
//! reaches the next session started. A session already running keeps its tools: its header
//! (`header_tools.rs`) shows what it held when it started, and no tool is rebuilt mid-session.

use clauseo_protocol::{HostEvent, ToolsPatch, ToolsSettingsView};
use clauseo_settings as settings;
use clauseo_settings::{Effective, SystemFiles};

use super::super::Actor;
use super::super::commands::CommandCtx;
use crate::error::Error;

/// The tool switches in force.
pub(super) fn view(effective: &Effective) -> ToolsSettingsView {
    let tools = &effective.settings().tools;
    ToolsSettingsView {
        web_search: tools.web_search,
        browser: tools.browser,
    }
}

/// `SetToolSettings`: the lock check and the write (`apply_tools_patch`), then
/// `SettingsChanged` to every app subscriber, then the receipt. A refusal changes nothing and
/// sends nothing: a locked key is `settings.locked`, any other settings failure `internal`.
pub(in crate::actor) fn set_tool_settings(actor: &mut Actor, ctx: &CommandCtx, patch: &ToolsPatch) {
    if let Err(error) = apply(actor, ctx, patch) {
        actor.finish_command(ctx.id, Err(error));
    }
}

fn apply(actor: &mut Actor, ctx: &CommandCtx, patch: &ToolsPatch) -> Result<(), Error> {
    let patch = patch_in(patch);
    actor.effective.apply_tools_patch(&SystemFiles, &patch)?;
    // An empty patch writes nothing, so nothing changed to tell anyone about.
    if patch != settings::ToolsPatch::default() {
        let changed = HostEvent::SettingsChanged(super::view(&actor.effective));
        super::super::app::publish(actor, &changed);
    }
    super::receipt(actor, ctx)
}

/// The settings crate's patch for the wire's. Every field is named, so a field added to the
/// wire's patch does not compile until here.
fn patch_in(patch: &ToolsPatch) -> settings::ToolsPatch {
    let ToolsPatch {
        web_search,
        browser,
    } = patch;
    settings::ToolsPatch {
        web_search: *web_search,
        browser: *browser,
    }
}
