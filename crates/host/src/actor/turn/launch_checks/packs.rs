//! The launch prep's skill packs part (decision record 0040). See `launch_checks.rs`.
//!
//! - On the actor, [`pack_prep`] builds the launch's [`PackRequest`] from the thread's matter
//!   list and the effective `skills` settings, as pure data: no pack file is read there. The
//!   profile hook adds where the packs live (`ProfileHook::pack_write`).
//! - On the launch-prep thread, after the built-in skills, [`PackPrep::run`] loads them
//!   (`PackWrite::load`: read, check, hash against the recorded hash, render in memory) and
//!   writes them into the launch's own plugin, [`RunFolders::skill_packs`], which goes with the
//!   run folder. A failure is a warning, and the launch goes on with no packs.
//! - What was skipped goes in one log line per launch, with the ids and reason codes alone (no
//!   path, no pack text: decision record 0047), and in one thread notice per skipped pack
//!   ([`pack_notices`]), shown once per thread, pack and reason for the host's run: a relaunch
//!   or a new agent session does not show it again, so an agent that restarts in a loop cannot
//!   fill the thread, and each thread keeps its own record.
//!
//! [`RunFolders::skill_packs`]: super::RunFolders::skill_packs

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use clauseo_profile::packs::{PACKS_PER_LAUNCH, PackEntry, PackHash, Refusal, Skip, is_pack_id};
use clauseo_profile::{PackWrite, PacksWritten};
use clauseo_protocol::{
    AgentId, AgentKind, Notice, NoticeKind, NoticeLevel, PackId, PackList, PackSkipReason, ThreadId,
};
use clauseo_settings::{SkillsExtra, SkillsSettings};

use super::super::super::Actor;
use super::super::state;
use crate::log_file::Cause;

#[cfg(test)]
mod tests;

/// Why a launch left a skill pack out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackSkip {
    /// The matter's list names a pack `skills.enabled` does not hold.
    NotTurnedOn,
    /// `skills.extra` is `off`, or `curated` and the pack is not in `skills.curated`.
    NotAdmitted,
    /// The launch prep's load left it out: refused, changed since it was turned on, or past
    /// the per-launch cap.
    Load(Skip),
}

impl PackSkip {
    /// The reason as a short code, for the log line.
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::NotTurnedOn => "not_turned_on",
            Self::NotAdmitted => "not_admitted",
            Self::Load(skip) => skip.code(),
        }
    }

    /// The reason as the app's notice names it: one of six categories, so two refusal kinds
    /// that read the same are one reason for the once-per rule.
    pub(crate) fn reason(self) -> PackSkipReason {
        match self {
            Self::NotTurnedOn => PackSkipReason::NotTurnedOn,
            Self::NotAdmitted => PackSkipReason::NotAdmitted,
            Self::Load(Skip::Changed) => PackSkipReason::Changed,
            Self::Load(Skip::TooMany) => PackSkipReason::TooMany,
            Self::Load(Skip::Refused(Refusal::Missing)) => PackSkipReason::Missing,
            Self::Load(_) => PackSkipReason::Invalid,
        }
    }

    /// The reason in the notice's words: a sentence or two, after the lead.
    fn words(self) -> String {
        match self {
            Self::NotTurnedOn => "This matter is set to use it, but it isn't turned on.".to_owned(),
            Self::NotAdmitted => {
                "The skill pack settings on this computer don't allow it.".to_owned()
            }
            Self::Load(Skip::Changed) => {
                "Its files changed after it was turned on. Turn it on again to use it.".to_owned()
            }
            Self::Load(Skip::TooMany) => {
                format!("Clauseo loads at most {PACKS_PER_LAUNCH} skill packs at a time.")
            }
            Self::Load(Skip::Refused(Refusal::Missing)) => {
                "Clauseo couldn't find it on this computer.".to_owned()
            }
            Self::Load(_) => "Its files aren't a valid skill pack.".to_owned(),
        }
    }

    /// The notice for the pack `id` this reason left out: the lead sentence naming the id in
    /// straight quotes, then the reason. `id` is `PackId`-shaped (callers check it), so it holds
    /// no quote character.
    pub(crate) fn notice(self, id: &str) -> String {
        format!(
            "Clauseo didn't load the skill pack \"{id}\". {}",
            self.words()
        )
    }

    /// The notice for the pack `pack` this reason left out: a warning of kind
    /// `SkillPackNotLoaded` naming the pack and the reason. Its `text` is [`Self::notice`]'s
    /// words, the fallback for a view that does not know the kind.
    pub(crate) fn host_notice(self, pack: PackId) -> Notice {
        Notice {
            level: NoticeLevel::Warning,
            text: self.notice(pack.as_str()),
            kind: Some(NoticeKind::SkillPackNotLoaded {
                pack,
                reason: self.reason(),
            }),
        }
    }
}

/// What a launch asks of its skill packs, built on the actor: the thread's matter list (none
/// means every pack in `skills.enabled`; an empty list means none; a list only narrows
/// `skills.enabled`), each pack with the hash recorded when it was turned on, after
/// `skills.extra` and `skills.curated` admitted it, and `skills.allow_scripts`. Pure data:
/// building it reads no file. The profile hook turns it into the packs the launch-prep thread
/// loads (`ProfileHook::pack_write`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRequest {
    entries: Vec<PackEntry>,
    allow_scripts: bool,
    skipped: Vec<(String, PackSkip)>,
}

impl PackRequest {
    /// The request for a thread whose matter's list is `matter`, under `skills`. Packs come in
    /// `skills.enabled` order. A pack policy does not admit (`extra` `off`, or `curated`
    /// without it) is skipped as not admitted, and an id the matter names that `skills.enabled`
    /// does not hold as not turned on.
    pub fn new(skills: &SkillsSettings, matter: Option<&PackList>) -> Self {
        let named = |id: &str| {
            matter.is_none_or(|list| list.as_slice().iter().any(|pack| pack.as_str() == id))
        };
        let mut entries = Vec::new();
        let mut skipped = Vec::new();
        for (id, hash) in skills.enabled.iter().filter(|(id, _)| named(id)) {
            let admitted = match skills.extra {
                SkillsExtra::Off => false,
                SkillsExtra::Curated => skills.curated.contains(id),
                SkillsExtra::Any => true,
            };
            match PackHash::parse(hash) {
                _ if !admitted => skipped.push((id.clone(), PackSkip::NotAdmitted)),
                Some(hash) => entries.push(PackEntry {
                    id: id.clone(),
                    hash,
                }),
                // The settings keep only `sha256:<64 hex>`, so this is never reached.
                None => skipped.push((id.clone(), PackSkip::NotTurnedOn)),
            }
        }
        for id in matter.map_or(&[][..], PackList::as_slice) {
            if !skills.enabled.contains_key(id.as_str()) {
                skipped.push((id.as_str().to_owned(), PackSkip::NotTurnedOn));
            }
        }
        Self {
            entries,
            allow_scripts: skills.allow_scripts,
            skipped,
        }
    }

    /// The packs to load, each with its recorded hash, in `skills.enabled` order.
    pub fn entries(&self) -> &[PackEntry] {
        &self.entries
    }

    /// Whether a pack may carry scripts (`skills.allow_scripts`).
    pub fn allow_scripts(&self) -> bool {
        self.allow_scripts
    }
}

/// A launch's skill packs, from the actor to the launch-prep thread.
#[derive(Debug, Default)]
pub(super) struct PackPrep {
    write: Option<PackWrite>,
    skipped: Vec<(String, PackSkip)>,
}

/// What the launch prep does for `agent`'s skill packs, from the actor's state: the thread's
/// matter list `matter` and the effective `skills` settings ([`PackRequest`]). Nothing for an
/// agent with no kind: packs are Clauseo's, for Claude and Codex. No file is read.
pub(super) fn pack_prep(actor: &Actor, agent: &AgentId, matter: Option<&PackList>) -> PackPrep {
    if AgentKind::of(agent).is_none() {
        return PackPrep::default();
    }
    let request = PackRequest::new(&actor.effective.settings().skills, matter);
    PackPrep {
        write: actor.profile.pack_write(agent, &request),
        skipped: request.skipped,
    }
}

impl PackPrep {
    /// Loads the packs and writes those that loaded into `plugin`, the launch's own pack plugin.
    /// The token when a pack was written, and every pack left out, by id, with the reason,
    /// which one log line holds. A failed write is a warning, and the launch goes on with no
    /// packs. Reads and writes files: on the launch-prep thread.
    pub(super) fn run(self, plugin: &Path) -> (Option<Arc<PacksWritten>>, Vec<(String, PackSkip)>) {
        let mut skipped = self.skipped;
        let written = self.write.and_then(|write| {
            let loaded = write.load();
            skipped.extend(
                loaded
                    .skipped
                    .into_iter()
                    .map(|(id, skip)| (id, PackSkip::Load(skip))),
            );
            if loaded.packs.is_empty() {
                return None;
            }
            match clauseo_profile::write_packs(plugin, &loaded.packs) {
                Ok(written) => Some(Arc::new(written)),
                Err(error) => {
                    tracing::warn!(
                        error = %Cause(&error),
                        "the skill packs were not written; the agent starts without them"
                    );
                    None
                }
            }
        });
        if !skipped.is_empty() {
            tracing::warn!(
                skipped = %skipped_text(&skipped),
                "skill packs were left out of a launch"
            );
        }
        (written, skipped)
    }
}

/// `skipped` as the log line holds it: `<id>:<code>` for each, comma-separated. An id that is
/// not pack-shaped is shown as `<bad id>`, so the line never echoes text that is no id.
pub(crate) fn skipped_text(skipped: &[(String, PackSkip)]) -> String {
    skipped
        .iter()
        .map(|(id, skip)| {
            let id = if is_pack_id(id) {
                id.as_str()
            } else {
                "<bad id>"
            };
            format!("{id}:{}", skip.code())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A thread's notices of skipped skill packs: what its last launch skipped, until its session
/// attaches, and what the thread was already told, by pack and reason, for the host's run. It
/// lives with the thread's turn state, so a relaunch keeps it.
#[derive(Debug, Default)]
pub(crate) struct PackNotices {
    pending: Vec<(String, PackSkip)>,
    shown: HashSet<(PackId, PackSkipReason)>,
}

/// Holds `skipped` (the launch `thread` just started) for its session's notice.
pub(super) fn hold_notice(actor: &mut Actor, thread: ThreadId, skipped: Vec<(String, PackSkip)>) {
    if let Some(state) = state(actor, thread) {
        state.packs.pending = skipped;
    }
}

/// The notices for `thread`'s session that just attached: one per pack its launch skipped
/// that the thread was not told of yet (each pack and reason once per thread for the host's
/// run), maybe none. Packs are named by id only, never by the pack's own text.
pub(crate) fn pack_notices(actor: &mut Actor, thread: ThreadId) -> Vec<Notice> {
    state(actor, thread).map_or_else(Vec::new, |state| state.packs.untold())
}

impl PackNotices {
    /// The notices for what is pending that the thread was not told of yet, keyed by pack and
    /// the reason, so two refusal kinds that read the same are told once.
    fn untold(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.pending)
            .into_iter()
            .filter(|(id, _)| is_pack_id(id))
            // `is_pack_id` implies `PackId::new`, so this drops nothing.
            .filter_map(|(id, skip)| Some((PackId::new(&id).ok()?, skip)))
            .filter(|(pack, skip)| self.shown.insert((pack.clone(), skip.reason())))
            .map(|(pack, skip)| skip.host_notice(pack))
            .collect()
    }
}
