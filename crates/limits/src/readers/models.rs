//! The model probe: each agent's models and efforts by name, read before any conversation, so a
//! new conversation's pickers name the real models and the agent's own default from the start.
//!
//! - Claude: the adapter as a session launch runs it (the entry's command, the agent's home
//!   forced as `CLAUDE_CONFIG_DIR`, the host's forced variables), then `initialize` and ACP's
//!   `session/options` with `cwd` the data folder, no MCP servers and no `_meta`. The adapter
//!   reads the options from Claude Code's start-up without a prompt and stores no session
//!   (claude-agent-acp 0.110.1, `dist/v1/acp-agent.js:631-660`). The per-model efforts come from
//!   each model choice's AIR metadata, which the adapter gives a client that declares AIR
//!   (`dist/session-model.js:262-276`), as Clauseo's `initialize` does.
//! - Codex: `codex app-server` exactly as the limits reader runs it, then `initialize` and every
//!   page of `model/list` (`codex.rs`).
//!
//! No prompt is sent, no conversation is made, and no MCP server is started. Any failure, a
//! timeout included, gives `None`: the caller keeps what it had.

mod claude;
mod codex;

use clauseo_protocol::{AgentModels, CompactString, ModelEntry, NamedChoice};

pub(crate) use claude::probe as claude_probe;
pub(crate) use codex::probe as codex_probe;

/// The most `model/list` pages read.
const MAX_PAGES: usize = 8;
/// The most models kept of one probe.
const MAX_MODELS: usize = 64;
/// The most efforts kept of one model.
const MAX_EFFORTS: usize = 16;

/// A model entry, its description as the engine gives it.
fn entry(
    id: &str,
    name: &str,
    description: Option<&str>,
    efforts: Vec<NamedChoice>,
    default_effort: Option<&str>,
) -> ModelEntry {
    let default_effort = default_effort
        .filter(|effort| efforts.iter().any(|known| known.id == *effort))
        .map(CompactString::from);
    ModelEntry {
        id: id.into(),
        name: name.into(),
        // As the engine gives it; the host filters it where `AgentSummary::models` is built.
        description: description
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned),
        efforts: efforts.into_iter().take(MAX_EFFORTS).collect(),
        default_effort,
    }
}

fn named(id: &str, name: &str) -> NamedChoice {
    NamedChoice {
        id: id.into(),
        name: name.into(),
    }
}

/// `models` with `current` as the model a session starts on and `effort` its effort; `None`
/// when no model is listed. A `current` the list lacks becomes the first model, and an effort
/// the current model does not take becomes its default.
fn models_of(
    model_option: &str,
    effort_option: Option<&str>,
    mut models: Vec<ModelEntry>,
    current: Option<&str>,
    effort: Option<&str>,
) -> Option<AgentModels> {
    models.truncate(MAX_MODELS);
    let current = current
        .and_then(|id| models.iter().find(|model| model.id == id))
        .or_else(|| models.first())?;
    let current_effort = effort
        .filter(|effort| current.efforts.iter().any(|known| known.id == *effort))
        .map(CompactString::from)
        .or_else(|| current.default_effort.clone());
    Some(AgentModels {
        model_option: model_option.into(),
        effort_option: effort_option.map(CompactString::from),
        current_model: current.id.clone(),
        current_effort,
        models,
    })
}

#[cfg(test)]
mod tests;
