//! The models an agent offers and the efforts each takes, by their real names: what a new
//! conversation's pickers show before its first message ([`AgentModels`], on
//! `AgentSummary::models`), and the model and effort a turn ran with ([`NamedChoice`], on
//! `TurnStarted`, the thread's header and its list row).
//!
//! Ids are the agent's own plain text, as `ConfigOption` keeps them: a command that picks one
//! sends it back in `ConfigPicks`, keyed by [`AgentModels::model_option`] or
//! [`AgentModels::effort_option`].

use serde::{Deserialize, Serialize};

use crate::CompactString;

/// A choice the agent offers by id, with its name to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NamedChoice {
    /// The agent's id for it, sent back in `ConfigPicks`.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub id: CompactString,
    /// Its name, as the agent gives it ("Opus 5.5", "Medium").
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub name: CompactString,
}

/// One model the agent offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ModelEntry {
    /// The model's id, the value of the model option.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub id: CompactString,
    /// Its name.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub name: CompactString,
    /// The agent's description of it, when it gave one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    /// The efforts it takes, lowest first; empty for a model that takes none, or whose efforts
    /// the host has not learnt yet.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub efforts: Vec<NamedChoice>,
    /// The effort the agent recommends for it, one of `efforts`' ids.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub default_effort: Option<CompactString>,
}

/// An agent's models before any session of a conversation, from the host's probe of the agent
/// or its latest session (kept across restarts). `current_model` and `current_effort` are what a
/// new session starts with: the agent's own default unless a session reported otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentModels {
    /// The id of the agent's model option, the key of a model pick in `ConfigPicks`.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub model_option: CompactString,
    /// The id of its effort option, the key of an effort pick; `None` when the agent named none.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub effort_option: Option<CompactString>,
    /// The models, in the agent's order.
    pub models: Vec<ModelEntry>,
    /// The model a new session starts on, one of `models`' ids.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub current_model: CompactString,
    /// The effort it starts with; `None` for a model that takes none.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub current_effort: Option<CompactString>,
}

impl AgentModels {
    /// The entry of the model `id`.
    #[must_use]
    pub fn model(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|model| model.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_optional_fields_read_as_empty() {
        let read: AgentModels = serde_json::from_str(
            r#"{"model_option":"model","models":[{"id":"m","name":"M"}],"current_model":"m"}"#,
        )
        .unwrap();
        assert_eq!(read.effort_option, None);
        assert_eq!(read.current_effort, None);
        let model = read.model("m").unwrap();
        assert_eq!(model.efforts, []);
        assert_eq!(model.default_effort, None);
        assert_eq!(model.description, None);
    }
}
