//! `AddSkill` and `RemoveSkill` (decision record 0040): a skill from this computer made a skill
//! pack and turned on, and such a pack deleted.
//!
//! - `AddSkill`: the lock on `skills.enabled` is checked on the actor; then off the actor
//!   (`profile::packs::added`) the skill is read, its id made, the firm's admission checked for
//!   that id, the pack built and checked in memory, and written. Back on the actor the checks
//!   run again and its hash is recorded, as `SetSkillPack` records it. `skill.no_skill_md` with
//!   no `SKILL.md`, `skill.invalid` for anything else that stops it (a read, a cap, a pack rule:
//!   the log line holds the rule's code), `settings.locked` when the firm decides.
//! - `RemoveSkill`: off the actor the folder is deleted when it was added here
//!   (`settings.refused` otherwise), then its `skills.enabled` entry goes.
//!
//! Both bump the id's generation (`Actor::pack_generations`), so a `SetSkillPack` read still
//! running for it writes nothing. A running agent keeps its skills; the next launch takes the
//! change.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_profile::packs::added::{self, AddError};
use clauseo_profile::packs::{self, Refusal};
use clauseo_protocol::{Date, ErrorCode, HostEvent, PackId};

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::super::input::Input;
use super::packs::{Admission, READ_JOB, check_firm, write};
use crate::agent::codex::prepare::off_actor;
use crate::error::Error;

/// The folder beside the packs folder a skill is written into before it is renamed in place.
const STAGING_DIR: &str = "skill-packs.adding";

/// An `AddSkill` or `RemoveSkill` back from its thread.
pub(crate) struct SkillDone {
    ctx: CommandCtx,
    what: Done,
}

/// What the thread did.
enum Done {
    /// The pack written, and its hash; or why not.
    Added(Result<Result<(PackId, String), Refused>, Error>),
    /// Whether the pack `id` was added here and is deleted.
    Removed(PackId, Result<std::io::Result<bool>, Error>),
}

/// Why an add stopped off the actor.
enum Refused {
    Add(AddError),
    /// The firm's policy does not admit the id.
    Firm(PackId),
}

/// `AddSkill`.
pub(in crate::actor) fn add_skill(actor: &mut Actor, ctx: &CommandCtx, path: String) {
    let effective = &actor.effective;
    if effective.is_locked("skills.enabled") {
        let error = Error::Settings(clauseo_settings::Error::Locked {
            key: "skills.enabled".to_owned(),
        });
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let data = effective.data_dir();
    let (root, staging) = (data.join(packs::PACKS_DIR), data.join(STAGING_DIR));
    let allow_scripts = effective.settings().skills.allow_scripts;
    let admission = Admission::of(effective);
    let today = added::utc_date(actor.clock.now().wall.as_millis());
    let clock = Arc::clone(&actor.clock);
    let ctx = *ctx;
    actor.spawn_post(async move {
        let done = off_actor(clock, READ_JOB, move || {
            Ok(add(
                Path::new(&path),
                &root,
                &staging,
                today,
                allow_scripts,
                &admission,
            ))
        })
        .await;
        Input::SkillDone(Box::new(SkillDone {
            ctx,
            what: Done::Added(done),
        }))
    });
}

/// Reads, builds, checks and writes the skill at `path`. Off the actor.
fn add(
    path: &Path,
    root: &Path,
    staging: &Path,
    today: Option<Date>,
    allow_scripts: bool,
    admission: &Admission,
) -> Result<(PackId, String), Refused> {
    let today = today.ok_or(Refused::Add(AddError::Invalid(Refusal::Io)))?;
    let source = added::read_source(path).map_err(Refused::Add)?;
    let id = added::free_id(root, &source.slug).map_err(Refused::Add)?;
    let pack_id =
        PackId::new(&id).map_err(|_bad_id| Refused::Add(AddError::Invalid(Refusal::BadId)))?;
    if admission.refusal_for(&id, true).is_some() {
        return Err(Refused::Firm(pack_id));
    }
    let (files, pack) = added::build(&source, &id, today, allow_scripts).map_err(Refused::Add)?;
    added::install(root, staging, &id, &files).map_err(Refused::Add)?;
    Ok((pack_id, pack.hash().as_str().to_owned()))
}

/// `RemoveSkill`.
pub(in crate::actor) fn remove_skill(actor: &mut Actor, ctx: &CommandCtx, id: PackId) {
    let root: PathBuf = actor.effective.data_dir().join(packs::PACKS_DIR);
    let clock = Arc::clone(&actor.clock);
    let ctx = *ctx;
    actor.spawn_post(async move {
        let text = id.as_str().to_owned();
        let done = off_actor(clock, READ_JOB, move || Ok(added::remove(&root, &text))).await;
        Input::SkillDone(Box::new(SkillDone {
            ctx,
            what: Done::Removed(id, done),
        }))
    });
}

/// The thread's answer, on the actor.
pub(in crate::actor) fn on_done(actor: &mut Actor, done: SkillDone) {
    let SkillDone { ctx, what } = done;
    let result = match what {
        Done::Added(Ok(Ok((id, hash)))) => {
            bump(actor, &id);
            tracing::info!(pack = id.as_str(), "a skill was added on this computer");
            check_firm(&actor.effective, &id)
                .and_then(|()| write(actor, &ctx, &id, Some(&hash), true))
        }
        Done::Added(Ok(Err(refused))) => Err(added_error(refused)),
        Done::Added(Err(_unanswered)) => {
            Err(Error::Internal("adding a skill did not answer".to_owned()))
        }
        Done::Removed(id, Ok(Ok(true))) => {
            bump(actor, &id);
            tracing::info!(
                pack = id.as_str(),
                "a skill added on this computer was removed"
            );
            let skills = &actor.effective.settings().skills;
            if skills.enabled.contains_key(id.as_str()) {
                write(actor, &ctx, &id, None, true)
            } else {
                let changed = HostEvent::SettingsChanged(super::view(actor));
                super::super::app::publish(actor, &changed);
                super::receipt(actor, &ctx)
            }
        }
        Done::Removed(id, Ok(Ok(false))) => Err(Error::refused(
            ErrorCode::SettingsRefused,
            format!("skill pack {id} was not added on this computer (not_added_here)"),
        )),
        Done::Removed(id, Ok(Err(error))) => Err(Error::Internal(format!(
            "skill pack {id} was not deleted ({:?})",
            error.kind()
        ))),
        Done::Removed(id, Err(_unanswered)) => Err(Error::Internal(format!(
            "removing skill pack {id} did not answer"
        ))),
    };
    if let Err(error) = result {
        tracing::warn!(
            code = error.code().as_str(),
            "a skill was not added or removed"
        );
        actor.finish_command(ctx.id, Err(error));
    }
}

/// Bumps `id`'s generation, so a `SetSkillPack` read running for it writes nothing.
fn bump(actor: &mut Actor, id: &PackId) {
    let generation = actor.pack_generations.entry(id.clone()).or_default();
    *generation = generation.wrapping_add(1);
}

/// The error an add that stopped off the actor answers. Its message holds a code, never a
/// path or the skill's text.
fn added_error(refused: Refused) -> Error {
    match refused {
        Refused::Add(AddError::NoSkillMd) => Error::refused(
            ErrorCode::SkillNoSkillMd,
            "the path holds no SKILL.md (no_skill_md)",
        ),
        Refused::Add(AddError::Invalid(refusal)) => Error::refused(
            ErrorCode::SkillInvalid,
            format!("the skill cannot be added ({})", refusal.code()),
        ),
        Refused::Firm(id) => Error::refused(
            ErrorCode::SettingsLocked,
            format!("skill pack {id} is not admitted by the firm's policy (not_admitted)"),
        ),
    }
}
