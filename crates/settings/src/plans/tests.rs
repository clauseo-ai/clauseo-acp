use super::{
    PLAN_ENV_NAMES, PLAN_PROXY_NAMES, SECRET_WORDS, bundle, check_launch_env, check_plan,
    entry_is_clauseos, id_ok, launch_is_clauseos, plan, registry, secret_shaped, web_address_ok,
};
use crate::error::Error;
use crate::schema::{AgentLaunch, Plan, PlanProvider};
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
        "https://gw.example:65535",
        "https://[::1]:8443/v1",
        "https://xn--bcher-kva.example",
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
        // Not ASCII: a right-to-left override, a zero-width space, a host not in punycode.
        "https://gw.example\u{202e}moc.evil",
        "https://gw\u{200b}.example",
        "https://bücher.example",
        // A backslash anywhere.
        "https://\\b.example",
        "https://gw.example\\@evil.example",
        "https://gw.example/a\\b",
        // A port that is no u16, or no port.
        "https://gw.example:99999",
        "https://gw.example:",
        "https://gw.example:+1",
        "https://[::1]x",
        "https://[]:443",
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
        // Only a Node may run an adapter's script: not a shell, not a relative `node`.
        json!({"command": abs_str("bin/sh"), "args": [ours]}),
        json!({"command": abs_str("usr/bin/env"), "args": [ours]}),
        json!({"command": "node", "args": [ours]}),
        json!({"command": "data/agents/a"}),
        json!({"args": [ours]}),
        json!("not an entry"),
    ] {
        assert!(!entry_is_clauseos(&entry, &data), "{entry}");
    }
}

#[test]
fn a_launch_is_clauseos_by_the_same_rule_as_an_entry() {
    let data = abs("data");
    let ours = abs_str("data/agents/claude-acp/install/h/index.js");
    let launch = |command: String| AgentLaunch {
        command,
        args: vec![ours.clone()],
        env: BTreeMap::new(),
    };
    for node in ["usr/local/bin/node", "Program Files/nodejs/node.exe"] {
        assert!(launch_is_clauseos(&launch(abs_str(node)), &data), "{node}");
    }
    assert!(launch_is_clauseos(
        &launch(abs_str("data/agents/runtime/node/bin/node")),
        &data
    ));
    for command in ["bin/sh", "usr/bin/python3", "usr/local/bin/nodejs"] {
        assert!(
            !launch_is_clauseos(&launch(abs_str(command)), &data),
            "{command}"
        );
    }
}

#[test]
fn a_launch_env_refuses_a_secret_shaped_name_with_a_value_and_quotes_nothing() {
    let launch = |name: &str, value: String| AgentLaunch {
        command: abs_str("data/agents/runtime/node/bin/node"),
        args: Vec::new(),
        env: BTreeMap::from([(name.to_owned(), value)]),
    };
    // The empty placeholder a stored key replaces at launch passes, and so does any other name.
    assert!(check_launch_env("claude-acp", &launch("ANTHROPIC_API_KEY", String::new())).is_ok());
    assert!(check_launch_env("claude-acp", &launch("CLAUDE_CONFIG_DIR", secret())).is_ok());
    for name in banned_names() {
        let error = check_launch_env("claude-acp", &launch(&name, secret())).unwrap_err();
        let Error::Refused { key, .. } = &error else {
            panic!("{name}: {error:?}");
        };
        assert_eq!(key, &format!("agents.claude-acp.env.{name}"));
        assert!(!format!("{error} {error:?}").contains(&secret()), "{name}");
    }
}

/// The text of a name that is no variable name, carrying a key; built at run time.
fn key_in_name() -> String {
    format!("ANTHROPIC_API_KEY={}", secret())
}

#[test]
fn a_name_that_is_no_variable_name_is_given_by_its_place_at_load() {
    let mut dropped = Vec::new();
    let value =
        json!({"env": {"AWS_PROFILE": "firm", "AWS_REGION": "eu-west-2", key_in_name(): "x"}});
    let typed = plan("plans.claude-acp", value, &mut dropped).unwrap();
    assert_eq!(typed.env.len(), 2);
    // In name order the bad name is the first: `AN` sorts before `AW`.
    let [WarningKind::InvalidValue { key, .. }] = dropped.as_slice() else {
        panic!("{dropped:?}");
    };
    assert_eq!(key, "plans.claude-acp.env[0]");
    let shown = format!("{dropped:?}");
    assert!(!shown.contains("sk-ant"), "{shown}");
}

#[test]
fn a_name_that_is_no_variable_name_is_given_by_its_place_at_set_plan() {
    let plan = Plan {
        env: BTreeMap::from([
            ("AWS_REGION".to_owned(), "eu-west-2".to_owned()),
            (key_in_name(), String::new()),
        ]),
        ..Plan::default()
    };
    let error = check_plan("claude-acp", &plan).unwrap_err();
    let Error::Refused { key, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(key, "plans.claude-acp.env[0]");
    assert!(!format!("{error} {error:?}").contains("sk-ant"));
}

#[test]
fn only_the_names_a_plan_supports_pass() {
    let mut env: BTreeMap<String, String> = PLAN_ENV_NAMES
        .iter()
        .map(|name| ((*name).to_owned(), "v".to_owned()))
        .collect();
    for proxy in PLAN_PROXY_NAMES {
        env.insert(
            proxy.to_owned(),
            "http://proxy.firm.example:3128".to_owned(),
        );
        env.insert(proxy.to_ascii_lowercase(), "proxy.firm.example".to_owned());
    }
    let allowed = Plan {
        env: env.clone(),
        ..Plan::default()
    };
    assert!(check_plan("claude-acp", &allowed).is_ok());
    let mut dropped = Vec::new();
    let typed = plan("plans.claude-acp", json!({"env": env}), &mut dropped).unwrap();
    assert_eq!(typed.env, env);
    assert!(dropped.is_empty(), "{dropped:?}");
}

/// Checks `name` set to `value` is refused at load and at `set_plan`, named, value unquoted.
fn refused_in_a_plan(name: &str, value: &str) {
    let mut dropped = Vec::new();
    let typed = plan(
        "plans.claude-acp",
        json!({"env": {name: value, "AWS_REGION": "eu-west-2"}}),
        &mut dropped,
    )
    .unwrap();
    assert_eq!(
        typed.env.keys().collect::<Vec<_>>(),
        ["AWS_REGION"],
        "{name}"
    );
    let [WarningKind::InvalidValue { key, .. }] = dropped.as_slice() else {
        panic!("{name}: {dropped:?}");
    };
    assert_eq!(key, &format!("plans.claude-acp.env.{name}"));
    assert!(!format!("{dropped:?}").contains(value), "{name}");
    let runtime = Plan {
        env: BTreeMap::from([(name.to_owned(), value.to_owned())]),
        ..Plan::default()
    };
    let error = check_plan("claude-acp", &runtime).unwrap_err();
    let Error::Refused { key, .. } = &error else {
        panic!("{name}: {error:?}");
    };
    assert_eq!(key, &format!("plans.claude-acp.env.{name}"));
    assert!(!format!("{error} {error:?}").contains(value), "{name}");
}

#[test]
fn a_credential_under_another_name_is_refused() {
    let header = format!("x-api-key: {}", secret());
    refused_in_a_plan("ANTHROPIC_CUSTOM_HEADERS", &header);
    for name in [
        "DB_PASSWD",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "FIRM_AUTH",
        "SSO_COOKIE",
    ] {
        refused_in_a_plan(name, &secret());
    }
}

#[test]
fn a_proxy_address_with_a_login_is_refused() {
    for name in ["HTTPS_PROXY", "http_proxy", "No_Proxy"] {
        refused_in_a_plan(
            name,
            &format!("http://user:{}@proxy.firm.example", secret()),
        );
        refused_in_a_plan(name, &format!("http://{}@proxy.firm.example", secret()));
    }
}

#[test]
fn a_variable_the_endpoint_or_provider_sets_is_refused_in_env() {
    for name in [
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_BEDROCK_BASE_URL",
        "ANTHROPIC_VERTEX_BASE_URL",
        "OPENAI_BASE_URL",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
    ] {
        refused_in_a_plan(name, "http://plain.example");
    }
}

#[test]
fn a_plan_id_is_an_agent_id() {
    for id in ["claude-acp", "codex-acp", "a", &"a".repeat(64)] {
        assert!(id_ok(id), "{id}");
    }
    for id in [
        "",
        "claude-acp.endpoint",
        "Claude-acp",
        "claude_acp",
        "é",
        &"a".repeat(65),
    ] {
        assert!(!id_ok(id), "{id}");
    }
}
