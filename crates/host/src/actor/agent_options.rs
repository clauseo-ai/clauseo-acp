//! The config options each agent's latest session reported, per agent id, for
//! `AgentSummary::config_options`: a new conversation's draft offers the model and effort
//! before its session starts, after a restart too. Kept in `<data_dir>/agent-options.json`,
//! `{"v":1,"agents":{"<agent id>":[<ConfigOption>, ...]}}`, written whole and atomically
//! (`write::replace_private_file`), as `connectors/hints.json` is.
//!
//! Every durable `ConfigOptionsReplaced` of a thread (the session's start, a `SetConfigOption`'s
//! answer, the agent's own update; `rail::note_commit`) replaces its agent's options when they
//! differ, and every app subscriber then gets `SettingsChanged`, on which a window lists the
//! agents again. An empty list replaces nothing. An agent with no session yet has none.
//!
//! Read once, at the first need. A file that is missing, does not read, is over [`CAP`] or is of
//! another version counts as none (logged): the worst a damaged file does is hide the pickers
//! until the agent's next session starts. A write that fails is logged and the options hold in
//! memory for the host's run.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use clauseo_protocol::{ConfigOption, HostEvent, ThreadId};
use serde::{Deserialize, Serialize};

use super::Actor;
use crate::log_file::Cause;

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
}

impl AgentOptions {
    /// The file in `data_dir`; nothing is read yet.
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(FILE),
            kept: None,
        }
    }

    /// `agent`'s options as its latest session reported them; empty before any.
    pub(crate) fn of(&mut self, agent: &str) -> &[ConfigOption] {
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

/// `thread` logged `options` durably: they become its agent's, and every app subscriber gets
/// `SettingsChanged` when that changed them.
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
    let changed = match actor.agent_options.keep(agent.as_str(), options) {
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

#[cfg(test)]
mod tests;
