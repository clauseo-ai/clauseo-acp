//! A model value in the form the session lists: Claude names a model by its short name
//! (`fable`, listed as "Fable 5.1") or its full id (`claude-fable-5-1`), and a resumed session
//! may list the other form (claude-agent-acp 0.110.1, `aliasResumedModelRow`,
//! `session-model.js:333`); the agent takes either (`session-config-options.js:195-205`).
//! Two forms match only with the same family and version: the full id's numbers (a date left
//! out) against the version in the short name's choice name, the listed one's or, for a kept
//! short name, the one `kept` (the thread's logged options) gives. Only `claude-` ids match.

use clauseo_protocol::{AgentChoiceId, ConfigCategory, ConfigOption, ConfigOptionKind};

use super::Pick;

/// `pick` with the value its model option in `options` lists for the same model, when it lists
/// not the value itself but its other form; else `pick` as it is.
pub(in crate::actor::turn) fn listed(
    options: &[ConfigOption],
    kept: &[ConfigOption],
    pick: Pick,
) -> Pick {
    let Some(choices) = model_choices(options, &pick.0) else {
        return pick;
    };
    let value = pick.1.as_str();
    if choices.iter().any(|(listed, _)| *listed == value) {
        return pick;
    }
    let kept_name = model_choices(kept, &pick.0)
        .and_then(|kept| kept.into_iter().find(|(id, _)| *id == value))
        .map(|(_, name)| name);
    let same = choices
        .into_iter()
        .find(|&(listed, name)| same_model((listed, name), (value, kept_name.unwrap_or_default())));
    match same.and_then(|(listed, _)| AgentChoiceId::new(listed).ok()) {
        Some(listed) => (pick.0, listed),
        None => pick,
    }
}

/// The values and names of the model option `id` among `options`.
fn model_choices<'a>(
    options: &'a [ConfigOption],
    id: &AgentChoiceId,
) -> Option<Vec<(&'a str, &'a str)>> {
    let option = options.iter().find(|option| option.id == id.as_str())?;
    let ConfigOptionKind::Select {
        options: choices, ..
    } = &option.kind
    else {
        return None;
    };
    (option.category == ConfigCategory::Model).then(|| {
        (choices.iter())
            .map(|choice| (choice.value.as_str(), choice.name.as_str()))
            .collect()
    })
}

/// Whether `a` and `b`, each a value and its choice name, are one model's short name and full
/// id: the family the same, and the full id's version the one in the short name's choice name.
fn same_model(a: (&str, &str), b: (&str, &str)) -> bool {
    let one_way = |(short, name): (&str, &str), (full, _): (&str, &str)| {
        let Some(rest) = (full.strip_prefix("claude-"))
            .and_then(|rest| rest.strip_prefix(short)?.strip_prefix('-'))
        else {
            return false;
        };
        let version: Vec<u32> = (rest.split('-'))
            .map_while(|part| part.parse().ok())
            .filter(|number| *number < 1000)
            .collect();
        let named: Vec<u32> = (name.rsplit(' ').next().unwrap_or_default().split('.'))
            .map_while(|part| part.parse().ok())
            .collect();
        !short.is_empty()
            && short.bytes().all(|b| b.is_ascii_lowercase())
            && !version.is_empty()
            && version == named
    };
    one_way(a, b) || one_way(b, a)
}

#[cfg(test)]
mod tests {
    use super::same_model;

    #[test]
    fn a_short_name_and_its_full_id_name_the_same_model_only_at_the_same_version() {
        assert!(same_model(("fable", "Fable 5.1"), ("claude-fable-5-1", "")));
        assert!(same_model(("claude-opus-5-5", ""), ("opus", "Opus 5.5")));
        assert!(same_model(
            ("haiku", "Haiku 4.5"),
            ("claude-haiku-4-5-20251001", "")
        ));
        assert!(!same_model(("opus", "Opus 5.5"), ("claude-opus-4-9", "")));
        assert!(!same_model(
            ("opus", "Opus 5.5"),
            ("claude-opus-5", "Opus 5")
        ));
        assert!(!same_model(("opus", "Opus 5.5"), ("claude-sonnet-5-5", "")));
        assert!(!same_model(("fable", ""), ("claude-fable-5-1", "")));
        assert!(!same_model(("gpt", "GPT 6"), ("gpt-6", "")));
    }
}
