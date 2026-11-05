//! The hook's env with no profile: only the key variable and the plan's variables, per kind.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, OnceLock, RwLock};

use clauseo_agents::keys::Keychain;
use clauseo_host::ProfileHook;
use clauseo_protocol::AgentId;
use clauseo_settings::{Plan, PlanProvider};

use super::SetupHook;
use crate::setup::keys::KeyCache;

const CLAUDE_KEY: &str = "fakeAnthropicKey0123";
const CODEX_KEY: &str = "fakeOpenAiKey0123456";

fn id(text: &str) -> AgentId {
    AgentId::new(text).unwrap()
}

fn env(pairs: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

/// Whether `actual` is `expected`, printing names only: values hold keys.
fn same(actual: &BTreeMap<OsString, OsString>, expected: &BTreeMap<OsString, OsString>) {
    assert!(
        actual == expected,
        "names {:?}, expected {:?}",
        actual.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
}

#[test]
fn the_env_holds_only_the_key_variable_plus_the_plan_variables() {
    let (claude, codex) = (id("claude-acp"), id("codex-acp"));
    let keychain = Keychain::memory();
    keychain.store(&claude, CLAUDE_KEY).unwrap();
    keychain.store(&codex, CODEX_KEY).unwrap();
    let slot = Arc::new(OnceLock::new());
    let plans = Arc::new(RwLock::new(BTreeMap::new()));
    let hook = SetupHook::new(Arc::clone(&slot), Arc::clone(&plans));
    // Keys not loaded yet, no plan: nothing at all.
    same(&hook.launch_env(&claude), &BTreeMap::new());
    let _set = slot.set(Arc::new(KeyCache::load_all(keychain, [&claude, &codex])));
    same(
        &hook.launch_env(&claude),
        &env(&[("ANTHROPIC_API_KEY", CLAUDE_KEY)]),
    );
    same(
        &hook.launch_env(&codex),
        &env(&[("CODEX_API_KEY", CODEX_KEY)]),
    );
    let plan = |provider| Plan {
        endpoint: Some("https://gateway.firm.example".to_owned()),
        provider,
        env: [("AWS_REGION".to_owned(), "eu-west-2".to_owned())].into(),
    };
    plans.write().unwrap().extend([
        (claude.clone(), plan(Some(PlanProvider::Bedrock))),
        (codex.clone(), plan(None)),
    ]);
    same(
        &hook.launch_env(&claude),
        &env(&[
            ("ANTHROPIC_API_KEY", CLAUDE_KEY),
            ("ANTHROPIC_BASE_URL", "https://gateway.firm.example"),
            ("AWS_REGION", "eu-west-2"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ]),
    );
    // A Codex gateway is later: the plan's variables and the key, no endpoint variable.
    same(
        &hook.launch_env(&codex),
        &env(&[("AWS_REGION", "eu-west-2"), ("CODEX_API_KEY", CODEX_KEY)]),
    );
    plans
        .write()
        .unwrap()
        .insert(claude.clone(), plan(Some(PlanProvider::Vertex)));
    assert_eq!(
        hook.launch_env(&claude)
            .get(&OsString::from("CLAUDE_CODE_USE_VERTEX")),
        Some(&OsString::from("1"))
    );
    // An agent of no kind gets nothing.
    same(&hook.launch_env(&id("gemini-acp")), &BTreeMap::new());
}
