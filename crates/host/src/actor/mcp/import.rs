//! Settings › Tools' import of the MCP servers Claude Desktop, Claude Code and Codex set up
//! (`McpCommand::Import`); the counts the screen's import buttons show come from `scan.rs`.
//!
//! - The apps' config files are read by the finder (`HostConfig::with_mcp_import`, over
//!   `clauseo_settings::mcp_import::find_candidates`), always off the actor and bounded
//!   (`finder.rs`: one worker of each kind at a time, waited for at most 10 s): at the start,
//!   at each app catch-up (a window opening), after a removal, and at each import. One scan runs
//!   at a time; asks while it runs fold into one more after it.
//! - A scan keeps names and sources only (`scan::Found`), never a value. The counts are worked out
//!   from it at each view against the list as it is then, so an import or a save lowers them at
//!   once.
//! - A server counts, and imports, unless a problem blocks it: already added, not approved in
//!   Claude Code, a transport Clauseo does not run, a program not found, a variable not set, or
//!   an entry partly unreadable. A changed name alone does not block it.
//! - An import, on the keychain thread ([`super::off_actor`], so it waits its turn with the
//!   other commands): the finder again, then each importable server of the app has its values
//!   written to the keychain, after one native confirm listing every server and what it starts
//!   (`McpLaunchConfirm::confirm_import`, A1's confirm): Allow imports them turned on; Cancel,
//!   or no confirm wired, imports nothing and answers `mcp.save_declined`. An import with
//!   nothing to import asks nothing. Then on the actor the servers the settings take go into
//!   `settings.json` in one write (`Effective::add_mcp_servers`). A server whose name the list
//!   holds in any case is skipped (`already_added`), never renamed or replaced; one the rules
//!   refuse is skipped too, and the values of a skipped server are put back.
//! - All or nothing: a keychain failure, or a settings write that fails after the keychain
//!   writes, puts back every value the import wrote (deleting new items, restoring replaced
//!   ones) and saves no server.
//! - The view names servers and apps, and for `command_not_found` the program's file name; never
//!   a value, a config file's path or a project folder.
//! - The result goes in the view as `last_import` with the command's id, then the command answers
//!   `Done`. Logs carry counts only; a value never leaves the keychain thread but into the store.

use clauseo_connectors::{SecretStore, StoreError};
use clauseo_protocol::{
    ErrorCode, MCP_SKIPPED_MAX, McpImportResult, McpImportSource, McpSkipReason, McpSkipped,
};
use clauseo_settings::mcp_import::{CandidateKind, ImportCandidate, ImportSecret, Problem, Source};
use clauseo_settings::{
    KeychainRef, McpRefusal, McpServerLaunch, McpTransport, McpValue, McpValueKind, SystemFiles,
    keychain_account,
};
use zeroize::Zeroizing;

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::keychain::{self, Step, Undo};
use super::{McpInput, finder};
use crate::config::mcp::McpLaunchAsk;
use crate::error::Error;

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;

/// The app a source belongs to: Claude Code's user and project scopes are one.
pub(super) fn source_of(source: &Source) -> McpImportSource {
    match source {
        Source::ClaudeDesktop => McpImportSource::ClaudeDesktop,
        Source::ClaudeCodeUser | Source::ClaudeCodeProject { .. } => McpImportSource::ClaudeCode,
        Source::Codex => McpImportSource::Codex,
    }
}

/// Why a candidate with `problems` is skipped (the first reason in declaration order), with the
/// program for `command_not_found`; `None` when it imports.
pub(super) fn skip_of(problems: &[Problem]) -> Option<(McpSkipReason, Option<String>)> {
    McpSkipReason::ALL.into_iter().find_map(|reason| {
        problems.iter().find_map(|problem| match (reason, problem) {
            (McpSkipReason::CommandNotFound, Problem::CommandNotFound { original }) => {
                Some((reason, Some(program_name(original))))
            }
            (McpSkipReason::AlreadyAdded, Problem::AlreadyAdded)
            | (McpSkipReason::NotApproved, Problem::NotApproved)
            | (McpSkipReason::UnsupportedTransport, Problem::UnsupportedTransport { .. })
            | (McpSkipReason::UnresolvedVariable, Problem::UnresolvedVariable { .. })
            | (McpSkipReason::Unreadable, Problem::Unreadable) => Some((reason, None)),
            _ => None,
        })
    })
}

/// The file name of a program as a config wrote it (`/Users/ana/bin/uvx` gives `uvx`), so the
/// view names what to install and holds no path.
fn program_name(original: &str) -> String {
    let name = original.rsplit(['/', '\\']).next().unwrap_or(original);
    if name.is_empty() { original } else { name }.to_owned()
}

/// One server ready to import: its entry, and the keychain writes of its values.
struct Ready {
    name: String,
    server: McpServerLaunch,
    steps: Vec<Step>,
}

impl Ready {
    /// What the confirm shows for it: its name, and the program with its arguments or the URL.
    fn ask(&self) -> McpLaunchAsk {
        let (program, url) = match &self.server.transport {
            McpTransport::Local { command, args, .. } => (
                std::iter::once(command).chain(args).cloned().collect(),
                None,
            ),
            McpTransport::Remote { url, .. } => (Vec::new(), Some(url.clone())),
        };
        McpLaunchAsk {
            server: self.name.clone(),
            program,
            url,
        }
    }
}

/// One server whose values are in the keychain, waiting for its settings write.
pub(crate) struct Written {
    name: String,
    server: McpServerLaunch,
    undo: Undo,
}

/// What an import's keychain thread did.
pub(crate) struct Imported {
    source: McpImportSource,
    written: Vec<Written>,
    skipped: Vec<McpSkipped>,
    /// The servers the read cap left unread.
    left_out: usize,
}

/// Why an import's keychain thread saved nothing.
pub(crate) enum ImportFailed {
    /// The store's error, or no store.
    Store(Option<StoreError>),
    /// The finder did not answer in time (`finder.rs`).
    Finder,
    /// The lawyer did not allow the programs, or no confirm is wired.
    Declined,
}

/// `McpCommand::Import`: refused while the firm's policy sets the list (the caller refused
/// `managed-mcp.json` already), else the finder and the keychain off the actor.
pub(super) fn import(
    actor: &mut Actor,
    ctx: CommandCtx,
    source: McpImportSource,
) -> Result<(), Error> {
    if actor.effective.mcp_locked() {
        return Err(Error::Settings(clauseo_settings::Error::Locked {
            key: "mcp.servers".to_owned(),
        }));
    }
    let Some(find) = actor.wiring.mcp_find.clone() else {
        let nothing = Imported {
            source,
            written: Vec::new(),
            skipped: Vec::new(),
            left_out: 0,
        };
        on_imported(actor, ctx, Ok(Box::new(nothing)));
        return Ok(());
    };
    let existing: Vec<String> = actor.effective.mcp_servers().keys().cloned().collect();
    let store = actor.wiring.mcp_secrets.clone();
    let busy = std::sync::Arc::clone(&actor.mcp.import_busy);
    let confirm = actor.wiring.mcp_confirm.clone();
    super::off_actor(actor, Some(ctx), move || {
        let existing_names = existing.clone();
        let job = move || find.find(&existing);
        let Some(found) = finder::bounded(job, &busy, finder::TIMEOUT) else {
            let result = Err(ImportFailed::Finder);
            return McpInput::Imported { ctx, result };
        };
        let left_out = found.left_out;
        let (mut ready, skipped) = pick(found.candidates, source, &existing_names);
        if !ready.is_empty() {
            let asks: Vec<McpLaunchAsk> = ready.iter().map(Ready::ask).collect();
            if !confirm.is_some_and(|confirm| confirm.confirm_import(&asks)) {
                let result = Err(ImportFailed::Declined);
                return McpInput::Imported { ctx, result };
            }
            for one in &mut ready {
                one.server.enabled = true;
            }
        }
        let result = write(store.as_deref(), ready)
            .map(|written| {
                Box::new(Imported {
                    source,
                    written,
                    skipped,
                    left_out,
                })
            })
            .map_err(ImportFailed::Store);
        McpInput::Imported { ctx, result }
    })
}

/// The candidates of `source`: those that import, ready, and those skipped, with why.
fn pick(
    candidates: Vec<ImportCandidate>,
    source: McpImportSource,
    existing: &[String],
) -> (Vec<Ready>, Vec<McpSkipped>) {
    let mut ready = Vec::new();
    let mut skipped = Vec::new();
    for candidate in candidates {
        if !candidate.sources.iter().any(|of| source_of(of) == source) {
            continue;
        }
        // A name the list holds in any case, or one this import takes already in another case,
        // is never replaced or renamed, and gets no keychain write: Credential Manager's accounts
        // are case-blind.
        let held = existing
            .iter()
            .chain(ready.iter().map(|one: &Ready| &one.name))
            .any(|name| name.eq_ignore_ascii_case(&candidate.name));
        if held {
            skipped.push(skipped_as(candidate.name, McpSkipReason::AlreadyAdded));
            continue;
        }
        if let Some((reason, program)) = skip_of(&candidate.problems) {
            skipped.push(McpSkipped {
                name: candidate.name,
                reason,
                program,
            });
            continue;
        }
        match prepare(candidate) {
            Ok(one) => ready.push(one),
            Err(one) => skipped.push(one),
        }
    }
    (ready, skipped)
}

/// A candidate as the entry `SaveServer` would add, turned off until the confirm allows it,
/// each value bound for the keychain; skipped when its command is not text or the settings'
/// rules refuse it.
fn prepare(candidate: ImportCandidate) -> Result<Ready, McpSkipped> {
    let ImportCandidate { name, kind, .. } = candidate;
    let (transport, steps) = match kind {
        CandidateKind::Local { command, args, env } => {
            let Some(command) = command.to_str().map(str::to_owned) else {
                return Err(skipped(name, McpSkipReason::Unreadable, None));
            };
            let (env, steps) = values(&name, McpValueKind::Env, env);
            let args = args.iter().map(|arg| arg.expose().to_owned()).collect();
            (McpTransport::Local { command, args, env }, steps)
        }
        CandidateKind::Remote { url, headers } => {
            let (headers, steps) = values(&name, McpValueKind::Header, headers);
            let url = url.expose().to_owned();
            (McpTransport::Remote { url, headers }, steps)
        }
    };
    // Off until the confirm's Allow turns it on (`import`).
    let server = McpServerLaunch {
        transport,
        enabled: false,
    };
    if let Err(refusal) = clauseo_settings::check_server(&name, &server) {
        return Err(refused(name, refusal, &server));
    }
    Ok(Ready {
        name,
        server,
        steps,
    })
}

/// The entry's `{"keychain": true}` markers, and the writes that put each value in its account.
fn values(
    server: &str,
    kind: McpValueKind,
    pairs: Vec<(String, ImportSecret)>,
) -> (std::collections::BTreeMap<String, McpValue>, Vec<Step>) {
    let mut markers = std::collections::BTreeMap::new();
    let mut steps = Vec::new();
    for (var, value) in pairs {
        steps.push(Step::Set {
            account: keychain_account(server, kind, &var),
            value: Zeroizing::new(value.expose().to_owned()),
        });
        markers.insert(var, McpValue::Keychain(KeychainRef::default()));
    }
    (markers, steps)
}

fn skipped_as(name: String, reason: McpSkipReason) -> McpSkipped {
    skipped(name, reason, None)
}

fn skipped(name: String, reason: McpSkipReason, program: Option<String>) -> McpSkipped {
    McpSkipped {
        name,
        reason,
        program,
    }
}

/// The skip a settings refusal of `server` gives.
fn refused(name: String, refusal: McpRefusal, server: &McpServerLaunch) -> McpSkipped {
    match refusal {
        McpRefusal::NameTaken => skipped(name, McpSkipReason::AlreadyAdded, None),
        McpRefusal::UrlInvalid => skipped(name, McpSkipReason::UnsupportedTransport, None),
        McpRefusal::CommandNotFound => {
            let program = server.command().map(program_name);
            skipped(name, McpSkipReason::CommandNotFound, program)
        }
        McpRefusal::NameEmpty
        | McpRefusal::NameReserved
        | McpRefusal::NameInvalid
        | McpRefusal::CommandInvalid
        | McpRefusal::HeaderNameInvalid
        | McpRefusal::EnvNameInvalid
        | McpRefusal::Unknown
        | _ => skipped(name, McpSkipReason::Unreadable, None),
    }
}

/// Writes each server's values; on a failure every value written so far is put back.
fn write(
    store: Option<&dyn SecretStore>,
    ready: Vec<Ready>,
) -> Result<Vec<Written>, Option<StoreError>> {
    let mut written: Vec<Written> = Vec::new();
    for Ready {
        name,
        server,
        steps,
    } in ready
    {
        let undo = if steps.is_empty() {
            Undo::default()
        } else {
            // No store and a value to keep: nothing was written before either.
            let store = store.ok_or(None)?;
            match keychain::run(store, steps) {
                Ok(undo) => undo,
                Err(error) => {
                    for one in written.into_iter().rev() {
                        keychain::put_back(store, one.undo);
                    }
                    return Err(Some(error));
                }
            }
        };
        written.push(Written { name, server, undo });
    }
    Ok(written)
}

/// The keychain thread ended: the servers the settings take saved in one write, the result in
/// the view, then the answer, after the put-back of values whose server was not saved. A write
/// that fails puts back every value the import wrote, and answers its error with nothing saved.
pub(super) fn on_imported(
    actor: &mut Actor,
    ctx: CommandCtx,
    result: Result<Box<Imported>, ImportFailed>,
) {
    let Imported {
        source,
        written,
        mut skipped,
        left_out,
    } = match result {
        Ok(imported) => *imported,
        Err(failed) => {
            actor.finish_command(ctx.id, Err(import_failed(failed)));
            return;
        }
    };
    if actor.effective.mcp_locked() {
        // The firm's list arrived while the keychain thread ran.
        let undos = written.into_iter().map(|one| one.undo).collect();
        let locked = clauseo_settings::Error::Locked {
            key: "mcp.servers".to_owned(),
        };
        put_back(actor, undos, Some((ctx, Error::Settings(locked))));
        return;
    }
    let mut batch: Vec<(String, McpServerLaunch)> = Vec::new();
    let mut kept = Vec::new();
    let mut undos = Vec::new();
    for Written { name, server, undo } in written {
        let twice = batch
            .iter()
            .any(|(held, _)| held.eq_ignore_ascii_case(&name));
        let refusal = if twice {
            Some(McpRefusal::NameTaken)
        } else {
            match actor.effective.check_mcp_server(None, &name, &server) {
                Ok(()) => None,
                Err(clauseo_settings::Error::Mcp { refusal, .. }) => Some(refusal),
                Err(_) => Some(McpRefusal::Unknown),
            }
        };
        if let Some(refusal) = refusal {
            skipped.push(refused(name, refusal, &server));
            undos.push(undo);
        } else {
            batch.push((name, server));
            kept.push(undo);
        }
    }
    if !batch.is_empty()
        && let Err(error) = actor.effective.add_mcp_servers(&SystemFiles, &batch)
    {
        tracing::warn!(error = %crate::log_file::Cause(&error), "imported MCP servers were not saved");
        undos.extend(kept);
        put_back(actor, undos, Some((ctx, Error::from(error))));
        return;
    }
    let imported: Vec<String> = batch.into_iter().map(|(name, _)| name).collect();
    actor
        .mcp
        .missing
        .retain(|(held, _)| !imported.contains(held));
    skipped.sort_by(|a, b| a.name.cmp(&b.name));
    tracing::info!(
        target: crate::log_file::LIFECYCLE,
        ?source,
        imported = imported.len(),
        skipped = skipped.len(),
        left_out,
        "MCP servers imported"
    );
    // The view names at most `MCP_SKIPPED_MAX` skipped servers; the rest, and those the read
    // cap left unread, are counted.
    let past = skipped.len().saturating_sub(MCP_SKIPPED_MAX);
    skipped.truncate(MCP_SKIPPED_MAX);
    let others = u32::try_from(past.saturating_add(left_out)).unwrap_or(u32::MAX);
    actor.mcp.last_import = Some(McpImportResult {
        command: ctx.id,
        source,
        imported,
        skipped,
        others,
    });
    super::changed(actor, ctx);
    put_back(actor, undos, None);
}

/// The answer to an import whose keychain thread saved nothing.
fn import_failed(failed: ImportFailed) -> Error {
    match failed {
        ImportFailed::Store(error) => {
            if let Some(error) = error {
                tracing::warn!(%error, "imported MCP servers' keychain values were not saved");
            }
            Error::refused(
                ErrorCode::McpSecretStoreFailed,
                "the keychain did not save the imported values",
            )
        }
        ImportFailed::Declined => Error::refused(
            ErrorCode::McpSaveDeclined,
            "the lawyer did not allow the imported programs",
        ),
        ImportFailed::Finder => {
            Error::Internal("the other apps' MCP settings did not read in time".to_owned())
        }
    }
}

/// Puts back the values of servers not saved, off the actor, then gives `answer`; with nothing
/// to put back, `answer` at once.
fn put_back(actor: &mut Actor, undos: Vec<Undo>, answer: Option<(CommandCtx, Error)>) {
    let undos: Vec<Undo> = undos.into_iter().filter(|undo| !undo.is_empty()).collect();
    let store = actor.wiring.mcp_secrets.clone();
    let (Some(store), false) = (store, undos.is_empty()) else {
        if let Some((ctx, error)) = answer {
            actor.finish_command(ctx.id, Err(error));
        }
        return;
    };
    let ctx = answer.as_ref().map(|(ctx, _)| *ctx);
    let started = super::off_actor(actor, ctx, move || {
        for undo in undos {
            keychain::put_back(store.as_ref(), undo);
        }
        McpInput::Tidied { answer }
    });
    if let Err(error) = started {
        tracing::warn!(error = %crate::log_file::Cause(&error), "the keychain put-back did not start");
        if let Some(ctx) = ctx {
            actor.finish_command(ctx.id, Err(error));
        }
    }
}
