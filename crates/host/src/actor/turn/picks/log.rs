//! What a host.log line about picks writes of a model or effort value: the value only when its
//! option lists it and it is a short plain id ([`loggable`]), `held` for a pick its option
//! held already, else `-` (record 0047 keeps settings values out of the log).

use clauseo_protocol::{AgentChoiceId, ConfigCategory, ConfigOption, ConfigOptionKind};

use super::Pick;

/// The longest value a host.log line about picks writes as itself.
const LOGGED_VALUE_MAX: usize = 40;

/// The model and the effort among `picks`, by their options' categories in `options`; else
/// `held` for one among `held`, a pick left out because its option held the value already;
/// `-` for none. All a host.log line about picks carries besides the agent id: no other
/// option. A value shows only when it is one its option in `options` lists and a short plain
/// id ([`loggable`]); otherwise `-`: record 0047 keeps settings values out of the log, and a
/// firm's model id on a cloud provider can carry its account number (a Bedrock ARN).
pub(super) fn model_and_effort<'a>(
    options: &[ConfigOption],
    picks: &'a [Pick],
    held: &[Pick],
) -> (&'a str, &'a str) {
    let of_category = |id: &AgentChoiceId, category: ConfigCategory| {
        (options.iter()).find(|option| option.id == id.as_str() && option.category == category)
    };
    let of = |category: ConfigCategory| {
        let sent = (picks.iter()).find_map(|(id, value)| {
            of_category(id, category).map(|option| loggable(option, value.as_str()))
        });
        let was_held = || {
            held.iter()
                .any(|(id, _)| of_category(id, category).is_some())
        };
        match sent {
            Some(value) => value,
            None if was_held() => "held",
            None => "-",
        }
    };
    (of(ConfigCategory::Model), of(ConfigCategory::ThoughtLevel))
}

/// A pick's value as a host.log line writes it: [`loggable`] against its option in `options`,
/// `-` when `options` hold no option of its id.
pub(super) fn pick_value<'a>(options: &[ConfigOption], (id, value): &'a Pick) -> &'a str {
    options
        .iter()
        .find(|option| option.id == id.as_str())
        .map_or("-", |option| loggable(option, value.as_str()))
}

/// `value` when `option` lists it among its choices and it is at most [`LOGGED_VALUE_MAX`]
/// characters of `[A-Za-z0-9._-]`; else `-`.
fn loggable<'a>(option: &ConfigOption, value: &'a str) -> &'a str {
    let listed = matches!(&option.kind, ConfigOptionKind::Select { options, .. }
        if options.iter().any(|choice| choice.value == value));
    let short = !value.is_empty()
        && value.len() <= LOGGED_VALUE_MAX
        && (value.bytes()).all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if listed && short { value } else { "-" }
}
