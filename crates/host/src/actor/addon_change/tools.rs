//! The tool half: Clauseo's own `case-law` and `browser` (a matter's switch only, locked with
//! `tools.connectors` and `tools.browser`), and the lawyer's MCP servers, whose changes wait
//! behind running MCP keychain work as the MCP commands do.

use clauseo_protocol::{AddonKind, ErrorCode};
use clauseo_settings::{ItemScope, SystemFiles};

use super::{Change, Want, built_in, finish, locked, refused, unknown};
use crate::actor::Actor;
use crate::actor::addon_rules::{BROWSER, CASE_LAW};
use crate::actor::commands::CommandCtx;
use crate::error::Error;

/// One tool command: now, or queued behind the MCP keychain work running.
pub(super) fn run(actor: &mut Actor, ctx: &CommandCtx, id: &str, change: Change) {
    let result = if id == CASE_LAW || id == BROWSER {
        builtin(actor, ctx, id, change)
    } else if crate::actor::mcp::queue_addon(actor, ctx, id, change) {
        Ok(())
    } else {
        added(actor, ctx, id, change)
    };
    if let Err(error) = result {
        refused(actor, ctx, AddonKind::Tool, error);
    }
}

/// A queued change's turn (`mcp.rs`).
pub(in crate::actor) fn dequeued(actor: &mut Actor, ctx: &CommandCtx, id: &str, change: Change) {
    if let Err(error) = added(actor, ctx, id, change) {
        refused(actor, ctx, AddonKind::Tool, error);
    }
}

/// `case-law` or `browser`: a matter's switch, unless policy sets the tool's key.
fn builtin(actor: &mut Actor, ctx: &CommandCtx, id: &str, change: Change) -> Result<(), Error> {
    let Change::On { matter, on } = change else {
        return Err(built_in(id));
    };
    let key = if id == CASE_LAW {
        "tools.connectors"
    } else {
        "tools.browser"
    };
    if actor.effective.is_locked(key) {
        return Err(locked(key));
    }
    actor.check_matter(matter)?;
    finish(
        actor,
        ctx,
        (AddonKind::Tool, id),
        Want::On { matter, on },
        false,
    );
    Ok(())
}

/// A server of the lawyer's.
fn added(actor: &mut Actor, ctx: &CommandCtx, id: &str, change: Change) -> Result<(), Error> {
    let Some(server) = actor.effective.mcp_servers().get(id) else {
        return Err(unknown(AddonKind::Tool, id));
    };
    let (enabled, scoped) = (server.enabled, server.matter_scoped());
    if crate::actor::mcp::view::managed(actor) {
        return Err(Error::refused(
            ErrorCode::SettingsLocked,
            "the firm runs MCP on this computer (managed-mcp.json)",
        ));
    }
    // While policy sets the list, the firm decides the set and each server's scope (decision
    // record 0040): a server it enables with scope `matter` takes a matter's switch either way;
    // any other change is `settings.locked`.
    let switch = matches!(change, Change::On { .. });
    if actor.effective.mcp_locked() && !(switch && enabled && scoped) {
        return Err(locked("mcp.servers"));
    }
    if let Some(matter) = change.matter() {
        actor.check_matter(matter)?;
    }
    // The scope written (`None` for `all`), and whether the server is turned on with it.
    let (want, scope) = match change {
        Change::Scope(only) => (
            Want::Only(only),
            Some((only.map(|_| ItemScope::Matter), true)),
        ),
        Change::On { matter, on: true } if !enabled => (
            Want::Only(Some(matter)),
            Some((Some(ItemScope::Matter), true)),
        ),
        Change::On { matter, on } => (Want::On { matter, on }, None),
        Change::Nowhere => (Want::Only(None), Some((Some(ItemScope::Matter), enabled))),
        Change::Remove => {
            crate::actor::mcp::remove_server(actor, id)?;
            finish(actor, ctx, (AddonKind::Tool, id), Want::Only(None), false);
            return Ok(());
        }
    };
    if let Some((scope, on)) = scope {
        actor
            .effective
            .set_mcp_server_scope(&SystemFiles, id, on, scope)?;
    }
    finish(actor, ctx, (AddonKind::Tool, id), want, false);
    Ok(())
}
