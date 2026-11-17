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
//! An agent with no session yet has none kept: a Claude or Codex agent then gives its seed
//! (`seed.rs`, the agent's own default, never sent), so its first conversation's draft shows the
//! pickers too; any other agent gives none. The seed is only ever shown, never kept: what is
//! kept comes from a session's reports alone, so the first session's start replaces it whole,
//! and [`merged`] never carries its value over.
//!
//! Read once, at the first need. A file that is missing, does not read, is over [`CAP`] or is of
//! another version counts as none (logged): the worst a damaged file does is hide the pickers
//! until the agent's next session starts. A write that fails is logged and the options hold in
//! memory for the host's run.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clauseo_protocol::{ConfigOption, ConfigOptionKind, HostEvent, ThreadId};
use serde::{Deserialize, Serialize};

use super::Actor;
use crate::log_file::Cause;

pub(crate) mod efforts;

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
}

/// The kept options, read at the first need.
#[derive(Debug)]
pub(crate) struct AgentOptions {
    path: PathBuf,
    kept: Option<BTreeMap<String, Vec<ConfigOption>>>,
    /// The options each thread's session started with, until its next durable options report
    /// ([`note`]); empty when the start reported none, so that report is the start's.
    starting: BTreeMap<ThreadId, Vec<ConfigOption>>,
}

impl AgentOptions {
    /// The file in `data_dir`; nothing is read yet.
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(FILE),
            kept: None,
            starting: BTreeMap::new(),
        }
    }

    /// `thread`'s session started with `options`, as the agent reported them before any pick
    /// is set: its next durable options report keeps these instead.
    pub(crate) fn started(&mut self, thread: ThreadId, options: &[ConfigOption]) {
        self.starting.insert(thread, options.to_vec());
    }

    /// `agent`'s options as its latest session reported them; before any, its kind's seed
    /// ([`seed::of`]: the model and effort at the agent's own default), else none.
    pub(crate) fn of(&mut self, agent: &str) -> &[ConfigOption] {
        self.kept()
            .get(agent)
            .map_or_else(|| seed::of(agent), Vec::as_slice)
    }

    /// `agent`'s options as its sessions reported them, with no seed: empty before any.
    fn stored(&mut self, agent: &str) -> &[ConfigOption] {
        self.kept().get(agent).map_or(&[], Vec::as_slice)
    }

    /// Keeps `options` as `agent`'s and writes the file when they differ from the kept ones;
    /// true when they did. The change holds in memory even when the write fails (the error is
    /// returned for the log).
    pub(crate) fn keep(&mut self, agent: &str, options: &[ConfigOption]) -> io::Result<bool> {
        let kept = self.kept();
        if kept.get(agent).is_some_and(|old| old == options) {
            return Ok(false);
        }
        kept.insert(agent.to_owned(), options.to_vec());
        let disk = OnDisk {
            v: VERSION,
            agents: kept.clone(),
        };
        let bytes = serde_json::to_vec(&disk).map_err(io::Error::other)?;
        let folder = self.path.parent().unwrap_or(&self.path);
        crate::write::create_dir_all(folder)?;
        crate::write::replace_private_file(&self.path, &bytes)?;
        Ok(true)
    }

    fn kept(&mut self) -> &mut BTreeMap<String, Vec<ConfigOption>> {
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
        None => merged(actor.agent_options.stored(agent.as_str()), options),
    };
    efforts::carry(&mut options, reported);
    let changed = match actor.agent_options.keep(agent.as_str(), &options) {
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
/// model) stays as kept, and one with none kept, or of another kind, is as reported. `kept`
/// holds no seed (`AgentOptions::stored`); a kept option at the seed's value counts as none
/// kept all the same ([`seed::is_seed`]), so [`seed::AGENT_DEFAULT`] is never kept over a
/// session's report.
fn merged(kept: &[ConfigOption], reported: &[ConfigOption]) -> Vec<ConfigOption> {
    reported
        .iter()
        .map(|option| {
            let Some(old) = kept
                .iter()
                .find(|old| old.id == option.id && !seed::is_seed(old))
            else {
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
fn read(path: &Path) -> BTreeMap<String, Vec<ConfigOption>> {
    let mut bytes = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|file| file.take(CAP + 1).read_to_end(&mut bytes));
    match read {
        Ok(_) if bytes.len() as u64 > CAP => {
            tracing::warn!("agent-options.json is over its cap; no options");
            return BTreeMap::new();
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(error) => {
            tracing::warn!(error = %Cause(&error), "agent-options.json does not read; no options");
            return BTreeMap::new();
        }
    }
    match serde_json::from_slice::<OnDisk>(&bytes) {
        Ok(disk) if disk.v == VERSION => disk.agents,
        Ok(disk) => {
            tracing::warn!(
                version = disk.v,
                "agent-options.json is of another version; no options"
            );
            BTreeMap::new()
        }
        Err(_unsaid) => {
            tracing::warn!("agent-options.json does not parse; no options");
            BTreeMap::new()
        }
    }
}

pub(crate) mod seed;
#[cfg(test)]
mod tests;
