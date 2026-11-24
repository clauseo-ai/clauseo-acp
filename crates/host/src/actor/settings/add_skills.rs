//! Adding skills from where a lawyer finds them (decision record 0040, "Adding from a link"):
//! `SearchSkills` and `PreviewSkill` (queries, `preview.rs`) and `AddSkills` (here).
//!
//! - Each runs off the host thread (`off_actor`), only on the lawyer's own action.
//! - `AddSkills`: the lock on `skills.enabled` and the scope are checked on the actor
//!   ([`scope_ok`]: a matter the host knows, and `skills.scope` not locked); then off the actor
//!   the preview is found again (`skill.preview_expired` when it is gone or past 30 minutes), every
//!   picked name is checked against it (`command.invalid` for one it does not hold), and each
//!   skill is installed as its own pack added here (`profile::packs::source::install_picked`),
//!   scripts allowed unless a firm's policy decides `skills.allow_scripts`; a skill from
//!   skills.sh or GitHub then gets its owner's picture (`skill_source/icon.rs`). Back on the actor
//!   each pack's hash is recorded in `skills.enabled` and the pack put in the scope asked, as
//!   `SetAddonScope` does (`addon_change::place_added`), then one `SettingsChanged` and one
//!   `AddonsChanged`, then `Done` in one commit with the matters' switches. A pack the firm
//!   does not admit is `settings.locked`.

mod preview;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_profile::packs::added::{self, AddError};
use clauseo_profile::packs::source::{
    self, ADDING_DIR, FromKind, Installed, Origin, Remote, SourceError, close_preview, discover,
    open_preview, parse_link,
};
use clauseo_profile::packs::{self, Refusal};
use clauseo_protocol::{AddonScope, Date, ErrorCode, HostEvent, PackId};
use clauseo_settings::SystemFiles;

use super::super::Actor;
use super::super::addon_change::{place_added, receipt_with_rows};
use super::super::commands::CommandCtx;
use super::super::input::Input;
use super::packs::{Admission, added_scripts};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::error::Error;
use crate::skill_source::icon::{self, ICONS_DIR};

use preview::Route;
pub(in crate::actor) use preview::{from_kind, preview, search};

/// The job an `AddSkills` runs as: reading and copying up to 64 skills of at most 5 MB each.
const ADD_JOB: Job = Job {
    what: "adding skills",
    matter: None,
    wait: Duration::from_mins(2),
};

/// The policy key that locks every pack's scope.
const SCOPE: &str = "skills.scope";

/// An `AddSkills` back from its thread.
pub(crate) struct SkillsAdded {
    ctx: CommandCtx,
    scope: AddonScope,
    done: Result<Added, Error>,
}

/// What the thread did: the packs written, in order, and why it stopped, if it did.
struct Added {
    packs: Vec<(PackId, String)>,
    stopped: Option<Error>,
}

/// Whether `scope` can be taken now: `all`, or a matter the host knows
/// (`command.unknown_matter`) while the firm does not lock `skills.scope` (`settings.locked`).
pub(in crate::actor) fn scope_ok(actor: &Actor, scope: AddonScope) -> Result<(), Error> {
    match scope {
        AddonScope::All => Ok(()),
        AddonScope::Matter(matter) => {
            actor.check_matter(matter)?;
            if actor.effective.is_locked(SCOPE) {
                return Err(Error::Settings(clauseo_settings::Error::Locked {
                    key: SCOPE.to_owned(),
                }));
            }
            Ok(())
        }
    }
}

/// `AddSkills`.
pub(in crate::actor) fn add_skills(
    actor: &mut Actor,
    ctx: &CommandCtx,
    (token, pick, scope): (String, Vec<String>, AddonScope),
) {
    let effective = &actor.effective;
    let checked = if effective.is_locked("skills.enabled") {
        Err(Error::Settings(clauseo_settings::Error::Locked {
            key: "skills.enabled".to_owned(),
        }))
    } else {
        scope_ok(actor, scope)
    };
    if let Err(error) = checked {
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let data = effective.data_dir();
    let (root, adding) = (data.join(packs::PACKS_DIR), data.join(ADDING_DIR));
    let allow_scripts = effective.settings().skills.allow_scripts || added_scripts(effective);
    let admission = Admission::of(effective);
    let now = actor.clock.now().wall.as_millis();
    let today = added::utc_date(now);
    let route = Route::of(actor);
    let icons = data.join(ICONS_DIR);
    let clock = Arc::clone(&actor.clock);
    let ctx = *ctx;
    actor.spawn_post(async move {
        let done = off_actor(clock, ADD_JOB, move || {
            let today = today.ok_or_else(|| Error::Internal("no date for today".to_owned()))?;
            let places = (root.as_path(), adding.as_path());
            let (origin, added) = add(
                &token,
                &pick,
                places,
                (today, allow_scripts, now),
                &admission,
            )?;
            keep_icons(&route, &icons, &origin, &added.packs);
            Ok(added)
        })
        .await;
        Input::SkillsAdded(Box::new(SkillsAdded { ctx, scope, done }))
    });
}

/// Finds the preview `token`, checks every name in `pick`, and installs each. Off the actor.
fn add(
    token: &str,
    pick: &[String],
    (root, adding): (&Path, &Path),
    (today, allow_scripts, now): (Date, bool, i64),
    admission: &Admission,
) -> Result<(Origin, Added), Error> {
    let staged = open_preview(adding, token, now).map_err(source_error)?;
    let held = |name: &String| {
        discover(&staged.top, Some(name))
            .is_ok_and(|found| found.iter().any(|skill| skill.name == *name))
    };
    if let Some(missing) = pick.iter().position(|name| !held(name)) {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            format!(
                "the preview holds no skill picked as number {}",
                missing + 1
            ),
        ));
    }
    let admit = |id: &str| admission.refusal_for(id, true).is_none();
    let mut added = Added {
        packs: Vec::new(),
        stopped: None,
    };
    for name in pick {
        match source::install_picked(
            &staged,
            name,
            (root, adding),
            (today, allow_scripts),
            &admit,
        ) {
            Ok(Some(Installed { id, pack })) => {
                added.packs.push((id, pack.hash().as_str().to_owned()));
            }
            Ok(None) => {
                added.stopped = Some(Error::refused(
                    ErrorCode::SettingsLocked,
                    "a picked skill is not admitted by the firm's policy (not_admitted)",
                ));
                break;
            }
            Err(error) => {
                added.stopped = Some(add_error(error));
                break;
            }
        }
    }
    close_preview(adding, token);
    Ok((staged.origin, added))
}

/// Keeps the icon of each of `packs` from `origin`: for skills.sh or GitHub, the owner's GitHub
/// picture, fetched once for them all; else, or when it does not come, none (one kept from an
/// earlier add of the same id goes).
fn keep_icons(route: &Route, icons: &Path, origin: &Origin, packs: &[(PackId, String)]) {
    if packs.is_empty() {
        return;
    }
    let owner = match origin.kind {
        FromKind::SkillsSh | FromKind::GitHub => origin
            .source_url
            .as_deref()
            .and_then(|link| parse_link(link).ok())
            .and_then(|remote| match remote {
                Remote::GitHub(github) => Some(github.owner),
                Remote::Zip { .. } => None,
            }),
        FromKind::Url | FromKind::Zip | FromKind::Folder => None,
    };
    let picture = owner.and_then(|owner| icon::fetch(&route.net().ok()?, &owner));
    for (id, _) in packs {
        match &picture {
            Some(bytes) => icon::store(icons, id.as_str(), bytes),
            None => icon::remove(icons, id.as_str()),
        }
    }
}

/// The thread's answer, on the actor: each pack's hash recorded and its scope set, then the
/// events and `Done`, or why it stopped (the packs written before it stay, recorded, with the
/// scope but no matter's switch: see `place_added`).
pub(in crate::actor) fn on_added(actor: &mut Actor, added: SkillsAdded) {
    let SkillsAdded { ctx, scope, done } = added;
    let result = done.and_then(|added| {
        let recorded = record(actor, &added.packs);
        let ids: Vec<&str> = added.packs.iter().map(|(id, _)| id.as_str()).collect();
        let switches = added.stopped.is_none();
        let placed = recorded.and_then(|()| place_added(actor, &ids, scope, switches));
        if !added.packs.is_empty() {
            let changed = HostEvent::SettingsChanged(super::view(actor));
            super::super::app::publish(actor, &changed);
            super::super::query::addons::changed(actor);
        }
        let rows = placed?;
        match added.stopped {
            Some(error) => Err(error),
            None => receipt_with_rows(actor, &ctx, rows),
        }
    });
    if let Err(error) = result {
        tracing::warn!(code = error.code().as_str(), "skills were not added");
        actor.finish_command(ctx.id, Err(error));
    }
}

/// Records each of `packs` in `skills.enabled`, its generation bumped so a `SetSkillPack` read
/// still running for it writes nothing; the first refusal stops it.
fn record(actor: &mut Actor, packs: &[(PackId, String)]) -> Result<(), Error> {
    for (id, hash) in packs {
        let generation = actor.pack_generations.entry(id.clone()).or_default();
        *generation = generation.wrapping_add(1);
        actor
            .effective
            .set_skill_pack(&SystemFiles, id.as_str(), Some(hash))?;
        tracing::info!(pack = id.as_str(), "a skill was added from a preview");
    }
    Ok(())
}

/// The error of a source that gave no skills. The message holds a code, never the link or a path.
pub(in crate::actor) fn source_error(error: SourceError) -> Error {
    let code = match error {
        SourceError::DownloadFailed => ErrorCode::SkillDownloadFailed,
        SourceError::NotFound => ErrorCode::SkillNotFound,
        SourceError::TooBig => ErrorCode::SkillTooBig,
        SourceError::Expired => ErrorCode::SkillPreviewExpired,
        SourceError::BadSource => ErrorCode::CommandInvalid,
        SourceError::Unsafe | SourceError::Io => ErrorCode::SkillInvalid,
    };
    Error::refused(
        code,
        format!("the skill source gave no skill ({})", error.code()),
    )
}

/// The error of an install that stopped.
fn add_error(error: AddError) -> Error {
    match error {
        AddError::NoSkillMd => Error::refused(
            ErrorCode::SkillNoSkillMd,
            "the picked skill holds no SKILL.md (no_skill_md)",
        ),
        AddError::Invalid(Refusal::TooBig) => Error::refused(
            ErrorCode::SkillTooBig,
            "the picked skill is over its cap (too_big)",
        ),
        AddError::Invalid(refusal) => Error::refused(
            ErrorCode::SkillInvalid,
            format!("the picked skill cannot be added ({})", refusal.code()),
        ),
    }
}

/// The packs folder of `actor`'s data folder.
fn packs_root(actor: &Actor) -> PathBuf {
    actor.effective.data_dir().join(packs::PACKS_DIR)
}
