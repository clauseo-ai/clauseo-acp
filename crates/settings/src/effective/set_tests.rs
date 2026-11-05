//! `Effective::set_agent_launch` and `Effective::set_plan` (decision record 0048): each writes
//! its entry, refuses a policy-locked id with `Error::Locked` (`settings.locked` on the wire),
//! and `set_agent_launch` never replaces an entry that is not Clauseo's own.

use super::{Effective, EntryWrite};
use crate::error::Error;
use crate::locations::Locations;
use crate::plans::SECRET_WORDS;
use crate::policy::PolicyRead;
use crate::schema::{AgentLaunch, Plan, PlanProvider};
use crate::testing::{FakeSource, MemFiles, abs, abs_json, abs_str, policy};
use std::collections::BTreeMap;

/// The data folder these tests use, absolute on every OS.
fn locations() -> Locations {
    Locations::under_root(&abs("data"))
}

/// Memory files holding `settings.json` with `text`.
fn user(text: &str) -> MemFiles {
    MemFiles::with(locations().settings_file(), text)
}

/// The settings file's text.
fn saved(files: &MemFiles) -> Option<String> {
    files.text(locations().settings_file())
}

/// A value no message or file may hold; built at run time.
fn secret() -> String {
    format!("sk{}", "-ant-never-written-4567")
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

/// Clauseo's own Claude entry: the Node it downloaded running the adapter it installed.
fn ours() -> AgentLaunch {
    AgentLaunch {
        command: abs_str("data/agents/runtime/node/bin/node"),
        args: vec![abs_str("data/agents/claude-acp/install/h1/index.js")],
        env: BTreeMap::from([("ANTHROPIC_API_KEY".to_owned(), String::new())]),
    }
}

/// A firm gateway plan.
fn gateway() -> Plan {
    Plan {
        endpoint: Some("https://gateway.firm.example/anthropic".to_owned()),
        provider: Some(PlanProvider::Anthropic),
        env: BTreeMap::from([("ANTHROPIC_MODEL".to_owned(), "x".to_owned())]),
    }
}

#[test]
fn set_agent_launch_writes_the_entry_and_a_fresh_load_agrees() {
    let files = user(r#"{"ui": {"theme": "dark"}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    assert!(matches!(
        effective.set_agent_launch(&files, "claude-acp", &ours()),
        Ok(EntryWrite::Written)
    ));
    assert_eq!(effective.agent("claude-acp"), Some(&ours()));
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.agent("claude-acp"), Some(&ours()));
    assert_eq!(fresh.settings().ui.theme, crate::schema::Theme::Dark);
    // Clauseo's own entry is replaced on the next write: a new install, or a source change.
    let mut next = ours();
    next.args = vec![abs_str("data/agents/claude-acp/install/h2/index.js")];
    let mut effective = fresh;
    assert!(matches!(
        effective.set_agent_launch(&files, "claude-acp", &next),
        Ok(EntryWrite::Written)
    ));
    assert_eq!(
        Effective::load(&locations(), &files, &[]).agent("claude-acp"),
        Some(&next)
    );
}

#[test]
fn set_agent_launch_refuses_a_locked_id_with_settings_locked() {
    let files = user("{}");
    let firm = policy(
        "firm",
        format!(
            r#"{{"agents": {{"claude-acp": {{"command": "{}"}}}}}}"#,
            abs_json("firm/claude-acp")
        ),
    );
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    let before = effective.agent("claude-acp").cloned();
    assert!(matches!(
        effective.set_agent_launch(&files, "claude-acp", &ours()),
        Err(Error::Locked { key }) if key == "agents.claude-acp"
    ));
    assert_eq!(effective.agent("claude-acp").cloned(), before);
    assert_eq!(saved(&files).as_deref(), Some("{}"));
}

#[test]
fn set_agent_launch_never_replaces_an_entry_outside_the_agents_folder() {
    // The lawyer's own adapter, their Node running their script, and an entry that does not
    // type (a relative command, left out at load): none is Clauseo's, so none is replaced.
    for entry in [
        format!(
            r#"{{"command": "{}"}}"#,
            abs_json("usr/local/bin/claude-acp")
        ),
        format!(
            r#"{{"command": "{}", "args": ["{}"]}}"#,
            abs_json("usr/local/bin/node"),
            abs_json("home/lawyer/claude-acp/index.js")
        ),
        r#"{"command": "claude-acp"}"#.to_owned(),
    ] {
        let text = format!(r#"{{"agents": {{"claude-acp": {entry}}}}}"#);
        let files = user(&text);
        let mut effective = Effective::load(&locations(), &files, &[]);
        let before = effective.agent("claude-acp").cloned();
        assert!(
            matches!(
                effective.set_agent_launch(&files, "claude-acp", &ours()),
                Ok(EntryWrite::LeftAlone)
            ),
            "{entry}"
        );
        assert_eq!(effective.agent("claude-acp").cloned(), before, "{entry}");
        assert_eq!(saved(&files), Some(text), "{entry}");
    }
}

#[test]
fn set_agent_launch_refuses_a_launch_that_is_not_clauseos_install() {
    let files = MemFiles::default();
    let mut effective = Effective::load(&locations(), &files, &[]);
    for launch in [
        AgentLaunch {
            command: abs_str("usr/bin/elsewhere"),
            args: Vec::new(),
            env: BTreeMap::new(),
        },
        AgentLaunch {
            command: "node".to_owned(),
            args: vec![abs_str("data/agents/claude-acp/install/h/index.js")],
            env: BTreeMap::new(),
        },
        // A shell running an adapter's script is not Clauseo's install: only a Node is.
        AgentLaunch {
            command: abs_str("bin/sh"),
            args: vec![abs_str("data/agents/claude-acp/install/h/index.js")],
            env: BTreeMap::new(),
        },
    ] {
        assert!(matches!(
            effective.set_agent_launch(&files, "claude-acp", &launch),
            Err(Error::Refused { .. })
        ));
    }
    assert_eq!(effective.agent("claude-acp"), None);
    assert_eq!(saved(&files), None);
}

#[test]
fn set_plan_writes_and_removes_the_plan() {
    let files = user(r#"{"plans": {"codex-acp": {"provider": "anthropic"}}}"#);
    let mut effective = Effective::load(&locations(), &files, &[]);
    effective
        .set_plan(&files, "claude-acp", Some(&gateway()))
        .unwrap();
    assert_eq!(effective.plan("claude-acp"), Some(&gateway()));
    let fresh = Effective::load(&locations(), &files, &[]);
    assert_eq!(fresh.plan("claude-acp"), Some(&gateway()));
    assert!(fresh.plan("codex-acp").is_some());
    let mut effective = fresh;
    effective.set_plan(&files, "claude-acp", None).unwrap();
    assert_eq!(effective.plan("claude-acp"), None);
    // An empty plan is no plan.
    effective
        .set_plan(&files, "codex-acp", Some(&Plan::default()))
        .unwrap();
    let fresh = Effective::load(&locations(), &files, &[]);
    assert!(fresh.settings().plans.is_empty());
    assert!(fresh.warnings().is_empty(), "{:?}", fresh.warnings());
}

#[test]
fn set_plan_refuses_a_locked_id_with_settings_locked() {
    let files = user("{}");
    let firm = policy(
        "firm",
        r#"{"plans": {"claude-acp": {"provider": "bedrock", "env": {"AWS_REGION": "eu-west-2"}}}}"#,
    );
    let mut effective = Effective::load(&locations(), &files, &[&firm]);
    assert!(effective.is_locked("plans.claude-acp"));
    assert!(!effective.is_locked("plans.codex-acp"));
    assert!(matches!(
        effective.set_plan(&files, "claude-acp", Some(&gateway())),
        Err(Error::Locked { key }) if key == "plans.claude-acp"
    ));
    assert!(matches!(
        effective.set_plan(&files, "claude-acp", None),
        Err(Error::Locked { .. })
    ));
    assert_eq!(
        effective.plan("claude-acp").and_then(|plan| plan.provider),
        Some(PlanProvider::Bedrock)
    );
    assert_eq!(saved(&files).as_deref(), Some("{}"));
    effective
        .set_plan(&files, "codex-acp", Some(&gateway()))
        .unwrap();
}

#[test]
fn set_plan_refuses_each_banned_word_and_writes_nothing() {
    let files = user("{}");
    let mut effective = Effective::load(&locations(), &files, &[]);
    for word in SECRET_WORDS {
        for name in [
            word.to_owned(),
            format!("firm_{}", word.to_ascii_lowercase()),
        ] {
            let plan = Plan {
                env: BTreeMap::from([(name.clone(), secret())]),
                ..gateway()
            };
            let error = effective
                .set_plan(&files, "claude-acp", Some(&plan))
                .unwrap_err();
            assert!(matches!(error, Error::Refused { .. }), "{name}");
            assert!(!format!("{error} {error:?}").contains(&secret()), "{name}");
        }
    }
    assert_eq!(effective.plan("claude-acp"), None);
    assert_eq!(saved(&files).as_deref(), Some("{}"));
}

#[test]
fn with_policy_unreadable_no_plan_can_be_set() {
    let files = user("{}");
    let mut effective = Effective::load(&locations(), &files, &[&unreadable()]);
    assert!(effective.is_locked("plans.claude-acp"));
    assert!(matches!(
        effective.set_plan(&files, "claude-acp", Some(&gateway())),
        Err(Error::Locked { key }) if key == "plans.claude-acp"
    ));
    // The agents section is no security key: an install still becomes launchable.
    assert!(matches!(
        effective.set_agent_launch(&files, "claude-acp", &ours()),
        Ok(EntryWrite::Written)
    ));
}

#[test]
fn a_failed_write_changes_nothing_in_memory() {
    let files = MemFiles {
        fail_writes: true,
        ..MemFiles::default()
    };
    let mut effective = Effective::load(&locations(), &files, &[]);
    assert!(matches!(
        effective.set_agent_launch(&files, "claude-acp", &ours()),
        Err(Error::Io { .. })
    ));
    assert!(matches!(
        effective.set_plan(&files, "claude-acp", Some(&gateway())),
        Err(Error::Io { .. })
    ));
    assert_eq!(effective.agent("claude-acp"), None);
    assert_eq!(effective.plan("claude-acp"), None);
}

#[test]
fn set_agent_launch_refuses_a_key_in_its_env_and_writes_nothing() {
    let files = user("{}");
    let mut effective = Effective::load(&locations(), &files, &[]);
    let mut launch = ours();
    launch.env.insert("ANTHROPIC_API_KEY".to_owned(), secret());
    let error = effective
        .set_agent_launch(&files, "claude-acp", &launch)
        .unwrap_err();
    let key = "agents.claude-acp.env.ANTHROPIC_API_KEY";
    assert!(
        matches!(&error, Error::Refused { key: refused, .. } if refused == key),
        "{error:?}"
    );
    assert!(!format!("{error} {error:?}").contains(&secret()));
    assert_eq!(effective.agent("claude-acp"), None);
    assert_eq!(saved(&files).as_deref(), Some("{}"));
}

#[test]
fn set_plan_gives_a_name_that_is_no_variable_name_by_its_place() {
    let files = user("{}");
    let mut effective = Effective::load(&locations(), &files, &[]);
    let plan = Plan {
        env: BTreeMap::from([(format!("ANTHROPIC_API_KEY={}", secret()), String::new())]),
        ..gateway()
    };
    let error = effective
        .set_plan(&files, "claude-acp", Some(&plan))
        .unwrap_err();
    assert!(
        !format!("{error} {error:?}").contains("sk-ant"),
        "{error:?}"
    );
    // `ANTHROPIC_API_KEY=…` sorts before `ANTHROPIC_MODEL`.
    assert!(matches!(&error, Error::Refused { key, .. } if key == "plans.claude-acp.env[0]"));
    // Nor is a dotted id a plan's.
    assert!(matches!(
        effective.set_plan(&files, "claude-acp.endpoint", Some(&gateway())),
        Err(Error::Refused { .. })
    ));
    assert_eq!(saved(&files).as_deref(), Some("{}"));
}
