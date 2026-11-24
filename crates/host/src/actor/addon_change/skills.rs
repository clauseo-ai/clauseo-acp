//! The skill half: a pack's checks, its read or removal off the actor, and the settings write.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_profile::packs::{self, ReadError, Refusal, added};
use clauseo_protocol::{AddonKind, ErrorCode, PackId};
use clauseo_settings::{ItemScope, SystemFiles};

use super::{Change, Want, built_in, finish, locked, refused, unknown};
use crate::actor::Actor;
use crate::actor::commands::CommandCtx;
use crate::actor::input::Input;
use crate::actor::settings::packs::{Hashed, READ_JOB, check_firm, read_hash, recheck};
use crate::agent::codex::prepare::off_actor;
use crate::error::Error;

/// The policy key that locks every pack's switch.
const ENABLED: &str = "skills.enabled";
/// The policy key that locks every pack's scope.
const SCOPE: &str = "skills.scope";

/// A pack's read or removal back from its thread.
pub(crate) struct SkillBack {
    ctx: CommandCtx,
    id: PackId,
    change: Change,
    done: Result<Done, Error>,
}

/// What the thread did.
enum Done {
    /// The pack read whole, or why not.
    Read(Result<Hashed, ReadError>),
    /// Whether the folder was there, and the removal's answer.
    Removed(bool, std::io::Result<bool>),
}

/// The checks on the actor; then the change, or the thread when the pack must be read or
/// removed.
pub(super) fn start(
    actor: &mut Actor,
    ctx: &CommandCtx,
    id: &str,
    change: Change,
) -> Result<(), Error> {
    if crate::actor::addon_rules::builtin_skill(id) {
        return Err(built_in(id));
    }
    let pack = PackId::new(id).map_err(|_bad| unknown(AddonKind::Skill, id))?;
    let effective = &actor.effective;
    if effective.is_locked(ENABLED) {
        return Err(locked(ENABLED));
    }
    if let Some(matter) = change.matter() {
        actor.check_matter(matter)?;
    }
    let enabled = effective.settings().skills.enabled.contains_key(id);
    // Turning it on in a matter while it is on nowhere makes its scope `matter`.
    let turns_on = matches!(change, Change::Scope(_))
        || (matches!(change, Change::On { on: true, .. }) && !enabled);
    if turns_on && effective.is_locked(SCOPE) {
        return Err(locked(SCOPE));
    }
    if turns_on && !enabled {
        check_firm(effective, &pack)?;
    }
    let allow_scripts = effective.settings().skills.allow_scripts;
    let root: PathBuf = effective.data_dir().join(packs::PACKS_DIR);
    // A later `SetSkillPack` read for the pack writes nothing.
    let generation = actor.pack_generations.entry(pack.clone()).or_default();
    *generation = generation.wrapping_add(1);
    let (ctx, clock) = (*ctx, Arc::clone(&actor.clock));
    if change == Change::Remove {
        actor.spawn_post(async move {
            let text = pack.as_str().to_owned();
            let done = off_actor(clock, READ_JOB, move || {
                let there = std::fs::symlink_metadata(root.join(&text)).is_ok();
                Ok(Done::Removed(there, added::remove(&root, &text)))
            })
            .await;
            back(ctx, pack, change, done)
        });
        return Ok(());
    }
    if enabled {
        return settle(actor, &ctx, &pack, change, None);
    }
    let hook = actor.wiring.pack_read_hook.clone();
    actor.spawn_post(async move {
        let text = pack.as_str().to_owned();
        let done = off_actor(clock, READ_JOB, move || {
            Ok(Done::Read(read_hash(
                &root,
                &text,
                allow_scripts,
                hook.as_ref(),
            )))
        })
        .await;
        back(ctx, pack, change, done)
    });
    Ok(())
}

fn back(ctx: CommandCtx, id: PackId, change: Change, done: Result<Done, Error>) -> Input {
    Input::Addon(Box::new(SkillBack {
        ctx,
        id,
        change,
        done,
    }))
}

/// The thread's answer, on the actor.
pub(in crate::actor) fn on_back(actor: &mut Actor, back: SkillBack) {
    let SkillBack {
        ctx,
        id,
        change,
        done,
    } = back;
    let result = match done {
        Ok(Done::Read(Err(ReadError::Refused(Refusal::Missing)))) => {
            Err(unknown(AddonKind::Skill, id.as_str()))
        }
        // Switching off a pack that does not read changes only the switch.
        Ok(Done::Read(Err(_))) if matches!(change, Change::On { on: false, .. }) => {
            settle(actor, &ctx, &id, change, None)
        }
        Ok(Done::Read(Err(error))) => Err(Error::refused(
            ErrorCode::SettingsRefused,
            match error {
                ReadError::Changed => format!(
                    "skill pack {id} changed while it was read; nothing was turned on (changed)"
                ),
                ReadError::Refused(refusal) => {
                    format!("skill pack {id} cannot be turned on ({})", refusal.code())
                }
            },
        )),
        Ok(Done::Read(Ok(hashed))) => settle(actor, &ctx, &id, change, Some(&hashed)),
        Ok(Done::Removed(there, removed)) => removed_pack(actor, &ctx, &id, there, removed),
        Err(_unanswered) => Err(Error::Internal(format!(
            "skill pack {id}: the read did not answer"
        ))),
    };
    if let Err(error) = result {
        refused(actor, &ctx, AddonKind::Skill, error);
    }
}

/// The settings half for a pack still there: turned on with `hashed` when the change needs
/// it on and it was not, its scope, then [`finish`].
fn settle(
    actor: &mut Actor,
    ctx: &CommandCtx,
    id: &PackId,
    change: Change,
    hashed: Option<&Hashed>,
) -> Result<(), Error> {
    let enabled = actor
        .effective
        .settings()
        .skills
        .enabled
        .contains_key(id.as_str());
    let (want, scope) = match change {
        Change::Scope(only) => (Want::Only(only), Some(only)),
        Change::On { matter, on: true } if !enabled => {
            (Want::Only(Some(matter)), Some(Some(matter)))
        }
        Change::On { matter, on } => (Want::On { matter, on }, None),
        Change::Remove => (Want::Only(None), None),
    };
    let mut settings = false;
    if let Some(only) = scope {
        if !enabled {
            let hashed =
                hashed.ok_or_else(|| Error::Internal(format!("skill pack {id} was not read")))?;
            recheck(&actor.effective, id, hashed)?;
            actor.effective.set_skill_pack(
                &SystemFiles,
                id.as_str(),
                Some(hashed.hash.as_str()),
            )?;
            settings = true;
        }
        let scope = if only.is_some() {
            ItemScope::Matter
        } else {
            ItemScope::All
        };
        settings |= actor.effective.skill_scope(id.as_str()) != scope;
        actor
            .effective
            .set_skill_scope(&SystemFiles, id.as_str(), scope)?;
    }
    finish(actor, ctx, (AddonKind::Skill, id.as_str()), want, settings);
    Ok(())
}

/// A removal back: the pack's entries go, then every switch.
fn removed_pack(
    actor: &mut Actor,
    ctx: &CommandCtx,
    id: &PackId,
    there: bool,
    removed: std::io::Result<bool>,
) -> Result<(), Error> {
    let enabled = actor
        .effective
        .settings()
        .skills
        .enabled
        .contains_key(id.as_str());
    match removed {
        Ok(true) => {}
        Ok(false) if !there && !enabled => return Err(unknown(AddonKind::Skill, id.as_str())),
        Ok(false) => {
            return Err(Error::refused(
                ErrorCode::SettingsRefused,
                format!("skill pack {id} was not added on this computer (not_added_here)"),
            ));
        }
        Err(error) => {
            return Err(Error::Internal(format!(
                "skill pack {id} was not deleted ({:?})",
                error.kind()
            )));
        }
    }
    tracing::info!(pack = id.as_str(), "a skill was removed");
    let effective = &mut actor.effective;
    if enabled {
        effective.set_skill_pack(&SystemFiles, id.as_str(), None)?;
    }
    if effective.skill_scope(id.as_str()) != ItemScope::All && !effective.is_locked(SCOPE) {
        effective.set_skill_scope(&SystemFiles, id.as_str(), ItemScope::All)?;
    }
    finish(
        actor,
        ctx,
        (AddonKind::Skill, id.as_str()),
        Want::Only(None),
        true,
    );
    Ok(())
}
