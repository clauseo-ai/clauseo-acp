//! An agent's kept options as models by name (`AgentSummary::models`), and a probe's models as
//! the options a session would report.

use clauseo_protocol::{
    AgentModels, CompactString, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind,
    ModelEntry, NamedChoice,
};

/// The models `options` offer, each with its efforts from `probed` (the agent's last probe):
/// the model option's choices, its current value, and the effort option's current value. A
/// model the probe did not list takes the effort option's levels when it is the current one,
/// else none. Without a model option in `options`, the probe's models as they are; `None`
/// with neither.
pub(crate) fn view(options: &[ConfigOption], probed: Option<&AgentModels>) -> Option<AgentModels> {
    let Some((model_option, current, choices)) = select_of(options, ConfigCategory::Model) else {
        return probed.cloned().map(described);
    };
    let effort = select_of(options, ConfigCategory::ThoughtLevel);
    let effort_levels: Vec<NamedChoice> = effort
        .map(|(_, _, levels)| levels.iter().map(named).collect())
        .unwrap_or_default();
    let models = choices
        .iter()
        .map(|choice| {
            let known = probed.and_then(|probed| probed.model(&choice.value));
            let (efforts, default_effort) = match known {
                Some(known) => (known.efforts.clone(), known.default_effort.clone()),
                None if choice.value == *current => (effort_levels.clone(), None),
                None => (Vec::new(), None),
            };
            let description = choice
                .description
                .clone()
                .or_else(|| known.and_then(|known| known.description.clone()));
            ModelEntry {
                id: choice.value.clone(),
                name: choice.name.clone(),
                description: description_line(description),
                efforts,
                default_effort,
            }
        })
        .collect();
    let effort_option = effort
        .map(|(id, _, _)| id.clone())
        .or_else(|| probed.and_then(|probed| probed.effort_option.clone()));
    let current_effort = match effort {
        Some((_, value, _)) => Some(value.clone()),
        None => probed
            .and_then(|probed| probed.model(current))
            .and_then(|model| model.default_effort.clone()),
    };
    Some(AgentModels {
        model_option: model_option.clone(),
        effort_option,
        models,
        current_model: current.clone(),
        current_effort,
    })
}

/// `models` with each description through [`description_line`].
fn described(mut models: AgentModels) -> AgentModels {
    for model in &mut models.models {
        model.description = description_line(model.description.take());
    }
    models
}

/// A model's description, as the engine gives it, for the pickers. B3: the description filter
/// (`model_line`, `clauseo_acp::map::describe`) applies here, the one place every
/// `AgentSummary::models` description is set.
fn description_line(description: Option<String>) -> Option<String> {
    description.filter(|text| !text.trim().is_empty())
}

/// The id, current value and choices of the first select option of `category`.
fn select_of(
    options: &[ConfigOption],
    category: ConfigCategory,
) -> Option<(&CompactString, &CompactString, &[ConfigChoice])> {
    options
        .iter()
        .filter(|option| option.category == category)
        .find_map(|option| match &option.kind {
            ConfigOptionKind::Select {
                current_value,
                options: choices,
            } => Some((&option.id, current_value, choices.as_slice())),
            _ => None,
        })
}

fn named(choice: &ConfigChoice) -> NamedChoice {
    NamedChoice {
        id: choice.value.clone(),
        name: choice.name.clone(),
    }
}

/// `old`, the kept options, with their model and effort options replaced by `models`' (in the
/// place of the old ones, else first): the model at the current model, and the effort option
/// with the current model's efforts, left out for a model that takes none. Every other option
/// (a mode, Claude's `fast`) stays as kept.
pub(crate) fn probed_options(old: &[ConfigOption], models: &AgentModels) -> Vec<ConfigOption> {
    let picked = |option: &ConfigOption| {
        matches!(
            option.category,
            ConfigCategory::Model | ConfigCategory::ThoughtLevel
        )
    };
    let mut new = Vec::with_capacity(old.len() + 2);
    new.push(model_option(old, models));
    if let Some(effort) = effort_option(old, models) {
        new.push(effort);
    }
    let at = old.iter().position(picked).unwrap_or(0);
    let mut options: Vec<ConfigOption> = old.iter().filter(|o| !picked(o)).cloned().collect();
    let at = at.min(options.len());
    options.splice(at..at, new);
    options
}

fn model_option(old: &[ConfigOption], models: &AgentModels) -> ConfigOption {
    let choices = models
        .models
        .iter()
        .map(|model| ConfigChoice {
            value: model.id.clone(),
            name: model.name.clone(),
            description: model.description.clone(),
            group: None,
            takes_effort: !model.efforts.is_empty(),
        })
        .collect();
    let kept = old.iter().find(|o| o.category == ConfigCategory::Model);
    ConfigOption {
        id: models.model_option.clone(),
        name: kept.map_or_else(|| "Model".into(), |o| o.name.clone()),
        description: kept.and_then(|o| o.description.clone()),
        category: ConfigCategory::Model,
        kind: ConfigOptionKind::Select {
            current_value: models.current_model.clone(),
            options: choices,
        },
    }
}

fn effort_option(old: &[ConfigOption], models: &AgentModels) -> Option<ConfigOption> {
    let id = models.effort_option.clone()?;
    let current = models.model(&models.current_model)?;
    let value = models.current_effort.clone()?;
    if current.efforts.is_empty() {
        return None;
    }
    let choices = current
        .efforts
        .iter()
        .map(|effort| ConfigChoice {
            value: effort.id.clone(),
            name: effort.name.clone(),
            description: None,
            group: None,
            takes_effort: true,
        })
        .collect();
    let kept = old
        .iter()
        .find(|o| o.category == ConfigCategory::ThoughtLevel);
    Some(ConfigOption {
        id,
        name: kept.map_or_else(|| "Effort".into(), |o| o.name.clone()),
        description: kept.and_then(|o| o.description.clone()),
        category: ConfigCategory::ThoughtLevel,
        kind: ConfigOptionKind::Select {
            current_value: value,
            options: choices,
        },
    })
}
