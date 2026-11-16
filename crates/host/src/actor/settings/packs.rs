//! `SetSkillPack` (decision record 0040, "Turning packs on (PR-E)"): turns one skill pack on or
//! off in `skills.enabled`, and [`Admission`], which both it and `ListSkillPacks` ask.
//!
//! On: the lock and the admission are checked on the actor, from the effective settings, then
//! the pack is read twice and hashed off the actor (`profile::packs::inspect_twice`, on a
//! `clauseo-codex-fs` thread), and its answer comes back as [`Input::SkillPack`]. Back on the
//! actor, the lock, the admission and `skills.allow_scripts` are checked again (a policy reload
//! may have come in between), then `Effective::set_skill_pack` records the hash. Off: the entry
//! is removed at once. A change sends `SettingsChanged` to every app subscriber, as the other
//! settings commands do.
//!
//! Last command wins: every `SetSkillPack` bumps its id's generation
//! (`Actor::pack_generations`), and a read whose generation is no longer the id's writes
//! nothing. Its `on` still answers `Done`: it was overtaken by a later command for the same
//! pack, not refused.
//!
//! The codes (no new one): `settings.locked` when the firm decided (policy locks
//! `skills.enabled`, or the pack is not admitted by a choice policy made: see [`Admission`]);
//! `settings.refused` when the lawyer's own `skills.extra` does not admit it, or the pack is
//! missing, invalid, or changed while it was read. A message names the pack by its id alone
//! (a `PackId`), with a reason code; a log line holds the id and the code, never a path or the
//! pack's text (decision record 0047).

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_profile::packs::{self, ReadError};
use clauseo_protocol::{ErrorCode, HostEvent, PackId};
use clauseo_settings::{Effective, SkillsExtra, SystemFiles};

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::super::input::Input;
use crate::agent::codex::prepare::{Job, LAUNCH_WAIT, off_actor};
use crate::error::Error;

/// The key `SetSkillPack` writes.
const ENABLED: &str = "skills.enabled";

pub(crate) use crate::config::PackReadHook;

/// The job a pack read runs as: up to two reads of 8 MiB, given a launch's wait, since an
/// antivirus scan can slow each file.
pub(in crate::actor) const READ_JOB: Job = Job {
    what: "skill pack read",
    matter: None,
    wait: LAUNCH_WAIT,
};

/// What admits a pack, taken from the effective settings so it can leave the actor.
#[derive(Debug, Clone)]
pub(in crate::actor) struct Admission {
    extra: SkillsExtra,
    curated: Vec<String>,
    /// Whether policy locks `skills.extra`.
    extra_locked: bool,
    /// Whether policy locks `skills.curated`.
    curated_locked: bool,
}

impl Admission {
    /// The admission `effective` gives.
    pub(in crate::actor) fn of(effective: &Effective) -> Self {
        let skills = &effective.settings().skills;
        Self {
            extra: skills.extra,
            curated: skills.curated.clone(),
            extra_locked: effective.is_locked("skills.extra"),
            curated_locked: effective.is_locked("skills.curated"),
        }
    }

    /// `None` when `id` is admitted, else whether the firm decided it is not. `off` is the
    /// firm's when policy locks `skills.extra`; `curated` leaving `id` out is the firm's when
    /// policy locks `skills.extra` or sets `skills.curated` (which only a policy sets).
    pub(in crate::actor) fn refusal(&self, id: &str) -> Option<bool> {
        match self.extra {
            SkillsExtra::Any => None,
            SkillsExtra::Curated if self.curated.iter().any(|listed| listed == id) => None,
            SkillsExtra::Curated => Some(self.extra_locked || self.curated_locked),
            SkillsExtra::Off => Some(self.extra_locked),
        }
    }
}

/// The answer of a pack's read off the actor, for [`on_read`].
pub(crate) struct PackRead {
    ctx: CommandCtx,
    id: PackId,
    /// The id's generation when the read started.
    generation: u64,
    /// The outer `Err` is the job's own (it did not answer); the inner one the read's.
    read: Result<Result<Hashed, ReadError>, Error>,
}

/// A pack read whole: its hash, and whether it holds a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::actor) struct Hashed {
    pub(in crate::actor) hash: String,
    pub(in crate::actor) scripts: bool,
}

/// `SetSkillPack`: on, the checks and the read off the actor; off, the write.
pub(in crate::actor) fn set_skill_pack(actor: &mut Actor, ctx: &CommandCtx, id: &PackId, on: bool) {
    let generation = actor.pack_generations.entry(id.clone()).or_default();
    *generation = generation.wrapping_add(1);
    let generation = *generation;
    let started = if on {
        start_on(actor, ctx, id, generation)
    } else {
        write(actor, ctx, id, None)
    };
    if let Err(error) = started {
        refused(actor, ctx, id, error);
    }
}

/// The checks on the actor, then the read on a thread of its own.
fn start_on(actor: &Actor, ctx: &CommandCtx, id: &PackId, generation: u64) -> Result<(), Error> {
    check_on(&actor.effective, id)?;
    let root: PathBuf = actor.effective.data_dir().join(packs::PACKS_DIR);
    let allow_scripts = actor.effective.settings().skills.allow_scripts;
    let hook = actor.wiring.pack_read_hook.clone();
    let clock = Arc::clone(&actor.clock);
    let (ctx, id) = (*ctx, id.clone());
    actor.spawn_post(async move {
        let text = id.as_str().to_owned();
        let read = off_actor(clock, READ_JOB, move || {
            Ok(read_hash(&root, &text, allow_scripts, hook.as_ref()))
        })
        .await;
        Input::SkillPack(Box::new(PackRead {
            ctx,
            id,
            generation,
            read,
        }))
    });
    Ok(())
}

/// Reads the pack `id` twice under `root` and gives its hash and whether it holds a script;
/// `hook` runs between the reads. Off the actor.
fn read_hash(
    root: &std::path::Path,
    id: &str,
    allow_scripts: bool,
    hook: Option<&PackReadHook>,
) -> Result<Hashed, ReadError> {
    read_pack(root, id, allow_scripts, hook).map(|pack| Hashed {
        hash: pack.hash().as_str().to_owned(),
        scripts: pack.has_scripts(),
    })
}

/// [`packs::inspect_twice_in`], the folder found in `folders`, with `hook` between the reads.
/// Off the actor.
pub(in crate::actor) fn read_listed_pack(
    folders: &packs::PackFolders,
    id: &str,
    allow_scripts: bool,
    hook: Option<&PackReadHook>,
) -> Result<packs::Pack, ReadError> {
    let between = || {
        if let Some(hook) = hook {
            hook(id);
        }
    };
    packs::inspect_twice_in(folders, id, allow_scripts, &between)
}

/// [`packs::inspect_twice`], with `hook` between the reads. Off the actor.
fn read_pack(
    root: &std::path::Path,
    id: &str,
    allow_scripts: bool,
    hook: Option<&PackReadHook>,
) -> Result<packs::Pack, ReadError> {
    match hook {
        Some(hook) => packs::inspect_twice_with(root, id, allow_scripts, &|| hook(id)),
        None => packs::inspect_twice(root, id, allow_scripts),
    }
}

/// The read came back: when a later `SetSkillPack` for the id overtook it, `Done` with nothing
/// written; else the checks again, then the write.
pub(in crate::actor) fn on_read(actor: &mut Actor, answer: PackRead) {
    let PackRead {
        ctx,
        id,
        generation,
        read,
    } = answer;
    if actor.pack_generations.get(&id) != Some(&generation) {
        tracing::info!(
            pack = id.as_str(),
            "a skill pack's read was overtaken by a later command"
        );
        if let Err(error) = super::receipt(actor, &ctx) {
            actor.finish_command(ctx.id, Err(error));
        }
        return;
    }
    let result = match read {
        Ok(Ok(hashed)) => recheck(&actor.effective, &id, &hashed)
            .and_then(|()| write(actor, &ctx, &id, Some(&hashed.hash))),
        Ok(Err(error)) => Err(Error::refused(
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
        Err(_unanswered) => Err(Error::Internal(format!(
            "skill pack {id}: the read did not answer"
        ))),
    };
    if let Err(error) = result {
        refused(actor, &ctx, &id, error);
    }
}

/// The checks after the read, against the settings in force now: [`check_on`], and a pack with
/// a script while `skills.allow_scripts` is off is refused as the read would have refused it
/// (`settings.refused`, `script`).
pub(in crate::actor) fn recheck(
    effective: &Effective,
    id: &PackId,
    hashed: &Hashed,
) -> Result<(), Error> {
    check_on(effective, id)?;
    if hashed.scripts && !effective.settings().skills.allow_scripts {
        let code = packs::Refusal::Script.code();
        return Err(Error::refused(
            ErrorCode::SettingsRefused,
            format!("skill pack {id} cannot be turned on ({code})"),
        ));
    }
    Ok(())
}

/// Whether `id` may be turned on: `settings.locked` while policy locks `skills.enabled` or a
/// firm decided it is not admitted, `settings.refused` when the lawyer's own `skills.extra`
/// does not admit it.
fn check_on(effective: &Effective, id: &PackId) -> Result<(), Error> {
    if effective.is_locked(ENABLED) {
        return Err(Error::Settings(clauseo_settings::Error::Locked {
            key: ENABLED.to_owned(),
        }));
    }
    match Admission::of(effective).refusal(id.as_str()) {
        None => Ok(()),
        Some(true) => Err(Error::refused(
            ErrorCode::SettingsLocked,
            format!("skill pack {id} is not admitted by the firm's policy (not_admitted)"),
        )),
        Some(false) => Err(Error::refused(
            ErrorCode::SettingsRefused,
            format!("skill pack {id} is not admitted by skills.extra (not_admitted)"),
        )),
    }
}

/// Records `hash` for `id`, or removes its entry, then tells every window when it changed, then
/// the receipt.
fn write(
    actor: &mut Actor,
    ctx: &CommandCtx,
    id: &PackId,
    hash: Option<&str>,
) -> Result<(), Error> {
    let before = actor
        .effective
        .settings()
        .skills
        .enabled
        .get(id.as_str())
        .cloned();
    actor
        .effective
        .set_skill_pack(&SystemFiles, id.as_str(), hash)?;
    if before.as_deref() != hash {
        tracing::info!(
            pack = id.as_str(),
            on = hash.is_some(),
            "a skill pack was turned on or off"
        );
        let changed = HostEvent::SettingsChanged(super::view(actor));
        super::super::app::publish(actor, &changed);
    }
    super::receipt(actor, ctx)
}

/// Answers the command with `error`, and logs the id and the code.
fn refused(actor: &mut Actor, ctx: &CommandCtx, id: &PackId, error: Error) {
    tracing::warn!(
        pack = id.as_str(),
        code = error.code().as_str(),
        "a skill pack was not turned on or off"
    );
    actor.finish_command(ctx.id, Err(error));
}

#[cfg(test)]
mod tests;
