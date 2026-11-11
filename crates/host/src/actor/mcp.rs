//! The lawyer's MCP server commands on the actor (Settings › Tools, `HostCommand::Mcp`).
//!
//! - Refused with `settings.locked` while the firm decides the list: policy sets `mcp.servers`
//!   (`McpControl::FirmList`), or its `managed-mcp.json` is on this computer (`FirmManaged`).
//! - `SaveServer`: the server is built from the edit and the entry it replaces
//!   (`keychain::plan`), checked by the settings crate (`Effective::check_mcp_server`), then on
//!   a thread of their own a local server's program is found (`mcp.command_not_found` when it is
//!   not, with nothing written; the settings keep it as typed), the lawyer's native confirm is
//!   asked when the save adds a server, changes what starts or a loader variable's value
//!   (`ask.rs`;
//!   `mcp.save_declined` when declined, with nothing written), and its values are written to the
//!   keychain, then the settings on the actor
//!   (`Effective::save_mcp_server`). A keychain failure is `mcp.secret_store_failed` with the
//!   keychain put back and the settings unchanged; a settings write that fails puts the keychain
//!   back, then answers the write's error. Items no longer named are deleted after the settings
//!   write. A remote server's save that touches no keychain value writes the settings at once.
//! - `RemoveServer` writes the settings, then deletes the server's items; `SetServerEnabled`
//!   writes the settings only.
//! - One command's keychain work runs at a time: a command that comes meanwhile waits its turn
//!   (`McpOnActor::queue`), so a delete never races a later write of the same account.
//! - Each change sends `McpToolsChanged` ([`view::publish`]) and answers `Done` with a receipt,
//!   as the other settings commands do. A command's secret value never reaches its receipt hash
//!   (it encodes as `[redacted]`), a log line or an error.
//! - `Import` (`import.rs`) saves the importable servers of one app as `SaveServer` adds them,
//!   and puts its result in the view; the counts the view shows come from a scan of the apps'
//!   config off the actor.
//! - A session reads the list at its launch (`agent::loads::lawyer`): a change reaches the next
//!   session started.

use std::collections::{BTreeSet, VecDeque};

use clauseo_connectors::StoreError;
use clauseo_protocol::{ErrorCode, McpCommand, McpServerEdit};
use clauseo_settings::{McpServerLaunch, SystemFiles};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Input;
use crate::error::Error;

mod ask;
pub(crate) mod finder;
pub(crate) mod import;
pub(crate) mod keychain;
pub(crate) mod view;

#[cfg(test)]
mod tests;

/// The name of a keychain thread.
const THREAD_NAME: &str = "clauseo-mcp-keychain";

/// The MCP state the actor holds.
#[derive(Default)]
pub(crate) struct McpOnActor {
    /// Whether a command's keychain work runs.
    running: bool,
    /// The commands that came while it runs, in order.
    queue: VecDeque<(CommandCtx, McpCommand)>,
    /// The variables (server, name) a launch found missing or unreadable in the keychain, until
    /// the server is saved again; the view shows them unsaved.
    pub(crate) missing: BTreeSet<(String, String)>,
    /// The view last sent to app subscribers.
    pub(crate) sent: Option<clauseo_protocol::McpToolsView>,
    /// What the last import scan found; `None` before the first one ends.
    pub(crate) found: Option<Vec<import::Found>>,
    /// Whether a scan runs.
    pub(crate) scanning: bool,
    /// Whether another scan was asked for while one ran.
    pub(crate) rescan: bool,
    /// Whether a scan's finder worker still runs (`finder.rs`).
    pub(crate) scan_busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Whether an import's finder worker still runs.
    pub(crate) import_busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// How the last import went, for the view.
    pub(crate) last_import: Option<clauseo_protocol::McpImportResult>,
}

/// A save whose keychain steps ran, and what its settings write takes.
pub(crate) struct Save {
    previous: Option<String>,
    name: String,
    server: McpServerLaunch,
    after: Vec<String>,
}

/// What a keychain thread posts back.
pub(crate) enum McpInput {
    /// A save's keychain steps ended.
    Written {
        ctx: CommandCtx,
        save: Box<Save>,
        result: Result<keychain::Undo, Failed>,
    },
    /// A put-back or a delete ended; with a command, the command is answered with `error`.
    Tidied { answer: Option<(CommandCtx, Error)> },
    /// The thread ended without its answer (it panicked); its command, if any, is `internal`.
    Lost { ctx: Option<CommandCtx> },
    /// An import's keychain writes ended.
    Imported {
        ctx: CommandCtx,
        result: Result<Box<import::Imported>, import::ImportFailed>,
    },
    /// An import scan ended; `None` when it panicked.
    Scanned(Option<Vec<import::Found>>),
}

impl std::fmt::Debug for McpInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Written { .. } => "Written",
            Self::Tidied { .. } => "Tidied",
            Self::Lost { .. } => "Lost",
            Self::Imported { .. } => "Imported",
            Self::Scanned(_) => "Scanned",
        })
    }
}

/// The start's view, as the one sent: a later change is what goes out.
/// The first import scan starts too.
pub(super) fn start(actor: &mut Actor) {
    actor.mcp.sent = Some(view::view(actor));
    import::scan(actor);
}

/// One command: run now, or queued behind the keychain work running.
pub(super) fn run(actor: &mut Actor, ctx: &CommandCtx, command: McpCommand) {
    if actor.mcp.running {
        actor.mcp.queue.push_back((*ctx, command));
        return;
    }
    begin(actor, *ctx, command);
}

/// A session's launch left these servers out for a missing value: the view shows it.
pub(crate) fn left_out(actor: &mut Actor, left_out: Vec<(String, String)>) {
    if left_out.is_empty() {
        return;
    }
    actor.mcp.missing.extend(left_out);
    view::publish(actor);
}

/// The checks and the work of one command; answered here unless keychain work started.
fn begin(actor: &mut Actor, ctx: CommandCtx, command: McpCommand) {
    if view::managed(actor) {
        let error = Error::refused(
            ErrorCode::SettingsLocked,
            "the firm runs MCP on this computer (managed-mcp.json)",
        );
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let result = match command {
        McpCommand::SetServerEnabled { name, enabled } => actor
            .effective
            .set_mcp_server_enabled(&SystemFiles, &name, enabled)
            .map_err(Error::from)
            .map(|()| changed(actor, ctx)),
        McpCommand::RemoveServer { name } => remove(actor, ctx, &name),
        McpCommand::SaveServer {
            previous_name,
            server,
        } => save(actor, ctx, previous_name, &server),
        McpCommand::Import { source } => import::import(actor, ctx, source),
    };
    if let Err(error) = result {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// `RemoveServer`: the settings, then the server's keychain items off the actor.
fn remove(actor: &mut Actor, ctx: CommandCtx, name: &str) -> Result<(), Error> {
    let Some(removed) = actor.effective.remove_mcp_server(&SystemFiles, name)? else {
        // Nothing to remove: done, with nothing written.
        return super::settings::receipt(actor, &ctx);
    };
    actor.mcp.missing.retain(|(server, _)| server != name);
    changed(actor, ctx);
    // A server the last scan found already added may import again.
    import::scan(actor);
    let accounts = keychain::accounts(name, &removed);
    tidy(actor, accounts, None);
    Ok(())
}

/// `SaveServer`: the plan and the checks on the actor, then the keychain steps off it, or the
/// settings at once when there is none.
fn save(
    actor: &mut Actor,
    ctx: CommandCtx,
    previous: Option<String>,
    edit: &McpServerEdit,
) -> Result<(), Error> {
    if actor.effective.mcp_locked() {
        return Err(Error::Settings(clauseo_settings::Error::Locked {
            key: "mcp.servers".to_owned(),
        }));
    }
    let held = previous
        .as_deref()
        .and_then(|name| Some((name, actor.effective.mcp_servers().get(name)?.clone())));
    if previous.is_some() && held.is_none() {
        return Err(Error::Settings(clauseo_settings::Error::Mcp {
            key: format!("mcp.servers.{}", previous.as_deref().unwrap_or_default()),
            refusal: clauseo_settings::McpRefusal::Unknown,
        }));
    }
    let (server, plan) = keychain::plan(held.as_ref().map(|(name, s)| (*name, s)), edit)?;
    actor
        .effective
        .check_mcp_server(previous.as_deref(), &edit.name, &server)?;
    // Refused before any prompt when too long to show whole.
    let ask = ask::ask(held.as_ref().map(|(_, held)| held), &server, edit)?;
    let save = Box::new(Save {
        previous,
        name: edit.name.clone(),
        server,
        after: plan.after,
    });
    let command = save.server.command().map(str::to_owned);
    if command.is_none() && plan.steps.is_empty() && ask.is_none() {
        return write_settings(actor, ctx, *save, keychain::Undo::default());
    }
    let store = actor.wiring.mcp_secrets.clone();
    if store.is_none() && !plan.steps.is_empty() {
        return Err(store_failed());
    }
    let resolve = std::sync::Arc::clone(&actor.wiring.mcp_resolve);
    let confirm = actor.wiring.mcp_confirm.clone();
    let steps = plan.steps;
    off_actor(actor, Some(ctx), move || {
        // The program first, as a launch finds it: one not found writes nothing anywhere, and
        // asks nothing. Then the lawyer's confirm, then the keychain.
        let found = command
            .as_deref()
            .is_none_or(|command| resolve(command).is_some());
        let allowed = || match (&ask, &confirm) {
            (None, _) => true,
            (Some(ask), Some(confirm)) => confirm.confirm(ask),
            // No confirm wired: a program nobody saw is never added.
            (Some(_), None) => false,
        };
        let result = if !found {
            Err(Failed::NotFound)
        } else if !allowed() {
            Err(Failed::Declined)
        } else {
            match store {
                Some(store) => keychain::run(store.as_ref(), steps).map_err(Failed::Store),
                None => Ok(keychain::Undo::default()),
            }
        };
        McpInput::Written { ctx, save, result }
    })
}

/// Why a save's work off the actor wrote nothing.
pub(crate) enum Failed {
    /// The local server's program is not on this computer.
    NotFound,
    /// The keychain failed; it was put back.
    Store(StoreError),
    /// The lawyer declined the confirm (or none is wired).
    Declined,
}

/// The settings write of a save whose keychain steps ran: on success the view, the receipt,
/// then the old items deleted; on failure the keychain put back, then the error.
fn write_settings(
    actor: &mut Actor,
    ctx: CommandCtx,
    save: Save,
    undo: keychain::Undo,
) -> Result<(), Error> {
    let Save {
        previous,
        name,
        server,
        after,
    } = save;
    if let Err(error) =
        actor
            .effective
            .save_mcp_server(&SystemFiles, previous.as_deref(), &name, &server)
    {
        let error = Error::from(error);
        match actor.wiring.mcp_secrets.clone() {
            Some(store) => {
                off_actor(actor, Some(ctx), move || {
                    keychain::put_back(store.as_ref(), undo);
                    McpInput::Tidied {
                        answer: Some((ctx, error)),
                    }
                })?;
                return Ok(());
            }
            None => return Err(error),
        }
    }
    actor
        .mcp
        .missing
        .retain(|(server, _)| Some(server) != previous.as_ref() && *server != name);
    changed(actor, ctx);
    tidy(actor, after, None);
    Ok(())
}

/// A change written: the view to every app subscriber, then the receipt.
fn changed(actor: &mut Actor, ctx: CommandCtx) {
    view::publish(actor);
    if let Err(error) = super::settings::receipt(actor, &ctx) {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// Deletes `accounts` off the actor; with none (or no keychain) nothing runs.
fn tidy(actor: &mut Actor, accounts: Vec<String>, answer: Option<(CommandCtx, Error)>) {
    let store = actor.wiring.mcp_secrets.clone();
    match store {
        Some(store) if !accounts.is_empty() => {
            let ctx = answer.as_ref().map(|(ctx, _)| *ctx);
            let started = off_actor(actor, ctx, move || {
                keychain::delete(store.as_ref(), &accounts);
                McpInput::Tidied { answer }
            });
            if let Err(error) = started {
                tracing::warn!(error = %crate::log_file::Cause(&error), "the keychain clean-up did not start");
            }
        }
        _ => {
            if let Some((ctx, error)) = answer {
                actor.finish_command(ctx.id, Err(error));
            }
        }
    }
}

/// A keychain thread's answer.
pub(super) fn on_input(actor: &mut Actor, input: McpInput) {
    match input {
        McpInput::Scanned(found) => import::on_scanned(actor, found),
        McpInput::Imported { ctx, result } => {
            actor.mcp.running = false;
            import::on_imported(actor, ctx, result);
        }
        McpInput::Written { ctx, save, result } => {
            actor.mcp.running = false;
            let answered = match result {
                Ok(undo) => write_settings(actor, ctx, *save, undo),
                Err(Failed::Store(error)) => {
                    tracing::warn!(%error, "an MCP server's keychain values were not saved");
                    Err(store_failed())
                }
                Err(Failed::Declined) => Err(Error::refused(
                    ErrorCode::McpSaveDeclined,
                    "the lawyer did not allow the program",
                )),
                Err(Failed::NotFound) => Err(Error::Settings(clauseo_settings::Error::Mcp {
                    key: format!("mcp.servers.{}", save.name),
                    refusal: clauseo_settings::McpRefusal::CommandNotFound,
                })),
            };
            if let Err(error) = answered {
                actor.finish_command(ctx.id, Err(error));
            }
        }
        McpInput::Tidied { answer } => {
            actor.mcp.running = false;
            if let Some((ctx, error)) = answer {
                actor.finish_command(ctx.id, Err(error));
            }
        }
        McpInput::Lost { ctx } => {
            actor.mcp.running = false;
            if let Some(ctx) = ctx {
                let error = Error::Internal("the keychain thread ended early".to_owned());
                actor.finish_command(ctx.id, Err(error));
            }
        }
    }
    next(actor);
}

/// Runs the queued commands until one starts keychain work.
fn next(actor: &mut Actor) {
    while !actor.mcp.running {
        let Some((ctx, command)) = actor.mcp.queue.pop_front() else {
            return;
        };
        begin(actor, ctx, command);
    }
}

fn store_failed() -> Error {
    Error::refused(
        ErrorCode::McpSecretStoreFailed,
        "the keychain did not save the value",
    )
}

/// Runs `job` on a keychain thread, which posts its answer; while it runs, later commands wait.
/// A thread that ends without answering (a panic) posts [`McpInput::Lost`] for `ctx`.
fn off_actor(
    actor: &mut Actor,
    ctx: Option<CommandCtx>,
    job: impl FnOnce() -> McpInput + Send + 'static,
) -> Result<(), Error> {
    let post = actor.post.clone();
    std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let mut unanswered = Unanswered {
                post: Some(post.clone()),
                ctx,
            };
            let input = job();
            unanswered.post = None;
            let _gone = post.try_send(Input::Mcp(Box::new(input)));
        })
        .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))?;
    actor.mcp.running = true;
    Ok(())
}

/// Posts [`McpInput::Lost`] when dropped while still armed: its thread ended without answering.
struct Unanswered {
    post: Option<async_channel::Sender<Input>>,
    ctx: Option<CommandCtx>,
}

impl Drop for Unanswered {
    fn drop(&mut self) {
        if let Some(post) = self.post.take() {
            let lost = McpInput::Lost { ctx: self.ctx };
            let _gone = post.try_send(Input::Mcp(Box::new(lost)));
        }
    }
}
