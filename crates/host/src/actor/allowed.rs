//! The lawyer's "Allow this site" (decision record 0053, PR 2) on the actor: `AllowBlockedSite`,
//! `RevokeAllowedSite`, the `AllowedSites` query, and the notices an allow can name.
//!
//! - Who: the commands reach the host only through `HostHandle::dispatch`, which the desktop
//!   bridge alone calls, and the bridge admits them from its windows by label (`admit.rs`:
//!   `AllowBlockedSite` from `main`, the revoke and the query from `main` and `settings`). An
//!   agent has no way to send a host command, and never sees a `block_id`: the notice goes
//!   to app subscribers only.
//! - What: an allow names a notice by its `block_id`. The host keeps the notices it sent
//!   (`block_id` to the host and matter), the latest [`MAX_NOTICES_PER_THREAD`] of each thread
//!   and [`MAX_NOTICES`] in all, so an agent looping in one thread cannot push out another's;
//!   an allow covers exactly the host the notice named, never one the client picked. An id it
//!   no longer keeps (or never sent) is `blocklist.notice_expired`. None is dropped when its
//!   thread ends: the lawyer may allow from a notice after the turn or the session is over.
//! - Policy: while `network.allow_unsafe_site_override` is off, `AllowBlockedSite` is
//!   `blocklist.override_off` and no stored allow is honoured (the views are empty), though
//!   none is deleted; revoking and listing work either way. [`refresh`] runs at the start and
//!   on every `SettingsChanged` the actor publishes, so a settings change applies at the
//!   proxies' next request. The policy file is read only at start, so a policy change takes
//!   effect at the next start; a future runtime reload would publish `SettingsChanged`.
//! - Limit: a new allow past `file::MAX_HOSTS` (10,000) is `command.invalid` (a count past its
//!   most, as 0003 words that code) and changes nothing; allowing a host already there is fine.
//! - Storage (`allowed/saving.rs`): an allow or revoke takes effect in memory at once, then its
//!   save runs on the allowed sites' own thread, in order, and the command is answered once the
//!   save is: `Done` when saved, else `blocklist.allow_not_saved` or
//!   `blocklist.revoke_not_saved` (the change lasts until Clauseo closes).
//! - Log: one info line per command, naming the host, the scope and the matter.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};

use clauseo_protocol::{
    AllowScope, BlockedSiteId, CommandId, ErrorCode, MatterId, QueryResult, ThreadId,
};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Reply;
use crate::blocklist::AllowedSites;
use crate::blocklist::allowed::file::{self, Stored};
use crate::blocklist::allowed::integrity::Job;
use crate::error::Error;

mod saving;
#[cfg(test)]
mod tests;

pub(crate) use saving::{AllowedInput, busy, on_input, shutdown, start};

/// How many of each thread's latest notices an allow can name.
pub(crate) const MAX_NOTICES_PER_THREAD: usize = 64;
/// How many notices are kept in all, the oldest dropped first: a backstop for memory.
pub(crate) const MAX_NOTICES: usize = 1024;

/// `AllowBlockedSite`'s refusal while overrides are off.
pub(crate) const OVERRIDE_OFF: &str =
    "Unblocking sites on Clauseo's unsafe-site list is turned off, usually by your firm.";
/// `AllowBlockedSite`'s refusal for a notice the host no longer keeps.
pub(crate) const UNKNOWN_NOTICE: &str =
    "This notice has expired. Open the site again, then allow it from the new notice.";
/// An allow past the most allows kept.
pub(crate) const TOO_MANY: &str = "Clauseo already keeps the most allowed sites it can, so this site stays blocked. Remove some in \
     Settings, then try again.";

/// One notice an allow can name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Notice {
    host: String,
    matter: MatterId,
}

/// The allowed sites on the actor: the latest notices, what is stored, and the views.
#[derive(Debug)]
pub(crate) struct AllowState {
    notices: VecDeque<(BlockedSiteId, ThreadId, Notice)>,
    stored: Stored,
    path: PathBuf,
    sites: Arc<AllowedSites>,
    /// What the next start would honour, when known (`saving.rs`).
    persisted: Option<Stored>,
    /// The thread's job queue; `None` without a thread, or after giving up on its read.
    writer: Option<mpsc::Sender<Job>>,
    /// The commands whose saves are out, in the order they were sent.
    waiting: VecDeque<(CommandId, Change, Stored)>,
    /// While the start-up read is out: the commands sent meanwhile, in order.
    opening: Option<Vec<Held>>,
    /// Dropped to end the read's deadline early (`saving.rs`).
    read_deadline: Option<futures::channel::oneshot::Sender<()>>,
    /// The secret store, for the stop's last delete (`saving.rs`).
    store: Option<saving::Secrets>,
    /// Set when the stop gives up on the queue: the thread writes nothing more.
    abandoned: Arc<AtomicBool>,
    /// Where the host's stop is with the queue.
    stop: saving::Stop,
    /// Dropped to end the stop's wait early.
    stop_deadline: Option<futures::channel::oneshot::Sender<()>>,
}

/// What a command changed, to answer it once its save is known.
#[derive(Debug)]
enum Change {
    Allow,
    Revoke {
        host: String,
        scope: AllowScope,
        matter: Option<MatterId>,
    },
}

/// An allow or revoke sent before the start-up read was back.
#[derive(Debug)]
enum Held {
    Allow {
        id: CommandId,
        block_id: BlockedSiteId,
        scope: AllowScope,
    },
    Revoke {
        id: CommandId,
        host: String,
        scope: AllowScope,
        matter: Option<MatterId>,
    },
}

impl AllowState {
    /// The views every launch's proxy takes its matter's allowed sites from.
    pub(crate) fn sites(&self) -> &Arc<AllowedSites> {
        &self.sites
    }

    /// Nothing noticed or stored yet; the file in `data_dir`, read by [`start`].
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            notices: VecDeque::new(),
            stored: Stored::default(),
            path: file::path(data_dir),
            sites: Arc::default(),
            persisted: None,
            writer: None,
            waiting: VecDeque::new(),
            opening: None,
            read_deadline: None,
            store: None,
            abandoned: Arc::default(),
            stop: saving::Stop::Running,
            stop_deadline: None,
        }
    }

    /// Keeps the notice `block_id` for `host` in `matter`, sent in `thread`: past
    /// [`MAX_NOTICES_PER_THREAD`] it drops that thread's oldest, else past [`MAX_NOTICES`] the
    /// oldest of all. A re-sent notice (same id) is kept once.
    pub(crate) fn noticed(
        &mut self,
        block_id: BlockedSiteId,
        host: &str,
        matter: MatterId,
        thread: ThreadId,
    ) {
        if self.notices.iter().any(|(id, _, _)| *id == block_id) {
            return;
        }
        let own = |(_, of, _): &(BlockedSiteId, ThreadId, Notice)| *of == thread;
        if self.notices.iter().filter(|entry| own(entry)).count() >= MAX_NOTICES_PER_THREAD {
            if let Some(oldest) = self.notices.iter().position(own) {
                self.notices.remove(oldest);
            }
        } else if self.notices.len() >= MAX_NOTICES {
            self.notices.pop_front();
        }
        let notice = Notice {
            host: host.to_owned(),
            matter,
        };
        self.notices.push_back((block_id, thread, notice));
    }

    fn notice(&self, block_id: BlockedSiteId) -> Option<&Notice> {
        self.notices
            .iter()
            .find(|(id, _, _)| *id == block_id)
            .map(|(_, _, notice)| notice)
    }
}

/// Publishes what is in force now to every launch's view: the stored allows, or none while
/// overrides are off.
pub(crate) fn refresh(actor: &Actor) {
    let in_force = actor.allowed.stored.in_force(honoured(actor));
    actor.allowed.sites.publish(in_force);
}

/// `network.allow_unsafe_site_override` in force.
fn honoured(actor: &Actor) -> bool {
    actor
        .effective
        .settings()
        .network
        .allow_unsafe_site_override
}

fn scope_word(scope: AllowScope) -> &'static str {
    match scope {
        AllowScope::Matter => "matter",
        AllowScope::Always => "always",
    }
}

/// `AllowBlockedSite`: answers once its save is known (`Done`, or not saved), or a refusal;
/// held while the start-up read is out.
pub(super) fn allow(
    actor: &mut Actor,
    ctx: &CommandCtx,
    block_id: BlockedSiteId,
    scope: AllowScope,
) {
    if let Some(held) = &mut actor.allowed.opening {
        held.push(Held::Allow {
            id: ctx.id,
            block_id,
            scope,
        });
        return;
    }
    run_allow(actor, ctx.id, block_id, scope);
}

fn run_allow(actor: &mut Actor, id: CommandId, block_id: BlockedSiteId, scope: AllowScope) {
    match try_allow(actor, block_id, scope) {
        Ok(()) => saving::save(actor, id, Change::Allow),
        Err(error) => actor.finish_command(id, Err(error)),
    }
}

/// Puts the allow in memory and in force; the save follows.
fn try_allow(actor: &mut Actor, block_id: BlockedSiteId, scope: AllowScope) -> Result<(), Error> {
    if !honoured(actor) {
        return Err(Error::refused(
            ErrorCode::BlocklistOverrideOff,
            OVERRIDE_OFF,
        ));
    }
    let Some(notice) = actor.allowed.notice(block_id).cloned() else {
        return Err(Error::refused(
            ErrorCode::BlocklistNoticeExpired,
            UNKNOWN_NOTICE,
        ));
    };
    let mut stored = actor.allowed.stored.clone();
    if stored.allow(&notice.host, scope, notice.matter, actor.now().wall) {
        if stored.count() > file::MAX_HOSTS {
            return Err(Error::refused(ErrorCode::CommandInvalid, TOO_MANY));
        }
        actor.allowed.stored = stored;
        refresh(actor);
    }
    tracing::info!(
        host = %notice.host,
        scope = scope_word(scope),
        matter = %notice.matter,
        "blocklist: the lawyer allowed a site on the block list"
    );
    Ok(())
}

/// `RevokeAllowedSite`: takes effect at once and answers once its save is known; held while
/// the start-up read is out.
pub(super) fn revoke(
    actor: &mut Actor,
    ctx: &CommandCtx,
    host: &str,
    scope: AllowScope,
    matter: Option<MatterId>,
) {
    if let Some(held) = &mut actor.allowed.opening {
        held.push(Held::Revoke {
            id: ctx.id,
            host: host.to_owned(),
            scope,
            matter,
        });
        return;
    }
    run_revoke(actor, ctx.id, host, scope, matter);
}

fn run_revoke(
    actor: &mut Actor,
    id: CommandId,
    host: &str,
    scope: AllowScope,
    matter: Option<MatterId>,
) {
    let removed = actor.allowed.stored.revoke(host, scope, matter);
    if removed {
        refresh(actor);
    }
    let shown = matter.map(|matter| matter.to_string());
    tracing::info!(
        host,
        scope = scope_word(scope),
        matter = shown.as_deref().unwrap_or("none"),
        removed,
        "blocklist: the lawyer took back an allowed site"
    );
    // Saved even when nothing was removed: the save is the whole state, and a retry after one
    // that failed must put it right.
    let change = Change::Revoke {
        host: host.to_owned(),
        scope,
        matter,
    };
    saving::save(actor, id, change);
}

/// `HostQuery::AllowedSites`: every stored allow, honoured or not, each saying whether it is
/// saved; none while the start-up read is out.
pub(super) fn list(actor: &Actor, reply: Reply<QueryResult>) {
    let allowed = &actor.allowed;
    let sites = allowed.stored.list(allowed.persisted.as_ref());
    let _gone = reply.send(Ok(QueryResult::AllowedSites(sites)));
}
