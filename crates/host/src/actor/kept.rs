//! The lawyer's view of the rules "Always for this matter" kept (`agent/always.rs`):
//! `HostQuery::KeptPermissions` lists a matter's rules, each read into a kind and a subject
//! ([`describe`]), and `HostCommand::RemoveKeptPermission` takes one back. Both read or write the
//! matter's file on a thread of their own; a removal's outcome then comes back to the actor
//! ([`Input::Kept`]) to answer the command. Logs name no rule: a command can be the lawyer's own
//! words.
//!
//! The lawyer's removal always wins. On the actor, before its file is touched, no running agent
//! of the matter applies the rule any more (`turn::always::forget`), whether or not the file
//! held it (an add's write can still be pending, or have failed); the [`Ledger`] drops it from
//! the rules a launch read before the removal, when its agent is built; and its file operation
//! carries the ledger's sequence number, so an add that started earlier can't put it back
//! (`agent/always/store.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{CommandOutcome, KeptKind, KeptPermission, MatterId, QueryResult};
use futures::task::LocalSpawnExt;

use super::Actor;
use super::commands::{CommandCtx, check_matter};
use super::input::{Input, Reply};
use crate::agent::always;
use crate::agent::codex::prepare::{Job, LAUNCH_WAIT, off_actor};
use crate::error::Error;
use crate::log_file::Cause;

/// The job a read or a removal runs as: one small file, given a launch's wait, since an
/// antivirus scan can slow it.
const JOB: Job = Job {
    what: "kept permissions",
    matter: None,
    wait: LAUNCH_WAIT,
};

/// The lawyer's adds and removals of kept rules this run, numbered in the order the actor saw
/// them, and the rules removed since, by matter.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    /// The last number given.
    seq: u64,
    /// Each matter's rules the lawyer removed and did not keep again since.
    removed: BTreeMap<MatterId, BTreeSet<String>>,
}

impl Ledger {
    /// The lawyer kept `rule` for `matter` (again): its add's number.
    pub(crate) fn keep(&mut self, matter: MatterId, rule: &str) -> u64 {
        if let Some(removed) = self.removed.get_mut(&matter) {
            removed.remove(rule);
        }
        self.next()
    }

    /// The lawyer removed `rule` from `matter`: its removal's number.
    fn remove(&mut self, matter: MatterId, rule: &str) -> u64 {
        self.removed
            .entry(matter)
            .or_default()
            .insert(rule.to_owned());
        self.next()
    }

    /// `rules`, read for a launch of `matter`, without those the lawyer removed since.
    pub(crate) fn drop_removed(&self, matter: MatterId, rules: &mut BTreeSet<String>) {
        if let Some(removed) = self.removed.get(&matter) {
            rules.retain(|rule| !removed.contains(rule));
        }
    }

    fn next(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }
}

/// A removal's outcome, from its thread.
#[derive(Debug)]
pub(crate) struct Removed {
    ctx: CommandCtx,
    matter: MatterId,
    outcome: Result<bool, Error>,
}

/// `HostQuery::KeptPermissions`: the matter's rules, read off the actor, answered from a task of
/// its own.
pub(super) fn list(actor: &Actor, matter: MatterId, reply: Reply<QueryResult>) {
    let folder = match check_matter(actor, matter) {
        Ok(entry) => entry.row.folder_path.clone(),
        Err(error) => {
            let _gone = reply.send(Err(error));
            return;
        }
    };
    let file = always::file_of(actor.effective.data_dir(), matter);
    let clock = Arc::clone(&actor.clock);
    let spawned = actor.spawner.spawn_local(async move {
        let read = off_actor(clock, JOB, move || Ok(always::list(&file))).await;
        let answer = read.map(|rules| {
            let roots = always::matter_roots([folder.as_str()], cfg!(windows));
            let kept = rules
                .iter()
                .filter_map(|rule| describe(rule, &roots, cfg!(windows)))
                .collect();
            QueryResult::KeptPermissions(kept)
        });
        let _gone = reply.send(answer);
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "the host pool refused a kept permissions read");
    }
}

/// `HostCommand::RemoveKeptPermission`: the rule is forgotten and recorded on the actor (see
/// the module doc), then removed from the file off the actor, then [`on_removed`].
pub(super) fn remove(actor: &mut Actor, ctx: &CommandCtx, matter: MatterId, rule: String) {
    if let Err(error) = check_matter(actor, matter) {
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    super::turn::always::forget(actor, matter, &rule);
    let seq = actor.kept.remove(matter, &rule);
    let file: PathBuf = always::file_of(actor.effective.data_dir(), matter);
    let clock = Arc::clone(&actor.clock);
    let ctx = *ctx;
    actor.spawn_post(async move {
        let removed = off_actor(clock, JOB, move || {
            always::remove_now(&file, &rule, seq).map_err(|error| {
                tracing::warn!(error = %Cause(&error), "a kept permission was not removed");
                Error::Internal("the kept permission could not be removed".to_owned())
            })
        })
        .await;
        Input::Kept(Box::new(Removed {
            ctx,
            matter,
            outcome: removed,
        }))
    });
}

/// A removal came back: the command is answered. The agents forgot the rule when it was asked
/// for; forgetting it again here would also drop one the lawyer kept again since.
pub(super) fn on_removed(actor: &mut Actor, done: Removed) {
    let Removed {
        ctx,
        matter,
        outcome: removed,
    } = done;
    tracing::info!(%matter, removed = ?removed.as_ref().ok(), "the lawyer removed a kept permission");
    actor.finish_command(ctx.id, removed.map(|_was| CommandOutcome::Done));
}

/// `rule` as the lawyer's list shows it; none for text no kept rule has. A file inside the
/// matter (one of `roots`, `always::matter_roots`) is shown relative to it.
pub(crate) fn describe(rule: &str, roots: &[String], windows: bool) -> Option<KeptPermission> {
    let kept = |kind, subject: &str, server: Option<&str>| KeptPermission {
        rule: rule.to_owned(),
        kind,
        subject: subject.to_owned(),
        server: server.map(str::to_owned),
    };
    if let Some(name) = rule.strip_prefix("mcp__") {
        let (server, tool) = name.split_once("__")?;
        return Some(kept(KeptKind::Tool, tool, Some(server)));
    }
    let (tool, content) = rule.strip_suffix(')')?.split_once('(')?;
    match tool {
        "Bash" => Some(kept(KeptKind::Run, content, None)),
        "WebFetch" => Some(kept(KeptKind::Site, content.strip_prefix("domain:")?, None)),
        "Edit" | "Read" => {
            let kind = if tool == "Edit" {
                KeptKind::Edit
            } else {
                KeptKind::Read
            };
            let subject = inside(content, roots, windows)
                .or_else(|| Some(always::rule_path(rule, windows)?.display().to_string()))?;
            Some(kept(kind, &subject, None))
        }
        _ => None,
    }
}

/// `content`'s path relative to the first of `roots` it is strictly inside, as written there;
/// on Windows compared in any case.
fn inside(content: &str, roots: &[String], windows: bool) -> Option<String> {
    let fold = |text: &str| {
        if windows {
            text.to_lowercase()
        } else {
            text.to_owned()
        }
    };
    let folded = fold(content);
    roots.iter().find_map(|root| {
        let root = fold(root);
        let rest = folded.strip_prefix(root.as_str())?.strip_prefix('/')?;
        // Lower-casing can change a text's length; then the original spelling can't be cut.
        let start = content.len().checked_sub(rest.len())?;
        (!rest.is_empty() && folded.len() == content.len())
            .then(|| content.get(start..).map(str::to_owned))
            .flatten()
    })
}

#[cfg(test)]
mod tests;
