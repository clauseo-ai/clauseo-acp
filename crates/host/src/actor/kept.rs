//! The lawyer's view of the rules "Always for this matter" kept (`agent/always.rs`):
//! `HostQuery::KeptPermissions` lists a matter's rules, each read into a kind and a subject
//! ([`describe`]), and `HostCommand::RemoveKeptPermission` takes one back. Both read or write the
//! matter's file on a thread of their own; a removal then comes back to the actor
//! ([`Input::Kept`]) so no running agent of the matter applies the rule any more
//! (`turn::always::forget`). Logs name no rule: a command can be the lawyer's own words.

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

/// A removal's outcome, from its thread.
#[derive(Debug)]
pub(crate) struct Removed {
    ctx: CommandCtx,
    matter: MatterId,
    rule: String,
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

/// `HostCommand::RemoveKeptPermission`: the removal off the actor, then [`on_removed`].
pub(super) fn remove(actor: &mut Actor, ctx: &CommandCtx, matter: MatterId, rule: String) {
    if let Err(error) = check_matter(actor, matter) {
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let file: PathBuf = always::file_of(actor.effective.data_dir(), matter);
    let clock = Arc::clone(&actor.clock);
    let ctx = *ctx;
    actor.spawn_post(async move {
        let key = rule.clone();
        let removed = off_actor(clock, JOB, move || {
            always::remove_now(&file, &key).map_err(|error| {
                tracing::warn!(error = %Cause(&error), "a kept permission was not removed");
                Error::Internal("the kept permission could not be removed".to_owned())
            })
        })
        .await;
        Input::Kept(Box::new(Removed {
            ctx,
            matter,
            rule,
            outcome: removed,
        }))
    });
}

/// A removal came back: a rule that was there is forgotten by every agent of its matter.
pub(super) fn on_removed(actor: &mut Actor, done: Removed) {
    let Removed {
        ctx,
        matter,
        rule,
        outcome: removed,
    } = done;
    if let Ok(true) = removed {
        super::turn::always::forget(actor, matter, &rule);
    }
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
