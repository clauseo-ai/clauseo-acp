//! Claude's flag settings as a file: each Claude session call's `claudeCode.options.settings`
//! (the host's hooks, `permissions`, `sandbox` and the rest, `agent::loads::profile`) is written
//! to `<data>/launch-settings/<run id>.json` just before the call, and the call carries the
//! file's absolute path in its place.
//!
//! Why not inline: claude-agent-acp 0.89.1 hands the object to claude-agent-sdk 0.3.293
//! (`dist/acp-agent.js:7280-7327`), which JSON-encodes it and starts Claude Code with it as one
//! argument, `--settings <json>` (`sdk.mjs`: `settings:typeof i==="object"?ie(i):i`, then
//! `qx(K,"settings",…)`). Windows caps a whole command line at 32,767 characters and Linux one
//! argument at 128 KiB, and the data folder's deny rules alone grow past the first with a few
//! dozen matters (`agent/data_fence/launch.rs` names every other matter's folders). A string is
//! passed on unchanged: acp reads the file only to merge its effort settings, as it does the
//! object (`:7287-7289`, `:7620`), and the SDK hands it to Claude Code as `--settings <path>`.
//!
//! Claude Code 2.1.293 reads the file itself at start-up (`JAs`: a value that is not `{…}` is a
//! path, read whole, at most 2 MiB, else the launch fails) and pins what it read
//! (`setFlagSettingsFilePinnedContent`): every later read of the flag settings (`iM`) takes the
//! pinned text and never the file again, so a change to the file after the start changes
//! nothing, and removing it is safe. That read is Claude Code's own, not a tool's, so the deny
//! rules that name `launch-settings` (`data_fence::DENIED`) keep the agent's tools out of it
//! without stopping Claude Code loading it.
//!
//! A session call of the same run that follows one that failed (`session/load` failed, then
//! `session/new`) gets a fresh name, `<run id>.<n>.json`: a file already at a name is never
//! replaced. Each run's files go with its run folder (`launch_checks::remove_folders`), and the
//! host's first launch removes every file an earlier host left ([`sweep`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value};

use crate::agent::codex::prepare::{Job, off_actor_or_undo};
use crate::clock::Clock;
use crate::error::Error;
use clauseo_protocol::{ErrorCode, MatterId};

/// The folder under `<data>` that holds the settings files.
pub(crate) const FOLDER: &str = "launch-settings";

/// The largest settings file Claude Code 2.1.293 reads (`Zf`, 2 MiB); a larger one makes it exit
/// at its start, so a launch whose settings would be larger fails before it (the shell rules
/// shed their variable forms first, `agent::loads::profile::SHED_AT`).
const MAX_FILE: usize = 2 * 1024 * 1024;

/// The most names a run tries before its session call fails.
const MAX_NAMES: u32 = 100;

/// `<data>/launch-settings`.
pub(crate) fn folder(data: &Path) -> PathBuf {
    data.join(FOLDER)
}

/// Where one run's settings files go: the folder ([`folder`]) and the run's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// `<data>/launch-settings`.
    pub(crate) folder: PathBuf,
    /// The run's id (`SessionKey::id`), the files' names' start.
    pub(crate) run: String,
}

impl Target {
    /// The `n`th name (from 1) this run tries: `<run>.json`, then `<run>.<n>.json`.
    fn name(&self, n: u32) -> String {
        if n == 1 {
            format!("{}.json", self.run)
        } else {
            format!("{}.{n}.json", self.run)
        }
    }

    /// Whether `name` is one of this run's names.
    fn owns(&self, name: &str) -> bool {
        let Some(rest) = name
            .strip_prefix(self.run.as_str())
            .and_then(|rest| rest.strip_suffix(".json"))
        else {
            return false;
        };
        rest.is_empty()
            || rest
                .strip_prefix('.')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    }

    /// Moves `meta`'s `claudeCode.options.settings` object into a new file of this run and puts
    /// the file's absolute path there instead; `meta` unchanged when it has no such object.
    /// The folder is made when missing (0700 on Unix, through no link below `<data>`'s parent);
    /// the file is written as `write::write_new_private_file` writes (0600 on Unix, atomic,
    /// never over another). The file's text is the object's JSON, as the SDK would have encoded
    /// it inline.
    pub(crate) fn write(&self, meta: &mut Map<String, Value>) -> std::io::Result<Option<PathBuf>> {
        let Some(options) = options(meta) else {
            return Ok(None);
        };
        let Some(Value::Object(settings)) = options.get("settings") else {
            return Ok(None);
        };
        let bytes = serde_json::to_vec(settings)?;
        if bytes.len() > MAX_FILE {
            // Claude Code would exit at its start; the launch fails here instead, never without
            // the rules.
            tracing::warn!(
                settings_bytes = bytes.len(),
                budget = MAX_FILE,
                "Claude's settings are larger than Claude Code reads; the launch fails"
            );
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Claude's settings are larger than Claude Code reads",
            ));
        }
        let folder = std::path::absolute(&self.folder)?;
        if let Some(base) = folder.parent() {
            crate::write::create_private_dirs_under(base, &folder)?;
        }
        for n in 1..=MAX_NAMES {
            let path = folder.join(self.name(n));
            match crate::write::write_new_private_file(&path, &bytes) {
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
                Ok(()) => {
                    let text = path.to_string_lossy().into_owned();
                    options.insert("settings".to_owned(), Value::String(text));
                    return Ok(Some(path));
                }
            }
        }
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    }
}

/// `meta`'s `claudeCode.options`, when it is an object.
fn options(meta: &mut Map<String, Value>) -> Option<&mut Map<String, Value>> {
    meta.get_mut("claudeCode")?
        .as_object_mut()?
        .get_mut("options")?
        .as_object_mut()
}

/// Whether `meta` has settings for [`Target::write`] to move.
fn has_settings(meta: &mut Map<String, Value>) -> bool {
    options(meta).is_some_and(|options| matches!(options.get("settings"), Some(Value::Object(_))))
}

/// Waits for `check`, then, when `target` is set and `meta` has Claude's settings, moves them
/// into this run's file ([`Target::write`]) off the actor, for `matter`'s session call. A file
/// written after the caller stopped waiting is removed there. A failed write fails the call.
pub(crate) async fn ready(
    check: impl Future<Output = Result<(), Error>>,
    clock: Arc<dyn Clock>,
    matter: Option<MatterId>,
    target: Option<Target>,
    mut meta: Map<String, Value>,
) -> Result<Map<String, Value>, Error> {
    check.await?;
    let Some(target) = target.filter(|_| has_settings(&mut meta)) else {
        return Ok(meta);
    };
    let job = Job::short("Claude's settings file", matter);
    let written = off_actor_or_undo(
        clock,
        job,
        move || match target.write(&mut meta) {
            Ok(path) => Ok((meta, path)),
            Err(error) => {
                let error = crate::log_file::Cause(&error);
                tracing::warn!(%error, "Claude's settings file was not written");
                Err(Error::refused(
                    ErrorCode::AgentLaunchFailed,
                    "Clauseo could not write the agent's settings file.",
                ))
            }
        },
        |(_, path)| path.iter().for_each(|path| remove_file(path)),
    );
    Ok(written.await?.0)
}

/// Removes every settings file of `target`'s run. Off the actor.
pub(crate) fn remove(target: &Target) {
    remove_where(&target.folder, |name| target.owns(name));
}

/// Removes every file in `folder` (`<data>/launch-settings`): an earlier host's leftovers, at
/// the host's first launch (`launch_checks::RunSweep`), before any launch of this host writes
/// one. A folder or a name that is not UTF-8 in it is left. Off the actor.
pub(crate) fn sweep(folder: &Path) {
    remove_where(folder, |_| true);
}

/// Removes each file or link in `folder` whose name `pick` takes; a folder there is left.
fn remove_where(folder: &Path, pick: impl Fn(&str) -> bool) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let picked = entry.file_name().to_str().is_some_and(&pick);
        let file = entry.file_type().is_ok_and(|kind| !kind.is_dir());
        if picked && file {
            remove_file(&entry.path());
        }
    }
}

/// Removes the file `path`, a warning when it stays.
fn remove_file(path: &Path) {
    if let Err(error) = crate::write::remove_old_file(path) {
        tracing::warn!(error = %crate::log_file::Cause(&error), "a Claude settings file stays");
    }
}

#[cfg(test)]
#[path = "launch_settings_tests.rs"]
mod tests;
