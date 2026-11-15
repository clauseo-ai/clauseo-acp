//! The other matters' Windows shell rules in a Claude launch's settings (`super`'s
//! `deny_data_folder`): as many matters as fit the settings file Claude Code reads, and which
//! were left out, for the lawyer's line (`agent::matter_fence`).

use std::ffi::OsString;

use serde_json::{Map, Value};

use super::json_len;

/// The other matters' Windows shell rules (`data_fence::windows_matter_shell_rules`) for a
/// launch whose other settings take `spent` bytes as JSON: every matter's, with the variable
/// forms, when they fit `budget` ([`super::SHED_AT`]); else without them, matter by matter in the
/// fence's order while they fit. The matters left out come second: the lawyer is told
/// (`actor::turn::fence`). Logged with counts only.
pub(crate) fn matter_rules_within(
    data: &crate::agent::data_fence::LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
    spent: usize,
    budget: usize,
) -> (Vec<String>, Vec<clauseo_protocol::MatterId>) {
    let all = crate::agent::data_fence::windows_matter_shell_rules(data, var, true);
    let rules: Vec<String> = all
        .iter()
        .flat_map(|(_, rules)| rules.iter().cloned())
        .collect();
    if spent + json_len(&rules) <= budget {
        return (rules, Vec::new());
    }
    let mut kept = Vec::new();
    let mut left_out = Vec::new();
    // Each rule costs its JSON text and a comma.
    let mut total = spent;
    for (matter, rules) in crate::agent::data_fence::windows_matter_shell_rules(data, var, false) {
        let cost: usize = rules.iter().map(|rule| json_len(rule) + 1).sum();
        if left_out.is_empty() && total + cost <= budget {
            total += cost;
            kept.extend(rules);
        } else {
            left_out.push(matter);
        }
    }
    tracing::warn!(
        settings_bytes = total,
        budget,
        matters_left_out = left_out.len(),
        "Claude's settings outgrew their budget; other matters' shell rules were cut"
    );
    (kept, left_out)
}

/// The other matters whose Windows shell rules a Claude session's `meta` left out
/// ([`matter_rules_within`]): those whose first rule without the variable forms is not among
/// its deny rules. Empty off Windows, or without a fence.
pub(crate) fn unfenced_matters(
    meta: &Map<String, Value>,
    data: &crate::agent::data_fence::LaunchDeny,
    windows: bool,
) -> Vec<clauseo_protocol::MatterId> {
    if !windows || data.matters.others.is_empty() {
        return Vec::new();
    }
    let deny: std::collections::HashSet<&str> = meta
        .get("claudeCode")
        .and_then(|claude| claude.pointer("/options/settings/permissions/deny"))
        .and_then(Value::as_array)
        .map(|rules| rules.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let var = |name: &str| std::env::var_os(name);
    crate::agent::data_fence::windows_matter_shell_rules(data, &var, false)
        .into_iter()
        .filter(|(_, rules)| {
            rules
                .first()
                .is_some_and(|first| !deny.contains(first.as_str()))
        })
        .map(|(matter, _)| matter)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clauseo_protocol::MatterId;
    use serde_json::{Map, Value, json};

    use super::{matter_rules_within, unfenced_matters};
    use crate::agent::data_fence::LaunchDeny;
    use crate::agent::matter_fence::{MatterFence, Other};

    fn id(n: u128) -> MatterId {
        let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
        MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
    }

    /// A launch fencing `n` sibling matters `C:\M\Matter <i>`.
    fn launch(n: u128) -> LaunchDeny {
        let others = (1..=n)
            .map(|i| {
                let folder = PathBuf::from(format!(r"C:\M\Matter {i:03}"));
                let mut other = Other::new(id(i), format!("Matter {i}"), folder.clone(), false);
                other.spellings = vec![folder.clone()];
                other.paths = vec![folder];
                other
            })
            .collect();
        LaunchDeny {
            spellings: vec![PathBuf::from(r"C:\Data\ai.clauseo.desktop")],
            matters: MatterFence::asked(others),
            ..LaunchDeny::default()
        }
    }

    fn meta_with(rules: &[String]) -> Map<String, Value> {
        let meta =
            json!({"claudeCode": {"options": {"settings": {"permissions": {"deny": rules}}}}});
        meta.as_object().cloned().unwrap()
    }

    #[test]
    fn every_matter_fits_a_roomy_budget() {
        let deny = launch(3);
        let (rules, left_out) = matter_rules_within(&deny, &|_| None, 0, 1 << 20);
        assert_eq!(left_out, Vec::<MatterId>::new());
        assert_eq!(
            unfenced_matters(&meta_with(&rules), &deny, true),
            Vec::<MatterId>::new()
        );
    }

    /// The cap: the matters past the budget are left out, in the fence's order, and named.
    #[test]
    fn matters_past_the_budget_are_left_out_and_named() {
        let deny = launch(10);
        let all = matter_rules_within(&deny, &|_| None, 0, 1 << 20).0;
        let budget = super::json_len(&all) / 2;
        let (rules, left_out) = matter_rules_within(&deny, &|_| None, 0, budget);
        // Alone, the kept rules' array gains its brackets and loses a comma.
        assert!(super::json_len(&rules) <= budget + 1);
        assert!(!left_out.is_empty() && left_out.len() < 10, "{left_out:?}");
        let expected: Vec<MatterId> = (11 - u128::try_from(left_out.len()).unwrap()..=10)
            .map(id)
            .collect();
        assert_eq!(left_out, expected);
        assert_eq!(unfenced_matters(&meta_with(&rules), &deny, true), left_out);
        // Off Windows nothing is named.
        assert_eq!(
            unfenced_matters(&meta_with(&rules), &deny, false),
            Vec::<MatterId>::new()
        );
    }
}
