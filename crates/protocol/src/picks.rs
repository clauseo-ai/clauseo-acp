//! The model and effort a conversation runs with, as the lawyer picks them: [`ConfigPicks`]
//! (one agent's picks, `CreateThread::config`) and [`AgentPrefs`] (a matter's remembered picks
//! per agent kind, `UpdateMatter::agent_prefs`). A pick names a config option the agent offers
//! (`ConfigOption`) and one of its values; the host sets only options whose category is
//! `Model` or `ThoughtLevel`.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{AgentChoiceId, AgentKind, Error};

/// The most picks one [`ConfigPicks`] holds: far above the model and effort a conversation
/// takes, so it only stops a client sending an unbounded map.
pub const MAX_CONFIG_PICKS: usize = 8;

/// The rule a [`ConfigPicks`] breaks, as one sentence.
const CONFIG_PICKS_RULE: &str = "config picks hold at most 8 options";

/// Values for an agent's config options: under each option's id (`ConfigOption::id`), the id
/// of the value to set (`ConfigChoice::value`). Select options only. At most
/// [`MAX_CONFIG_PICKS`] entries; [`ConfigPicks::new`] and `Deserialize` check it. Serialised as
/// a JSON object, `{"model": "opus", "effort": "high"}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConfigPicks(BTreeMap<AgentChoiceId, AgentChoiceId>);

impl ConfigPicks {
    /// The picks of `map`.
    ///
    /// # Errors
    ///
    /// [`Error::Format`] (`kind` `"ConfigPicks"`) for more than [`MAX_CONFIG_PICKS`] entries.
    pub fn new(map: BTreeMap<AgentChoiceId, AgentChoiceId>) -> Result<Self, Error> {
        if map.len() > MAX_CONFIG_PICKS {
            return Err(Error::Format {
                kind: "ConfigPicks",
                rule: CONFIG_PICKS_RULE,
            });
        }
        Ok(Self(map))
    }

    /// The picks, by option id.
    pub fn as_map(&self) -> &BTreeMap<AgentChoiceId, AgentChoiceId> {
        &self.0
    }

    /// Whether it holds no pick.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Sets `option` to `value`, replacing its pick. Refused, with nothing changed, when that
    /// would make more than [`MAX_CONFIG_PICKS`] entries.
    ///
    /// # Errors
    ///
    /// [`Error::Format`], as [`ConfigPicks::new`].
    pub fn set(&mut self, option: AgentChoiceId, value: AgentChoiceId) -> Result<(), Error> {
        if !self.0.contains_key(&option) && self.0.len() >= MAX_CONFIG_PICKS {
            return Err(Error::Format {
                kind: "ConfigPicks",
                rule: CONFIG_PICKS_RULE,
            });
        }
        self.0.insert(option, value);
        Ok(())
    }

    /// Takes `option`'s pick out; true when there was one.
    pub fn remove(&mut self, option: &str) -> bool {
        let before = self.0.len();
        self.0.retain(|id, _| id.as_str() != option);
        self.0.len() != before
    }
}

impl<'de> Deserialize<'de> for ConfigPicks {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = BTreeMap::<AgentChoiceId, AgentChoiceId>::deserialize(deserializer)?;
        Self::new(map).map_err(de::Error::custom)
    }
}

/// A matter's remembered picks, per agent kind: a new conversation in the matter that names no
/// `CreateThread::config` starts with its agent kind's picks. Serialised as a JSON object keyed
/// by kind, `{"claude": {"model": "opus"}}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentPrefs(pub BTreeMap<AgentKind, ConfigPicks>);

impl AgentPrefs {
    /// `kind`'s picks; `None` when the matter has none for it.
    pub fn of(&self, kind: AgentKind) -> Option<&ConfigPicks> {
        self.0.get(&kind).filter(|picks| !picks.is_empty())
    }

    /// Whether it holds no pick for any kind.
    pub fn is_empty(&self) -> bool {
        self.0.values().all(ConfigPicks::is_empty)
    }
}

#[cfg(test)]
mod tests;
