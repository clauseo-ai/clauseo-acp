//! The helpers `Effective::load` uses to lock keys and say why: the warning for a policy `true`
//! on a key it may only turn off, and the locks a source that cannot be read or a stronger
//! source leaves. Split from `effective.rs` for its length.

use std::collections::BTreeSet;

use crate::leaf::{self, Leaf};
use crate::schema::{Plan, Settings};
use crate::warning::{Origin, Warning, WarningKind};

/// The warning for a policy `true` on a key a policy may only turn off
/// (`leaf::OFF_ONLY_KEYS`): it is ignored and locks nothing.
pub(super) fn policy_on_ignored(name: &str, key: String) -> Warning {
    Warning {
        origin: Origin::Store {
            name: name.to_owned(),
        },
        kind: WarningKind::InvalidValue {
            message: format!(
                "a policy can only turn {key} off, so true is ignored and the lawyer's switch stands"
            ),
            key,
        },
    }
}

/// Locks each security key a stronger source has not set off ([`leaf::refuse`]), for the
/// unreadable source `name`: what it held for the key is unknown, so the user's value must not
/// stand in for it, and the default may be on. Returns the keys it locked.
pub(super) fn lock_security_keys(
    name: &str,
    settings: &mut Settings,
    locked: &mut BTreeSet<String>,
    warnings: &mut Vec<Warning>,
) -> Vec<&'static str> {
    let mut keys = Vec::new();
    for key in leaf::SECURITY_KEYS {
        if lock_and_apply(Leaf::Refused(key.to_owned()), settings, locked) {
            let word = leaf::locked_word(key);
            let message = format!("the policy source could not be read, so this is locked {word}");
            keys.push(key);
            warnings.push(Warning {
                origin: Origin::Store {
                    name: name.to_owned(),
                },
                kind: WarningKind::InvalidValue {
                    key: key.to_owned(),
                    message,
                },
            });
        }
    }
    keys
}

/// Locks `leaf`'s key and applies it as policy, unless a stronger source locked that key, or a
/// section over it (`plans.`), already. Each plan a stronger source locked stands through a
/// refusal of the whole section. Returns whether it applied.
pub(super) fn lock_and_apply(
    leaf: Leaf,
    settings: &mut Settings,
    locked: &mut BTreeSet<String>,
) -> bool {
    let key = leaf.key();
    if locked.contains(&key) || under_section_lock(locked, &key) {
        return false;
    }
    let kept: Vec<(String, Plan)> = settings
        .plans
        .iter()
        .filter(|(id, _)| locked.contains(&format!("plans.{id}")))
        .map(|(id, plan)| (id.clone(), plan.clone()))
        .collect();
    locked.insert(key);
    leaf.apply_policy(settings);
    settings.plans.extend(kept);
    true
}

/// Whether a locked section (a key ending in `.`) holds `key`, which is not that section.
pub(super) fn under_section_lock(locked: &BTreeSet<String>, key: &str) -> bool {
    locked
        .iter()
        .any(|lock| lock.ends_with('.') && key.len() > lock.len() && key.starts_with(lock.as_str()))
}
