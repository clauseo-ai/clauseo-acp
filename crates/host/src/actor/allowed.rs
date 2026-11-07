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
//! - Storage (`blocklist/allowed/file.rs`): an allow is written before it takes effect (a
//!   failed write is `store.io` and allows nothing); a revoke takes effect first and is then
//!   written (a failed write is `store.io`, and the site stays blocked until Clauseo restarts).
//!   A revoke always writes, so retrying one whose write failed saves it even though memory no
//!   longer holds the allow. Both writes run on the actor thread, as `hints.json`'s does.
//! - Log: one info line per command, naming the host, the scope and the matter.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{
    AllowScope, BlockedSiteId, CommandOutcome, ErrorCode, MatterId, QueryResult, ThreadId,
};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Reply;
use crate::blocklist::AllowedSites;
use crate::blocklist::allowed::file::{self, Stored};
use crate::error::Error;
use crate::log_file::Cause;

#[cfg(test)]
mod tests;

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
/// An allow whose write failed.
pub(crate) const ALLOW_NOT_SAVED: &str =
    "Clauseo couldn't save the allowed site, so it stays blocked. Try again.";
/// A revoke whose write failed.
pub(crate) const REVOKE_NOT_SAVED: &str = "The site is blocked again, but Clauseo couldn't save that, so it may be allowed again \
     after a restart. Try again.";

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
}

impl AllowState {
    /// Nothing noticed or stored yet; the file in `data_dir`, read by [`start`].
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            notices: VecDeque::new(),
            stored: Stored::default(),
            path: file::path(data_dir),
            sites: Arc::default(),
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

/// At the start: `sites` (the seams' views, or a fresh one) and the stored allows, published.
pub(super) fn start(actor: &mut Actor, sites: Arc<AllowedSites>) {
    actor.allowed.sites = sites;
    actor.allowed.stored = file::read(&actor.allowed.path);
    refresh(actor);
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

/// `AllowBlockedSite`: answers `Done`, with no receipt, or a refusal.
pub(super) fn allow(
    actor: &mut Actor,
    ctx: &CommandCtx,
    block_id: BlockedSiteId,
    scope: AllowScope,
) {
    let result = try_allow(actor, block_id, scope).map(|()| CommandOutcome::Done);
    actor.finish_command(ctx.id, result);
}

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
        // On the actor thread, fsync and all, as `connectors/hints.json`'s save is.
        if let Err(error) = file::write(&actor.allowed.path, &stored) {
            tracing::warn!(error = %Cause(&error), "blocklist: allowed.json not written");
            return Err(Error::refused(ErrorCode::StoreIo, ALLOW_NOT_SAVED));
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

/// `RevokeAllowedSite`: answers `Done`, with no receipt, or `store.io` when the write failed.
pub(super) fn revoke(
    actor: &mut Actor,
    ctx: &CommandCtx,
    host: &str,
    scope: AllowScope,
    matter: Option<MatterId>,
) {
    let removed = actor.allowed.stored.revoke(host, scope, matter);
    if removed {
        refresh(actor);
    }
    // Written even when nothing was removed: an earlier revoke whose write failed left the file
    // holding the allow, and "Try again" must put it right. On the actor thread, fsync and all,
    // as `connectors/hints.json`'s save is: a small file, written only on the lawyer's act.
    let mut result = Ok(CommandOutcome::Done);
    if let Err(error) = file::write(&actor.allowed.path, &actor.allowed.stored) {
        tracing::warn!(error = %Cause(&error), "blocklist: allowed.json not written");
        result = Err(Error::refused(ErrorCode::StoreIo, REVOKE_NOT_SAVED));
    }
    let matter = matter.map(|matter| matter.to_string());
    tracing::info!(
        host,
        scope = scope_word(scope),
        matter = matter.as_deref().unwrap_or("none"),
        removed,
        "blocklist: the lawyer took back an allowed site"
    );
    actor.finish_command(ctx.id, result);
}

/// `HostQuery::AllowedSites`: every stored allow, honoured or not.
pub(super) fn list(actor: &Actor, reply: Reply<QueryResult>) {
    let _gone = reply.send(Ok(QueryResult::AllowedSites(actor.allowed.stored.list())));
}
