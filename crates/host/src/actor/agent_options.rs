//! The config options each agent's latest session reported, per agent id, for
//! `AgentSummary::config_options`: a new conversation's draft offers the model and effort
//! before its session starts, after a restart too. Kept in `<data_dir>/agent-options.json`,
//! `{"v":1,"agents":{"<agent id>":[<ConfigOption>, ...]}}`, written whole and atomically
//! (`write::replace_private_file`), as `connectors/hints.json` is.
//!
//! Every durable `ConfigOptionsReplaced` of a thread (the session's start, a `SetConfigOption`'s
//! answer, the agent's own update; `rail::note_commit`) replaces its agent's options when they
//! differ, and every app subscriber then gets `SettingsChanged`, on which a window lists the
//! agents again. An empty list replaces nothing.
//!
//! Each option's current value is the one the session started with: the options the agent
//! reported in its `session/new`, `session/load` or `session/resume` answer ([`started`],
//! taken before Clauseo sets any pick), else its first options report. Every later report (a
//! pick's answer, the agent's own update) refreshes the options and their values but keeps each
//! kept current that it still offers ([`merged`]), so a pick in one conversation never becomes
//! another's draft default. An option the report drops goes; one with no kept current takes the
//! report's.
//!
//! The host's model probe (`probe.rs`) also fills it, at start and after a sign-in, before any
//! session: the agent's models and per-model efforts by name (`AgentModels`, kept under
//! `"models"` beside `"agents"`), and its model and effort options as a session would report
//! them (`models.rs`). An agent that neither a probe nor a session has reached has nothing kept:
//! no options and no models, so the window shows no pickers until the probe answers.
//!
//! Read once, at the first need. A file that is missing, does not read, is over [`CAP`] or is of
//! another version counts as none (logged): the worst a damaged file does is hide the pickers
//! until the agent's next session starts. A write that fails is logged and the options hold in
//! memory for the host's run.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentModels, ConfigOption, ConfigOptionKind, HostEvent, ThreadId};
use serde::{Deserialize, Serialize};

use super::Actor;
use crate::log_file::Cause;

pub(crate) mod efforts;
mod models;
pub(crate) mod probe;

/// The file's name in the data folder.
pub(crate) const FILE: &str = "agent-options.json";
/// The most of the file read, 4 MiB: an agent's options are a few kilobytes (Codex's eight
/// models and their efforts), so a larger file is not one this host wrote.
const CAP: u64 = 4 * 1024 * 1024;
/// The version this host writes.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct OnDisk {
    v: u32,
    #[serde(default)]
    agents: BTreeMap<String, Vec<ConfigOption>>,
    /// Each agent's models as its last probe found them; missing in a file written before them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    models: BTreeMap<String, AgentModels>,
}

/// What is kept: each agent's options and its probed models.
#[derive(Debug, Default)]
struct Kept {
    options: BTreeMap<String, Vec<ConfigOption>>,
    models: BTreeMap<String, AgentModels>,
}

/// The kept options, read at the first need.
#[derive(Debug)]
pub(crate) struct AgentOptions {
    path: PathBuf,
    kept: Option<Kept>,
    /// The options each thread's session started with, until its next durable options report
    /// ([`note`]); empty when the start reported none, so that report is the start's.
    starting: BTreeMap<ThreadId, Vec<ConfigOption>>,
    /// The model probes run since the start (`probe.rs`).
    pub(crate) probes: probe::Probes,
}

impl AgentOptions {
    /// The file in `data_dir`; nothing is read yet.
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(FILE),
            kept: None,
            starting: BTreeMap::new(),
            probes: probe::Probes::default(),
        }
    }

    /// `thread`'s session started with `options`, as the agent reported them before any pick
    /// is set: its next durable options report keeps these instead.
    pub(crate) fn started(&mut self, thread: ThreadId, options: &[ConfigOption]) {
        self.starting.insert(thread, options.to_vec());
    }

    /// `agent`'s options as its latest session (or probe) reported them; none before either.
    pub(crate) fn of(&mut self, agent: &str) -> &[ConfigOption] {
        self.kept().options.get(agent).map_or(&[], Vec::as_slice)
    }

    /// `agent`'s models by name, for `AgentSummary::models`: its kept options' model and effort,
    /// with each model's efforts from its last probe (`models::view`); `None` before either.
    pub(crate) fn models_of(&mut self, agent: &str) -> Option<AgentModels> {
        let kept = self.kept();
        let options = kept.options.get(agent).map_or(&[][..], Vec::as_slice);
        models::view(options, kept.models.get(agent))
    }

    /// Keeps `models`, a probe's answer, as `agent`'s, with its model and effort options over
    /// the kept ones (`models::probed_options`); true when that changed anything.
    pub(crate) fn keep_probed(&mut self, agent: &str, models: AgentModels) -> io::Result<bool> {
        let kept = self.kept();
        let old = kept.options.get(agent).map_or(&[][..], Vec::as_slice);
        let options = models::probed_options(old, &models);
        let same_models = kept.models.get(agent) == Some(&models);
        if same_models && kept.options.get(agent) == Some(&options) {
            return Ok(false);
        }
        kept.models.insert(agent.to_owned(), models);
        kept.options.insert(agent.to_owned(), options);
        self.write().map(|()| true)
    }

    /// Keeps `options` as `agent`'s and writes the file when they differ from the kept ones;
    /// true when they did. The change holds in memory even when the write fails (the error is
    /// returned for the log).
    pub(crate) fn keep(&mut self, agent: &str, options: &[ConfigOption]) -> io::Result<bool> {
        let kept = self.kept();
        if kept.options.get(agent).is_some_and(|old| old == options) {
            return Ok(false);
        }
        kept.options.insert(agent.to_owned(), options.to_vec());
        self.write().map(|()| true)
    }

    /// Writes the file whole.
    fn write(&mut self) -> io::Result<()> {
        let kept = self.kept();
        let disk = OnDisk {
            v: VERSION,
            agents: kept.options.clone(),
            models: kept.models.clone(),
        };
        let bytes = serde_json::to_vec(&disk).map_err(io::Error::other)?;
        let folder = self.path.parent().unwrap_or(&self.path);
        crate::write::create_dir_all(folder)?;
        crate::write::replace_private_file(&self.path, &bytes)?;
        Ok(())
    }

    fn kept(&mut self) -> &mut Kept {
        let path = &self.path;
        self.kept.get_or_insert_with(|| read(path))
    }
}

/// `thread` logged `options` durably: they become its agent's, each current value the session
/// started with (module doc), and every app subscriber gets `SettingsChanged` when that changed
/// them.
pub(super) fn note(actor: &mut Actor, thread: ThreadId, options: &[ConfigOption]) {
    let Some(agent) = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone())
    else {
        return;
    };
    if options.is_empty() {
        return;
    }
    let reported = options;
    let mut options = match actor.agent_options.starting.remove(&thread) {
        Some(start) if !start.is_empty() => start,
        Some(_) => options.to_vec(),
        None => merged(actor.agent_options.of(agent.as_str()), options),
    };
    efforts::carry(&mut options, reported);
    let kept = actor.agent_options.keep(agent.as_str(), &options);
    told(actor, kept);
}

/// Every app subscriber gets `SettingsChanged` when a keep changed the kept options (or failed
/// to write them, which still changed them in memory).
pub(super) fn told(actor: &mut Actor, kept: io::Result<bool>) {
    let changed = match kept {
        Ok(changed) => changed,
        Err(error) => {
            tracing::warn!(error = %Cause(&error), "agent-options.json was not written");
            true
        }
    };
    if changed {
        let view = super::settings::view(actor);
        super::app::publish(actor, &HostEvent::SettingsChanged(view));
    }
}

/// `reported`, a session's later options, with each current value from `kept` that `reported`
/// still offers; an option whose kept current it no longer offers (the efforts of a picked
/// model) stays as kept, and one with none kept, or of another kind, is as reported.
fn merged(kept: &[ConfigOption], reported: &[ConfigOption]) -> Vec<ConfigOption> {
    reported
        .iter()
        .map(|option| {
            let Some(old) = kept.iter().find(|old| old.id == option.id) else {
                return option.clone();
            };
            let mut option = option.clone();
            match (&old.kind, &mut option.kind) {
                (
                    ConfigOptionKind::Select { current_value, .. },
                    ConfigOptionKind::Select {
                        current_value: now,
                        options,
                    },
                ) => {
                    if !options.iter().any(|choice| choice.value == *current_value) {
                        return old.clone();
                    }
                    now.clone_from(current_value);
                }
                (
                    ConfigOptionKind::Boolean { current_value },
                    ConfigOptionKind::Boolean { current_value: now },
                ) => *now = *current_value,
                _ => {}
            }
            option
        })
        .collect()
}

/// The kept options on disk; none when the file is missing or unusable.
fn read(path: &Path) -> Kept {
    let mut bytes = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|file| file.take(CAP + 1).read_to_end(&mut bytes));
    match read {
        Ok(_) if bytes.len() as u64 > CAP => {
            tracing::warn!("agent-options.json is over its cap; no options");
            return Kept::default();
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Kept::default(),
        Err(error) => {
            tracing::warn!(error = %Cause(&error), "agent-options.json does not read; no options");
            return Kept::default();
        }
    }
    match serde_json::from_slice::<OnDisk>(&bytes) {
        Ok(disk) if disk.v == VERSION => Kept {
            options: disk.agents,
            models: disk.models,
        },
        Ok(disk) => {
            tracing::warn!(
                version = disk.v,
                "agent-options.json is of another version; no options"
            );
            Kept::default()
        }
        Err(_unsaid) => {
            tracing::warn!("agent-options.json does not parse; no options");
            Kept::default()
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
