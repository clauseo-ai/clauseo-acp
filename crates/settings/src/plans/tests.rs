use super::{
    SECRET_WORDS, bundle, check_plan, entry_is_clauseos, plan, registry, secret_shaped,
    web_address_ok,
};
use crate::error::Error;
use crate::schema::{Plan, PlanProvider};
use crate::testing::{abs, abs_str};
use crate::warning::WarningKind;
use serde_json::json;
use std::collections::BTreeMap;

/// A value no message may quote; built at run time, so no file holds a key-shaped text.
fn secret() -> String {
    format!("sk{}", "-ant-never-shown-0123")
}

/// Every spelling of each banned word a test tries: as it is, lower case, mixed, and inside a
/// longer name.
fn banned_names() -> Vec<String> {
    SECRET_WORDS
        .iter()
        .flat_map(|word| {
            let lower = word.to_ascii_lowercase();
            let mixed: String = word
                .chars()
                .enumerate()
                .map(|(at, c)| {
                    if at % 2 == 0 {
                        c
                    } else {
                        c.to_ascii_lowercase()
                    }
                })
                .collect();
            [
                (*word).to_owned(),
                lower.clone(),
                mixed,
                format!("ANTHROPIC_{word}"),
                format!("my_{lower}_here"),
            ]
        })
        .collect()
}

#[test]
fn each_banned_word_is_secret_shaped_in_any_case() {
    for name in banned_names() {
        assert!(secret_shaped(&name), "{name}");
    }
    for name in [
        "AWS_REGION",
        "HTTPS_PROXY",
        "CLAUDE_CODE_USE_BEDROCK",
        "ANTHROPIC_MODEL",
    ] {
        assert!(!secret_shaped(name), "{name}");
    }
}

#[test]
fn a_plan_leaves_out_each_banned_name_and_keeps_the_rest() {
    for name in banned_names() {
        let mut dropped = Vec::new();
        let value = json!({
            "endpoint": "https://gateway.firm.example/v1",
            "env": {name.clone(): secret(), "AWS_REGION": "eu-west-2"},
        });
        let typed = plan("plans.claude-acp", value, &mut dropped).unwrap();
        assert_eq!(
            typed.env,
            BTreeMap::from([("AWS_REGION".to_owned(), "eu-west-2".to_owned())]),
            "{name}"
        );
        assert_eq!(
            typed.endpoint.as_deref(),
            Some("https://gateway.firm.example/v1")
        );
        let [WarningKind::InvalidValue { key, message }] = dropped.as_slice() else {
            panic!("{name}: {dropped:?}");
        };
        assert_eq!(key, &format!("plans.claude-acp.env.{name}"));
        assert!(!message.contains(&secret()), "{name}");
    }
}

#[test]
fn a_name_that_is_no_variable_name_is_left_out() {
    for name in ["", "1ST", "A=B", "A B", "É", "A-B"] {
        let mut dropped = Vec::new();
        let typed = plan("plans.x", json!({"env": {name: "v"}}), &mut dropped).unwrap();
        assert!(typed.env.is_empty(), "{name}");
        assert_eq!(dropped.len(), 1, "{name}");
    }
}

#[test]
fn web_addresses_are_https_with_a_host_and_nothing_secret_shaped() {
    for url in [
        "https://gateway.firm.example",
        "https://gateway.firm.example/v1/",
        "HTTPS://gw.example:8443/anthropic",
    ] {
        assert!(web_address_ok(url), "{url}");
    }
    for url in [
        "http://gateway.firm.example",
        "https://",
        "https:///path",
        "https://:443",
        "https://user:pass@gw.example",
        "https://gw.example/?key=1",
        "https://gw.example/#frag",
        "https://gw .example",
        "ftp://gw.example",
        "gw.example",
        "",
    ] {
        assert!(!web_address_ok(url), "{url}");
    }
}

#[test]
fn a_bad_endpoint_or_shape_refuses_the_plan_and_quotes_nothing() {
    let leaked = format!("https://me:{}@gw.example", secret());
    let mut dropped = Vec::new();
    let refused = plan("plans.x", json!({"endpoint": leaked}), &mut dropped).unwrap_err();
    assert!(!format!("{refused:?}").contains(&secret()));
    // serde's own message would quote the unknown variant; the plan's never does.
    let refused = plan("plans.x", json!({"provider": secret()}), &mut dropped).unwrap_err();
    assert!(!format!("{refused:?}").contains(&secret()));
    let refused = plan("plans.x", json!({"env": {"A": [secret()]}}), &mut dropped).unwrap_err();
    assert!(!format!("{refused:?}").contains(&secret()));
    assert!(plan("plans.x", json!("https://gw.example"), &mut dropped).is_err());
}

#[test]
fn a_plan_types_its_fields_and_warns_on_one_it_does_not_know() {
    let mut dropped = Vec::new();
    let typed = plan(
        "plans.claude-acp",
        json!({"provider": "bedrock", "env": {"AWS_REGION": "eu-west-2"}, "model": "x"}),
        &mut dropped,
    )
    .unwrap();
    assert_eq!(typed.provider, Some(PlanProvider::Bedrock));
    assert_eq!(typed.endpoint, None);
    assert_eq!(
        dropped,
        [WarningKind::UnknownKey {
            key: "plans.claude-acp.model".to_owned()
        }]
    );
}

#[test]
fn a_runtime_plan_with_a_banned_name_is_refused_whole_and_quotes_nothing() {
    for name in banned_names() {
        let plan = Plan {
            env: BTreeMap::from([(name.clone(), secret())]),
            ..Plan::default()
        };
        let Err(error @ Error::Refused { .. }) = check_plan("claude-acp", &plan) else {
            panic!("{name} was not refused");
        };
        assert!(
            error
                .to_string()
                .contains(&format!("plans.claude-acp.env.{name}"))
        );
        assert!(!error.to_string().contains(&secret()), "{name}");
        assert!(!format!("{error:?}").contains(&secret()), "{name}");
    }
    let bad = Plan {
        endpoint: Some(format!("https://gw.example/?k={}", secret())),
        ..Plan::default()
    };
    let error = check_plan("claude-acp", &bad).unwrap_err();
    assert!(!format!("{error} {error:?}").contains(&secret()));
}

#[test]
fn install_keys_are_an_absolute_bundle_and_an_https_mirror() {
    assert_eq!(
        bundle("install.bundle", json!(abs_str("b"))),
        Ok(Some(abs("b")))
    );
    assert_eq!(bundle("install.bundle", json!(null)), Ok(None));
    for value in [json!("relative"), json!(1), json!({})] {
        assert!(bundle("install.bundle", value.clone()).is_err(), "{value}");
    }
    assert_eq!(
        registry("install.registry", json!("https://npm.firm.example/")),
        Ok(Some("https://npm.firm.example/".to_owned()))
    );
    assert_eq!(registry("install.registry", json!(null)), Ok(None));
    for value in [json!("http://npm.firm.example"), json!(true)] {
        assert!(
            registry("install.registry", value.clone()).is_err(),
            "{value}"
        );
    }
}

#[test]
fn an_entry_is_clauseos_when_its_command_or_script_is_under_the_agents_folder() {
    let data = abs("data");
    let ours = abs_str("data/agents/claude-acp/install/h/index.js");
    let node = abs_str("data/agents/runtime/node/bin/node");
    let theirs = abs_str("usr/local/bin/claude-acp");
    let users_node = abs_str("usr/local/bin/node");
    for entry in [
        json!({"command": node, "args": [ours]}),
        json!({"command": users_node, "args": [ours]}),
        json!({"command": node}),
    ] {
        assert!(entry_is_clauseos(&entry, &data), "{entry}");
    }
    let dotted = format!("{}/../../usr/bin/x", abs_str("data/agents"));
    for entry in [
        json!({"command": theirs}),
        json!({"command": users_node, "args": [theirs]}),
        json!({"command": abs_str("data/agents")}),
        json!({"command": abs_str("data/agentsx/a")}),
        json!({"command": dotted}),
        json!({"command": "data/agents/a"}),
        json!({"args": [ours]}),
        json!("not an entry"),
    ] {
        assert!(!entry_is_clauseos(&entry, &data), "{entry}");
    }
}
