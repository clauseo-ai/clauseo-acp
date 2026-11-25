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
//! - A session reads the list at its launch (`agent::loads::lawyer`). Each change bumps the
//!   Tools revision ([`McpOnActor::tools_rev`]); a save that writes a keychain value also bumps
//!   that server's ([`McpOnActor::secret_revs`]). A live agent whose servers would differ is
//!   stopped once it is idle (`turn/idle/tools.rs`), and its next prompt relaunches it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use clauseo_connectors::StoreError;
use clauseo_protocol::{AddonKind, AddonScope, ErrorCode, McpCommand, McpServerEdit};
use clauseo_settings::{ItemScope, McpServerLaunch, McpValueKind, SystemFiles, keychain_account};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Input;
use crate::error::Error;

mod ask;
pub(crate) mod finder;
pub(crate) mod import;
pub(crate) mod keychain;
pub(crate) mod read_again;
pub(crate) mod scan;
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
    queue: VecDeque<Queued>,
    /// The variables (server, name) a launch found missing or unreadable in the keychain, until
    /// the server is saved again; the view shows them unsaved.
    pub(crate) missing: BTreeSet<(String, String)>,
    /// Of `missing`, those whose read the keychain refused (not "no item"), with their keychain
    /// account: `KeychainReadAgain` reads only these again (`read_again.rs`).
    pub(crate) refused: BTreeMap<(String, String), String>,
    /// The view last sent to app subscribers.
    pub(crate) sent: Option<clauseo_protocol::McpToolsView>,
    /// What the last import scan found; `None` before the first one ends.
    pub(crate) found: Option<Vec<scan::Found>>,
    /// What the last fresh scan saw, so a scan with nothing changed reads nothing.
    pub(crate) last_scan: Option<scan::LastScan>,
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
    /// Bumped by each change to the list written (`changed`).
    pub(crate) tools_rev: u64,
    /// By server, bumped by each save that wrote one of its keychain values.
    pub(crate) secret_revs: BTreeMap<String, u64>,
}

/// A command waiting behind keychain work.
enum Queued {
    /// An MCP command.
    Mcp(CommandCtx, McpCommand),
    /// A tool's scope, switch or removal (`addon_change/tools.rs`).
    Addon(CommandCtx, String, super::addon_change::Change),
}

/// A save whose keychain steps ran, and what its settings write takes.
pub(crate) struct Save {
    previous: Option<String>,
    name: String,
    server: McpServerLaunch,
    after: Vec<String>,
    /// The edit's scope, when it gave one.
    place: Option<AddonScope>,
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
    /// An import scan ended; `None` when it panicked or timed out.
    Scanned(Option<scan::ScanAnswer>),
    /// `KeychainReadAgain`'s read of the refused values ended.
    ReadAgain(Vec<read_again::Read>),
}

impl std::fmt::Debug for McpInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Written { .. } => "Written",
            Self::Tidied { .. } => "Tidied",
            Self::Lost { .. } => "Lost",
            Self::Imported { .. } => "Imported",
            Self::Scanned(_) => "Scanned",
            Self::ReadAgain(_) => "ReadAgain",
        })
    }
}

/// The start's view, as the one sent: a later change is what goes out.
/// The first import scan starts too.
pub(super) fn start(actor: &mut Actor) {
    actor.mcp.sent = Some(view::view(actor));
    scan::scan(actor);
}

/// One command: run now, or queued behind the keychain work running.
pub(super) fn run(actor: &mut Actor, ctx: &CommandCtx, command: McpCommand) {
    if actor.mcp.running {
        actor.mcp.queue.push_back(Queued::Mcp(*ctx, command));
        return;
    }
    begin(actor, *ctx, command);
}

/// Queues a tool's scope, switch or removal while keychain work runs: `true` when queued.
pub(super) fn queue_addon(
    actor: &mut Actor,
    ctx: &CommandCtx,
    id: &str,
    change: super::addon_change::Change,
) -> bool {
    if actor.mcp.running {
        actor
            .mcp
            .queue
            .push_back(Queued::Addon(*ctx, id.to_owned(), change));
    }
    actor.mcp.running
}

/// `RemoveAddon` of a server: as `RemoveServer`, without the receipt, which the caller sends
/// with the matters' rows. A server not there is refused by the caller first.
pub(super) fn remove_server(actor: &mut Actor, name: &str) -> Result<(), Error> {
    let Some(removed) = actor.effective.remove_mcp_server(&SystemFiles, name)? else {
        return Ok(());
    };
    // As `RemoveServer`: a value whose last read the keychain refused is kept (#1348).
    let accounts = not_refused(actor, keychain::accounts(name, &removed));
    forget_missing(actor, |server| server == name);
    scan::scan(actor);
    tidy(actor, accounts, None);
    Ok(())
}

/// A session's launch left these servers out for a missing value: the view shows it. Those
/// whose keychain account is in `refused` (the read was refused, not "no item") are also kept
/// apart, for `KeychainReadAgain`; one found missing this time is no longer.
pub(crate) fn left_out(
    actor: &mut Actor,
    left_out: Vec<(String, String)>,
    refused: &BTreeSet<String>,
) {
    if left_out.is_empty() {
        return;
    }
    for (server, var) in &left_out {
        let account = [McpValueKind::Env, McpValueKind::Header]
            .into_iter()
            .map(|kind| keychain_account(server, kind, var))
            .find(|account| refused.contains(account));
        let key = (server.clone(), var.clone());
        match account {
            Some(account) => {
                actor.mcp.refused.insert(key, account);
            }
            None => {
                actor.mcp.refused.remove(&key);
            }
        }
    }
    actor.mcp.missing.extend(left_out);
    view::publish(actor);
}

/// Forgets what launches found missing or refused for the servers `gone` names (saved again,
/// removed or imported over).
pub(crate) fn forget_missing(actor: &mut Actor, gone: impl Fn(&str) -> bool) {
    actor.mcp.missing.retain(|(server, _)| !gone(server));
    actor.mcp.refused.retain(|(server, _), _| !gone(server));
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
    let accounts = not_refused(actor, keychain::accounts(name, &removed));
    forget_missing(actor, |server| server == name);
    changed_moving(actor, ctx, (name, None));
    // A server the last scan found already added may import again.
    scan::scan(actor);
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
    let (mut server, plan) = keychain::plan(held.as_ref().map(|(name, s)| (*name, s)), edit)?;
    // A scope given replaces the kept one (`all` for a new server).
    let place = edit.scope;
    if let Some(AddonScope::Matter(matter)) = place {
        actor.check_matter(matter)?;
    }
    if let Some(scope) = place {
        server.scope = matches!(scope, AddonScope::Matter(_)).then_some(ItemScope::Matter);
    }
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
        place,
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
        place,
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
    let after = not_refused(actor, after);
    forget_missing(actor, |server| {
        Some(server) == previous.as_deref() || server == name
    });
    if !undo.is_empty() {
        // Its entry still reads `{"keychain": true}`: only this tells a live agent apart.
        *actor.mcp.secret_revs.entry(name.clone()).or_default() += 1;
    }
    let renamed = previous.as_deref().filter(|previous| *previous != name);
    match (place, renamed) {
        // A scope given places it as `SetAddonScope` does, the old name's switches gone.
        (Some(scope), _) => {
            let only = match scope {
                AddonScope::All => None,
                AddonScope::Matter(matter) => Some(matter),
            };
            actor.mcp.tools_rev += 1;
            view::publish(actor);
            let sent = super::addon_change::receipt_placing(actor, &ctx, (&name, renamed), only);
            if let Err(error) = sent {
                actor.finish_command(ctx.id, Err(error));
            }
        }
        // A rename carries the matters' switches (its scope came with its entry).
        (None, Some(previous)) => changed_moving(actor, ctx, (previous, Some(&name))),
        (None, None) => changed(actor, ctx),
    }
    tidy(actor, after, None);
    Ok(())
}

/// A change written: the view to every app subscriber, then the receipt.
fn changed(actor: &mut Actor, ctx: CommandCtx) {
    actor.mcp.tools_rev += 1;
    view::publish(actor);
    if let Err(error) = super::settings::receipt(actor, &ctx) {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// `accounts` without those whose last read the keychain refused: what they hold is not known,
/// so the clean-up keeps them, logged by count. Taken before `forget_missing` forgets them.
fn not_refused(actor: &Actor, mut accounts: Vec<String>) -> Vec<String> {
    let before = accounts.len();
    accounts.retain(|account| !actor.mcp.refused.values().any(|refused| refused == account));
    if accounts.len() < before {
        tracing::warn!(
            kept = before - accounts.len(),
            "keychain values whose last read was refused are not deleted"
        );
    }
    accounts
}

/// [`changed`], with every matter's switch for the server cleared or moved in the receipt's
/// commit (`addon_change::receipt_moving`).
fn changed_moving(actor: &mut Actor, ctx: CommandCtx, names: (&str, Option<&str>)) {
    actor.mcp.tools_rev += 1;
    view::publish(actor);
    let sent = super::addon_change::receipt_moving(actor, &ctx, AddonKind::Tool, names);
    if let Err(error) = sent {
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
        McpInput::Scanned(found) => scan::on_scanned(actor, found),
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
        // Its own thread, not the queue's: a read changes nothing in the keychain.
        McpInput::ReadAgain(read) => read_again::on_read(actor, read),
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
        match actor.mcp.queue.pop_front() {
            None => return,
            Some(Queued::Mcp(ctx, command)) => begin(actor, ctx, command),
            Some(Queued::Addon(ctx, id, change)) => {
                super::addon_change::tools::dequeued(actor, &ctx, &id, change);
            }
        }
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
