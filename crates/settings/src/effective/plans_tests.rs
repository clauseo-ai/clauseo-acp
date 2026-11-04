//! The `plans` and `install` sections at load (decision record 0048): the safe values with a
//! policy source unreadable, and the refusal of a secret-shaped variable name at every layer.

use super::Effective;
use crate::error::Error;
use crate::plans::SECRET_WORDS;
use crate::policy::{PolicyRead, managed_prefs, registry};
use crate::schema::{InstallSettings, PlanProvider, Settings};
use crate::testing::{FakeSource, abs, abs_json, locations, policy, user};
use crate::warning::WarningKind;

/// A value no warning may quote; built at run time.
fn secret() -> String {
    format!("sk{}", "-ant-never-warned-8910")
}

fn unreadable() -> FakeSource<impl Fn() -> PolicyRead> {
    FakeSource {
        name: "broken",
        read: || {
            PolicyRead::Unreadable(Error::Parse {
                path: abs("policy.json"),
                line: 1,
                column: 1,
                message: "expected value".to_owned(),
            })
        },
    }
}

/// `settings.json` where the lawyer chose a gateway for Claude and both install keys.
fn lawyers_choices() -> String {
    format!(
        r#"{{
            "plans": {{"claude-acp": {{"endpoint": "https://elsewhere.example", "env": {{"A": "1"}}}}}},
            "install": {{"bundle": "{}", "registry": "https://npm.elsewhere.example"}}
        }}"#,
        abs_json("bundle")
    )
}

#[test]
fn the_lawyers_plans_and_install_keys_load_unlocked() {
    let effective = Effective::load(&locations(), &user(&lawyers_choices()), &[]);
    let plan = effective.plan("claude-acp").unwrap();
    assert_eq!(plan.endpoint.as_deref(), Some("https://elsewhere.example"));
    assert_eq!(effective.settings().install.bundle, Some(abs("bundle")));
    assert!(!effective.is_locked("plans.claude-acp"));
    assert!(!effective.is_locked("install.registry"));
    assert!(
        effective.warnings().is_empty(),
        "{:?}",
        effective.warnings()
    );
}

#[test]
fn with_policy_unreadable_plans_and_install_take_their_safe_values() {
    let effective = Effective::load(&locations(), &user(&lawyers_choices()), &[&unreadable()]);
    // No plan: no endpoint the lawyer chose stands in for the firm's.
    assert!(effective.settings().plans.is_empty());
    // Install keys unset: the pinned public hosts, with their integrity checks.
    assert_eq!(effective.settings().install, InstallSettings::default());
    for key in [
        "plans.claude-acp",
        "plans.codex-acp",
        "install.bundle",
        "install.registry",
    ] {
        assert!(effective.is_locked(key), "{key}");
    }
    assert!(effective.locked().any(|key| key == "plans."));
}

#[test]
fn a_stronger_sources_plan_stands_when_a_weaker_one_is_unreadable() {
    let firm = policy(
        "firm",
        r#"{"plans": {"claude-acp": {"provider": "vertex"}}, "install": {"registry": "https://npm.firm.example"}}"#,
    );
    let effective = Effective::load(
        &locations(),
        &user(&lawyers_choices()),
        &[&firm, &unreadable()],
    );
    assert_eq!(effective.settings().plans.len(), 1);
    assert_eq!(
        effective.plan("claude-acp").and_then(|plan| plan.provider),
        Some(PlanProvider::Vertex)
    );
    assert_eq!(
        effective.settings().install.registry.as_deref(),
        Some("https://npm.firm.example")
    );
    assert_eq!(effective.settings().install.bundle, None);
    // An unreadable stronger source wins over a weaker one's plan: its value is unknown.
    let effective = Effective::load(
        &locations(),
        &user(&lawyers_choices()),
        &[&unreadable(), &firm],
    );
    assert!(effective.settings().plans.is_empty());
    assert_eq!(effective.settings().install, InstallSettings::default());
}

#[test]
fn a_plans_section_the_policy_misnames_locks_every_plan() {
    for text in [r#"{"plans": "off"}"#, r#"{"Plans": {"claude-acp": {}}}"#] {
        let firm = policy("firm", text);
        let effective = Effective::load(&locations(), &user(&lawyers_choices()), &[&firm]);
        assert!(effective.settings().plans.is_empty(), "{text}");
        assert!(effective.is_locked("plans.claude-acp"), "{text}");
    }
    // Managed preferences: a `plans` value that is no dictionary.
    let read = managed_prefs::parse(vec![(
        "plans".to_owned(),
        managed_prefs::PlistValue::String("off".to_owned()),
        true,
    )]);
    let prefs = FakeSource {
        name: managed_prefs::SOURCE,
        read: move || match &read {
            PolicyRead::Read(values) => PolicyRead::Read(values.clone()),
            _ => PolicyRead::Absent,
        },
    };
    let effective = Effective::load(&locations(), &user(&lawyers_choices()), &[&prefs]);
    assert!(effective.settings().plans.is_empty());
    assert!(effective.is_locked("plans.claude-acp"));
}

/// The variables of `plans.claude-acp` as `text` (a `settings.json` or a `policy.json`) sets them
/// with each banned word, spelled three ways, beside one allowed name.
fn banned_env(section_only: bool) -> String {
    let mut env = serde_json::Map::new();
    for word in SECRET_WORDS {
        for name in [
            word.to_owned(),
            word.to_ascii_lowercase(),
            format!("FIRM_{word}_X"),
        ] {
            env.insert(name, secret().into());
        }
    }
    env.insert("AWS_REGION".to_owned(), "eu-west-2".into());
    let plan = serde_json::json!({"claude-acp": {"provider": "bedrock", "env": env}});
    if section_only {
        plan.to_string()
    } else {
        serde_json::json!({ "plans": plan }).to_string()
    }
}

/// Checks the plan kept only `AWS_REGION`, warned once per banned name, and quoted no value.
fn only_the_allowed_name(effective: &Effective, layer: &str) {
    let plan = effective.plan("claude-acp").expect(layer);
    assert_eq!(
        plan.env.keys().collect::<Vec<_>>(),
        ["AWS_REGION"],
        "{layer}"
    );
    let refused = effective
        .warnings()
        .iter()
        .filter(|warning| {
            matches!(&warning.kind, WarningKind::InvalidValue { key, .. }
                if key.starts_with("plans.claude-acp.env."))
        })
        .count();
    assert_eq!(refused, SECRET_WORDS.len() * 3, "{layer}");
    assert!(
        !format!("{:?}", effective.warnings()).contains(&secret()),
        "{layer}"
    );
}

#[test]
fn a_banned_variable_name_is_refused_at_every_layer() {
    // Defaults: no plan, so no variable at all.
    assert!(Settings::default().plans.is_empty());
    // The user's settings.json.
    let effective = Effective::load(&locations(), &user(&banned_env(false)), &[]);
    only_the_allowed_name(&effective, "settings.json");
    // A policy.json.
    let firm = policy("firm", banned_env(false));
    let effective = Effective::load(&locations(), &user("{}"), &[&firm]);
    only_the_allowed_name(&effective, "policy.json");
    assert!(effective.is_locked("plans.claude-acp"));
    // The registry: one REG_SZ value of JSON for the plan, its name in another case.
    let plan: serde_json::Value = serde_json::from_str(&banned_env(true)).unwrap();
    let values = registry::parse(vec![(
        "Plans.claude-acp".to_owned(),
        registry::RegValue::Sz(plan["claude-acp"].to_string()),
    )]);
    let reg = FakeSource {
        name: registry::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    let effective = Effective::load(&locations(), &user("{}"), &[&reg]);
    only_the_allowed_name(&effective, "registry");
    // Managed preferences: the `plans` dictionary.
    let env: Vec<(String, managed_prefs::PlistValue)> = plan["claude-acp"]["env"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, value)| {
            let text = value.as_str().unwrap().to_owned();
            (name.clone(), managed_prefs::PlistValue::String(text))
        })
        .collect();
    let dict = managed_prefs::PlistValue::Dict(vec![(
        "claude-acp".to_owned(),
        managed_prefs::PlistValue::Dict(vec![
            (
                "provider".to_owned(),
                managed_prefs::PlistValue::String("bedrock".to_owned()),
            ),
            ("env".to_owned(), managed_prefs::PlistValue::Dict(env)),
        ]),
    )]);
    let PolicyRead::Read(values) = managed_prefs::parse(vec![("plans".to_owned(), dict, true)])
    else {
        panic!("managed preferences read nothing");
    };
    let prefs = FakeSource {
        name: managed_prefs::SOURCE,
        read: move || PolicyRead::Read(values.clone()),
    };
    let effective = Effective::load(&locations(), &user("{}"), &[&prefs]);
    only_the_allowed_name(&effective, "managed preferences");
}

#[test]
fn a_policy_install_value_that_does_not_type_locks_it_unset() {
    let firm = policy(
        "firm",
        format!(
            r#"{{"install": {{"registry": "http://npm.firm.example", "bundle": "{}"}}}}"#,
            abs_json("firm/bundle")
        ),
    );
    let effective = Effective::load(&locations(), &user(&lawyers_choices()), &[&firm]);
    assert_eq!(effective.settings().install.registry, None);
    assert_eq!(
        effective.settings().install.bundle.as_deref(),
        Some(abs("firm/bundle").as_path())
    );
    assert!(effective.is_locked("install.registry"));
}
