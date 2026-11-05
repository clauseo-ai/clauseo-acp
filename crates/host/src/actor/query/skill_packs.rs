//! `ListSkillPacks` (decision record 0040, "Turning packs on (PR-E)"): one `PackStatus` per
//! folder in `<data>/skill-packs/` whose name is a `PackId`, and one per pack `skills.enabled`
//! names with no folder, sorted by id. The actor takes what it needs from the settings (the
//! admission, `skills.allow_scripts`, `skills.enabled`); the folder is listed and each pack read
//! twice off the actor (`profile::packs::inspect_twice`), and the answer goes to the caller from
//! a task of its own. Nothing is kept.
//!
//! A pack's state, first match wins:
//! 1. `not_admitted` when `skills.extra` (or the firm's `skills.curated`) does not admit it,
//!    whatever else holds; `reason` says whether the firm decided;
//! 2. `invalid` when it breaks a pack rule (`reason` the rule), `missing` included for a pack
//!    turned on whose folder is gone;
//! 3. `changed` when it changed while it was read, or turned on with a hash it no longer has;
//! 4. `on` when turned on with the hash it has, else `off`.
//!
//! The manifest's strings pass `profile::packs::listed` (no control or format character) with
//! the manifest's own caps; one that fails is left out. A folder whose name is not a `PackId`
//! is not listed: it has no id to show, and is logged by count alone.
//!
//! The folder is listed once and each pack found in that listing, so the list is linear in the
//! folders. It reads at most [`MAX_LISTED`] packs, the first by id; the rest are left out of
//! the answer and logged by count alone, never by name or path.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_profile::packs::manifest::{DISPLAY_MAX, URL_MAX};
use clauseo_profile::packs::{self, Pack, PackFolders, ReadError};
use clauseo_protocol::{PackId, PackReason, PackRefusal, PackState, PackStatus, QueryResult};
use futures::task::LocalSpawnExt;

use crate::actor::Actor;
use crate::actor::input::Reply;
use crate::actor::settings::packs::{Admission, PackReadHook, READ_JOB, read_listed_pack};
use crate::agent::codex::prepare::off_actor;
use crate::error::Error;

/// The most packs one list reads and answers with: 256, each read twice, keeps a list well
/// inside the read wait however many folders the packs folder holds.
pub(crate) const MAX_LISTED: usize = 256;

/// What the list needs from the actor, taken so it can leave it.
struct Listing {
    root: PathBuf,
    admission: Admission,
    allow_scripts: bool,
    enabled: BTreeMap<String, String>,
    hook: Option<PackReadHook>,
}

/// Answers `ListSkillPacks` from a task of its own, the reads off the actor.
pub(super) fn list(actor: &Actor, reply: Reply<QueryResult>) {
    let skills = &actor.effective.settings().skills;
    let listing = Listing {
        root: actor.effective.data_dir().join(packs::PACKS_DIR),
        admission: Admission::of(&actor.effective),
        allow_scripts: skills.allow_scripts,
        enabled: skills.enabled.clone(),
        hook: actor.wiring.pack_read_hook.clone(),
    };
    let clock = Arc::clone(&actor.clock);
    let spawned = actor.spawner.spawn_local(async move {
        let answer = off_actor(clock, READ_JOB, move || Ok(statuses(&listing))).await;
        let answer = answer
            .map(QueryResult::SkillPacks)
            .map_err(|_unanswered| Error::Internal("the skill pack list did not answer".into()));
        let _gone = reply.send(answer);
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "the host pool refused a skill pack list");
    }
}

/// Every pack's status, sorted by id, at most [`MAX_LISTED`] of them. Off the actor.
fn statuses(listing: &Listing) -> Vec<PackStatus> {
    let folders = PackFolders::list(&listing.root);
    let mut ids = folder_ids(&folders);
    ids.extend(listing.enabled.keys().filter_map(|id| PackId::new(id).ok()));
    let over = ids.len().saturating_sub(MAX_LISTED);
    if over > 0 {
        tracing::warn!(
            over,
            "skill packs past the list's cap of {MAX_LISTED} are not listed"
        );
    }
    ids.into_iter()
        .take(MAX_LISTED)
        .map(|id| status(listing, &folders, id))
        .collect()
}

/// The names in `folders` that are folders, or links (refused as such when read), and
/// `PackId`s. A missing packs folder holds none.
fn folder_ids(folders: &PackFolders) -> BTreeSet<PackId> {
    let mut ids = BTreeSet::new();
    let mut unnamed = 0_usize;
    for name in folders.folder_names() {
        match name.to_str().map(PackId::new) {
            Some(Ok(id)) => {
                ids.insert(id);
            }
            _ => unnamed += 1,
        }
    }
    if unnamed > 0 {
        tracing::warn!(
            unnamed,
            "skill pack folders whose names are not pack ids are not listed"
        );
    }
    ids
}

/// The pack `id`'s status. Off the actor.
fn status(listing: &Listing, folders: &PackFolders, id: PackId) -> PackStatus {
    let read = read_listed_pack(
        folders,
        id.as_str(),
        listing.allow_scripts,
        listing.hook.as_ref(),
    );
    let recorded = listing.enabled.get(id.as_str());
    let (state, reason) = match (listing.admission.refusal(id.as_str()), &read) {
        (Some(by_policy), _) => (
            PackState::NotAdmitted,
            Some(PackReason::NotAdmitted { by_policy }),
        ),
        (None, Err(ReadError::Refused(refusal))) => (
            PackState::Invalid,
            Some(PackReason::Invalid {
                kind: PackRefusal::from(*refusal),
            }),
        ),
        (None, Err(ReadError::Changed)) => (PackState::Changed, None),
        (None, Ok(pack)) => match recorded {
            Some(hash) if hash == pack.hash().as_str() => (PackState::On, None),
            Some(_) => (PackState::Changed, None),
            None => (PackState::Off, None),
        },
    };
    let pack = read.ok();
    shown(id, pack.as_ref(), state, reason)
}

/// The status of `id` in `state`, with `pack`'s manifest when it read.
fn shown(
    id: PackId,
    pack: Option<&Pack>,
    state: PackState,
    reason: Option<PackReason>,
) -> PackStatus {
    let manifest = pack.map(Pack::manifest);
    let text = |pick: fn(&packs::PackManifest) -> &str, max| {
        manifest.and_then(|manifest| packs::listed(pick(manifest), max))
    };
    PackStatus {
        name: text(|m| &m.name, DISPLAY_MAX),
        version: text(|m| &m.version, DISPLAY_MAX),
        author: text(|m| &m.author, DISPLAY_MAX),
        licence: text(|m| &m.licence, DISPLAY_MAX),
        source_url: text(|m| &m.source_url, URL_MAX),
        last_reviewed: manifest.map(|manifest| manifest.last_reviewed),
        instruction_only: manifest.map(|manifest| manifest.instruction_only),
        hash: pack.map(|pack| pack.hash().as_str().to_owned()),
        id,
        state,
        reason,
    }
}
