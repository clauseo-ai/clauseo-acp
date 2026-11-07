//! The lawyer's notice of a blocked site (decision record 0053): the egress proxy's sink calls
//! `HostHandle::site_blocked` for each site the block list made it refuse, and the host tells
//! the app with `HostEvent::SiteBlocked`.
//!
//! - Attributed to the session's thread and the turn running on it then (none when no turn
//!   runs).
//! - Once per site per turn: the first refusal of a host in a turn gets a fresh `block_id` and
//!   count 1; the same host again in that turn re-sends the event with the same `block_id` and
//!   the count one higher (`FolderScanned`'s practice: the same event re-sent with new counts).
//!   A new turn starts fresh.
//! - Bounded: a thread keeps the hosts of one turn only, and a turn that is no longer running
//!   is forgotten at the next report, any thread's; at most [`MAX_HOSTS_PER_TURN`] hosts per
//!   turn, after which a new host is dropped, logged once for the turn.
//! - Never logged in the store and in no stream: the app feed only. The host's log file gets
//!   one info line per refusal: the host, the tag and the list's date.

use std::collections::{BTreeMap, HashMap};

use clauseo_protocol::{BlockedSiteId, HostEvent, SiteTag, ThreadId, Timestamp, TurnId};

use super::Actor;
use crate::config::SessionKey;
use crate::error::Error;
use crate::handle::BlockedSite;

#[cfg(test)]
mod tests;

/// The most hosts one turn's notices name; a new host past them is dropped.
pub(crate) const MAX_HOSTS_PER_TURN: usize = 64;

/// The sites refused per thread, for the turn they were refused in.
#[derive(Debug, Default)]
pub(crate) struct Blocks {
    threads: BTreeMap<ThreadId, TurnBlocks>,
}

/// One thread's refused sites, all in `turn`.
#[derive(Debug)]
struct TurnBlocks {
    turn: Option<TurnId>,
    /// By host, folded to lower case: its notice's id and how many refusals it counted.
    hosts: HashMap<String, (BlockedSiteId, u32)>,
    /// A new host was dropped at the cap (logged once).
    capped: bool,
}

impl TurnBlocks {
    fn new(turn: Option<TurnId>) -> Self {
        Self {
            turn,
            hosts: HashMap::new(),
            capped: false,
        }
    }
}

/// What one refusal comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Noted {
    /// Send the notice: its id and count.
    Send { block_id: BlockedSiteId, count: u32 },
    /// The turn already names [`MAX_HOSTS_PER_TURN`] hosts: dropped; `first` for the first drop.
    Capped { first: bool },
}

impl Blocks {
    /// Notes a refusal of `host` for `thread` while `turn` runs. `mint` makes a new notice's
    /// id; it fails only on a host bug, and then nothing is noted.
    pub(crate) fn note(
        &mut self,
        thread: ThreadId,
        turn: Option<TurnId>,
        host: &str,
        mint: impl FnOnce() -> Result<BlockedSiteId, Error>,
    ) -> Result<Noted, Error> {
        let blocks = self
            .threads
            .entry(thread)
            .or_insert_with(|| TurnBlocks::new(turn));
        if blocks.turn != turn {
            *blocks = TurnBlocks::new(turn);
        }
        let key = host.to_ascii_lowercase();
        if let Some((block_id, count)) = blocks.hosts.get_mut(&key) {
            *count = count.saturating_add(1);
            return Ok(Noted::Send {
                block_id: *block_id,
                count: *count,
            });
        }
        if blocks.hosts.len() >= MAX_HOSTS_PER_TURN {
            let first = !blocks.capped;
            blocks.capped = true;
            return Ok(Noted::Capped { first });
        }
        let block_id = mint()?;
        blocks.hosts.insert(key, (block_id, 1));
        Ok(Noted::Send { block_id, count: 1 })
    }

    /// Forgets each thread's sites whose turn `running` no longer gives for it.
    pub(crate) fn forget_ended(&mut self, running: impl Fn(ThreadId) -> Option<TurnId>) {
        self.threads
            .retain(|thread, blocks| running(*thread) == blocks.turn);
    }

    /// How many threads hold sites (tests).
    #[cfg(test)]
    pub(crate) fn threads(&self) -> usize {
        self.threads.len()
    }
}

/// The wire's tag for the list's.
fn site_tag(tag: clauseo_blocklist::Tag) -> SiteTag {
    match tag {
        clauseo_blocklist::Tag::Malware => SiteTag::Malware,
        clauseo_blocklist::Tag::Phishing => SiteTag::Phishing,
    }
}

/// When the list was made, as the wire's milliseconds; the epoch for a date past the wire's
/// range, which no real list has.
fn list_generated(generated: clauseo_blocklist::Generated) -> Timestamp {
    generated
        .unix_seconds()
        .checked_mul(1000)
        .and_then(|millis| Timestamp::new(millis).ok())
        .unwrap_or(Timestamp::ZERO)
}

/// `HostHandle::site_blocked`: notes the refusal for the running turn and, unless the turn is
/// at its cap, publishes the notice to the app subscribers. Only a host's first refusal in a
/// turn is logged (`count` 1), so an agent retrying a listed site in a loop writes one line.
pub(super) fn site_blocked(actor: &mut Actor, session: SessionKey, site: BlockedSite) {
    let thread = session.thread;
    let turn = super::turn::running_turn(actor, thread);
    let mut blocks = std::mem::take(&mut actor.blocked);
    blocks.forget_ended(|thread| super::turn::running_turn(actor, thread));
    let wall = actor.clock.now().wall;
    let noted = blocks.note(thread, turn, &site.host, || actor.ids.mint(wall));
    actor.blocked = blocks;
    let (block_id, count) = match noted {
        Ok(Noted::Send { block_id, count }) => {
            if count == 1 {
                tracing::info!(
                    host = %site.host,
                    tag = site.tag.word(),
                    list = %site.generated.date(),
                    "egress: refused a site on the block list"
                );
            }
            (block_id, count)
        }
        Ok(Noted::Capped { first }) => {
            if first {
                tracing::info!(
                    %thread,
                    max = MAX_HOSTS_PER_TURN,
                    "egress: the turn's blocked-site notices are at their cap; new sites are \
                     not shown"
                );
            }
            return;
        }
        Err(error) => {
            tracing::warn!(error = %crate::log_file::Cause(&error), "no blocked-site notice");
            return;
        }
    };
    // An allow names the notice by its id (`allowed.rs`); a re-sent one is kept once.
    actor
        .allowed
        .noticed(block_id, &site.host, session.matter, thread);
    let event = HostEvent::SiteBlocked {
        block_id,
        matter: session.matter,
        thread: Some(thread),
        turn,
        host: site.host,
        tag: site_tag(site.tag),
        list_generated: list_generated(site.generated),
        override_allowed: actor
            .effective
            .settings()
            .network
            .allow_unsafe_site_override,
        count,
    };
    super::app::publish(actor, &event);
}
