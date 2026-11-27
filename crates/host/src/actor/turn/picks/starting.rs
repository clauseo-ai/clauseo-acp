//! A per-agent STARTING EFFORT: the effort a conversation's first session starts with when the
//! lawyer has never chosen one, instead of the engine's own. One constant per agent kind:
//! Codex starts at `low`, Claude at `medium`; `None` would leave the engine's own.
//!
//! "Never chosen": the thread's first session on its agent (that agent has logged no options
//! here, so a reopened conversation keeps the values it ran with), and its picks hold no effort
//! pick: none in the thread's kept values for the agent (`own.rs`), none in `CreateThread`'s
//! config, none in the matter's remembered picks for the agent's kind
//! (`AgentPrefs`, where every effort the lawyer sets is kept), and no `SetConfigOption` of the
//! effort waiting for the session. There is no global memory of picks: the agent's kept options
//! (`agent-options.json`) hold the engine's starting values, never a lawyer's choice.
//!
//! It applies only when the start's options offer it for the model the session starts on, and
//! never with a pick that changes the model. It goes through the same picks as a lawyer's
//! (`picks::apply`), so `TurnStarted`, the header and the composer show the effort in force,
//! and it is never kept in the matter's prefs: a later pick of the lawyer's wins, and stays. An
//! agent that refuses it or does not list it gives no kept-value notice: that notice is only for
//! the thread's kept values, and it is none of them when the session starts (`lost` in
//! `picks::after`). What the first session then runs with becomes the thread's own, as any first
//! session's values do (`own::seed_first`).

use clauseo_protocol::{AgentChoiceId, AgentKind, ConfigCategory, ConfigOption, ConfigOptionKind};

use super::Pick;

/// Claude's starting effort.
pub(crate) const CLAUDE: Option<&str> = Some("medium");
/// Codex's starting effort.
pub(crate) const CODEX: Option<&str> = Some("low");

/// The starting effort of `kind`.
pub(super) const fn of(kind: AgentKind) -> Option<&'static str> {
    match kind {
        AgentKind::Claude => CLAUDE,
        AgentKind::Codex => CODEX,
    }
}

/// The starting effort's pick for a session whose start reported `options`, given `picks`,
/// every pick the session takes (those the options hold already included), and whether this is
/// the thread's first session; `None` when the lawyer has chosen an effort, the effort is unset,
/// held already, or not offered for the starting model, or a pick changes the model.
pub(super) fn first_pick(
    effort: Option<&str>,
    first_session: bool,
    picks: &[Pick],
    options: &[ConfigOption],
) -> Option<Pick> {
    let effort = effort.filter(|_| first_session)?;
    let category = |id: &AgentChoiceId| {
        options
            .iter()
            .find(|option| option.id == id.as_str())
            .map(|option| (option.category, current(option)))
    };
    for (id, value) in picks {
        match category(id) {
            Some((ConfigCategory::ThoughtLevel, _)) => return None,
            Some((ConfigCategory::Model, current)) if current != Some(value.as_str()) => {
                return None;
            }
            _ => {}
        }
    }
    let option = options.iter().find(|option| {
        option.category == ConfigCategory::ThoughtLevel
            && matches!(option.kind, ConfigOptionKind::Select { .. })
    })?;
    let ConfigOptionKind::Select {
        current_value,
        options: levels,
    } = &option.kind
    else {
        return None;
    };
    if current_value == effort || !levels.iter().any(|level| level.value == effort) {
        return None;
    }
    let id = AgentChoiceId::new(option.id.as_str()).ok()?;
    Some((id, AgentChoiceId::new(effort).ok()?))
}

fn current(option: &ConfigOption) -> Option<&str> {
    match &option.kind {
        ConfigOptionKind::Select { current_value, .. } => Some(current_value.as_str()),
        ConfigOptionKind::Boolean { .. } | ConfigOptionKind::Unknown { .. } => None,
    }
}

#[cfg(test)]
mod tests;
