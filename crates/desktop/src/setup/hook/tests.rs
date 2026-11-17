//! The hook's env with no profile: only the key variable and the plan's variables, per kind;
//! and its session `PATH` folders: the Python slot's, for Claude and Codex only; and Codex's PDF
//! line, only with the PDF tools on its `PATH`; and a firm plan's endpoint and provider left out
//! while the agent's key read has failed.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use clauseo_agents::keys::Keychain;
use clauseo_host::ProfileHook;
use clauseo_process::AsyncIoTimer;
use clauseo_protocol::{AgentId, Secret};
use clauseo_settings::{Plan, PlanProvider};
use zeroize::Zeroizing;

use super::SetupHook;
use crate::setup::KeyWrite;
use crate::setup::keys::{KEYS_LOAD_BOUND, KeyCache, KeyMarker, KeyNotes};
use crate::setup::python_path::PythonSlot;
use crate::test_support::{Scratch, block_on};

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

/// Whether `actual` is `expected` beside `NODE_USE_SYSTEM_CA=1`, which every launch gets (S17),
/// printing names only: values hold keys.
fn same(actual: &BTreeMap<OsString, OsString>, expected: &BTreeMap<OsString, OsString>) {
    let mut actual = actual.clone();
    assert_eq!(
        actual.remove(&OsString::from("NODE_USE_SYSTEM_CA")),
        Some(OsString::from("1"))
    );
    assert!(
        &actual == expected,
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
    let hook = SetupHook::new(Arc::clone(&slot), Arc::clone(&plans), Arc::default());
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
    assert!(hook.launch_env(&id("gemini-acp")).is_empty());
}

#[test]
fn the_python_folders_go_to_claude_and_codex_only_as_the_slot_holds_them() {
    let python = PythonSlot::default();
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), Arc::default())
        .with_python(Arc::clone(&python));
    let agents = [id("claude-acp"), id("codex-acp"), id("gemini-acp")];
    // Python not Ready: an empty slot, no folder for anyone.
    for agent in &agents {
        assert_eq!(hook.agent_path(agent), Vec::<PathBuf>::new(), "{agent}");
    }
    let dirs = vec![
        PathBuf::from("/data/agents/runtime/clauseo-python/clauseo-bin"),
        PathBuf::from("/data/agents/runtime/clauseo-python"),
    ];
    python.write().unwrap().path.clone_from(&dirs);
    assert_eq!(hook.agent_path(&agents[0]), dirs);
    assert_eq!(hook.agent_path(&agents[1]), dirs);
    assert_eq!(hook.agent_path(&agents[2]), Vec::<PathBuf>::new());
    // Emptied again (any state but Ready): none.
    python.write().unwrap().path.clear();
    assert_eq!(hook.agent_path(&agents[0]), Vec::<PathBuf>::new());
    // A hook built with no slot gives none.
    let bare = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), Arc::default());
    assert_eq!(bare.agent_path(&agents[0]), Vec::<PathBuf>::new());
}

/// Claude and Codex get `TESSDATA_PREFIX`, the runtime's tesseract language folder, while the
/// slot holds one (the runtime ready with its folder there); no other agent gets it, and none
/// gets it while the runtime is missing. A value the plan sets stays.
#[test]
fn the_tessdata_prefix_goes_to_claude_and_codex_while_the_runtime_has_its_folder() {
    let python = PythonSlot::default();
    let claude = id("claude-acp");
    let plan = Plan {
        env: [("TESSDATA_PREFIX".to_owned(), "/firm/tessdata".to_owned())].into(),
        ..Plan::default()
    };
    let plans = Arc::new(RwLock::new(BTreeMap::from([(claude.clone(), plan)])));
    let hook = SetupHook::new(
        Arc::new(OnceLock::new()),
        Arc::clone(&plans),
        Arc::default(),
    )
    .with_python(Arc::clone(&python));
    let name = OsString::from("TESSDATA_PREFIX");
    let tessdata = PathBuf::from("/data/agents/runtime/clauseo-python/bin/tessdata");
    // The runtime missing: no variable, and the firm plan's own value only.
    assert_eq!(
        hook.launch_env(&claude).get(&name),
        Some(&OsString::from("/firm/tessdata"))
    );
    plans.write().unwrap().clear();
    assert_eq!(hook.launch_env(&claude).get(&name), None);
    assert_eq!(hook.launch_env(&id("codex-acp")).get(&name), None);
    python.write().unwrap().tessdata = Some(tessdata.clone());
    assert_eq!(
        hook.launch_env(&id("codex-acp")).get(&name),
        Some(&tessdata.clone().into_os_string())
    );
    assert_eq!(
        hook.launch_env(&claude).get(&name),
        Some(&tessdata.into_os_string())
    );
    assert_eq!(hook.launch_env(&id("gemini-acp")).get(&name), None);
    // Back to no runtime: no variable again.
    python.write().unwrap().tessdata = None;
    assert_eq!(hook.launch_env(&id("codex-acp")).get(&name), None);
}

/// The bundled pdftoppm and pdfinfo run the Python runtime, so their folder goes on Claude's and
/// Codex's PATH after the runtime's folders, and only while those are there.
#[test]
fn the_pdf_tools_folder_follows_the_python_folders_for_claude_and_codex() {
    let python = PythonSlot::default();
    let pdf = PathBuf::from("/Applications/Clauseo.app/Contents/Helpers/pdf");
    let hook = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), Arc::default())
        .with_python(Arc::clone(&python))
        .with_pdf_tools(Some(pdf.clone()));
    let agents = [id("claude-acp"), id("codex-acp"), id("gemini-acp")];
    for agent in &agents {
        assert_eq!(hook.agent_path(agent), Vec::<PathBuf>::new(), "{agent}");
    }
    let bin = PathBuf::from("/data/agents/runtime/clauseo-python/bin");
    python.write().unwrap().path.push(bin.clone());
    assert_eq!(hook.agent_path(&agents[0]), [bin.clone(), pdf.clone()]);
    assert_eq!(hook.agent_path(&agents[1]), [bin.clone(), pdf]);
    assert_eq!(hook.agent_path(&agents[2]), Vec::<PathBuf>::new());
    python.write().unwrap().path.clear();
    assert_eq!(hook.agent_path(&agents[1]), Vec::<PathBuf>::new());
    // Not found (a debug build, a bundle without it): the Python folders alone.
    let without = SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), Arc::default())
        .with_python(Arc::clone(&python))
        .with_pdf_tools(None);
    python.write().unwrap().path.push(bin.clone());
    assert_eq!(without.agent_path(&agents[0]), [bin]);
}

/// A launch profile whose Codex instructions are `Base.`, and whose env is empty.
struct Base;

impl ProfileHook for Base {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<serde_json::Map<String, serde_json::Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }

    fn codex_config(
        &self,
        _agent: &AgentId,
        _written: Option<&clauseo_profile::SkillsWritten>,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> serde_json::Map<String, serde_json::Value> {
        serde_json::json!({ "developer_instructions": "Base." })
            .as_object()
            .unwrap()
            .clone()
    }
}

/// Codex is told to render PDF pages with pdftoppm only where the tools are on its PATH: the
/// Python runtime ready and the PDF tools' folder found; and no launch carries a variable that
/// points the tools at another interpreter (they find it on PATH only).
#[test]
fn codex_gets_the_pdf_line_only_with_the_pdf_tools_on_its_path() {
    let line = clauseo_profile::instructions::codex_pdf();
    let instructions = |hook: &SetupHook| {
        let keys = hook.codex_config(&id("codex-acp"), None, None);
        keys["developer_instructions"].as_str().unwrap().to_owned()
    };
    let python = PythonSlot::default();
    let hook = |pdf: Option<PathBuf>| {
        SetupHook::new(Arc::new(OnceLock::new()), Arc::default(), Arc::default())
            .with_python(Arc::clone(&python))
            .with_pdf_tools(pdf)
            .over(Box::new(Base))
    };
    let pdf = PathBuf::from("/Applications/Clauseo.app/Contents/Helpers/pdf");
    let (found, missing) = (hook(Some(pdf)), hook(None));
    // The runtime not ready: no line, found or not.
    assert_eq!(instructions(&found), "Base.");
    assert_eq!(instructions(&missing), "Base.");
    python
        .write()
        .unwrap()
        .path
        .push(PathBuf::from("/data/agents/runtime/clauseo-python/bin"));
    assert_eq!(instructions(&found), format!("Base.\n\n{line}"));
    assert_eq!(instructions(&missing), "Base.");
    assert!(
        found.codex_config(&id("claude-acp"), None, None).is_empty() || {
            let keys = found.codex_config(&id("claude-acp"), None, None);
            !keys["developer_instructions"]
                .as_str()
                .unwrap()
                .contains(line)
        }
    );
    for agent in [id("claude-acp"), id("codex-acp")] {
        let env = found.launch_env(&agent);
        assert!(
            env.keys()
                .all(|name| !name.to_string_lossy().contains("PDF")),
            "{agent}: {:?}",
            env.keys().collect::<Vec<_>>()
        );
    }
}

/// A key cache over a keychain that fails every read, with no "key stored" mark: Claude's read
/// is in the `failed` set (decision record 0048's M-d rule).
fn failed_read_cache(scratch: &Scratch, keychain: &Keychain, claude: &AgentId) -> KeyCache {
    keychain.fail_reads(true);
    let notes = KeyNotes::new();
    let cache = KeyCache::load_bounded(
        keychain.clone(),
        [claude],
        KeyMarker::new(scratch.path(), [claude]),
        notes.sender(),
        &AsyncIoTimer,
        KEYS_LOAD_BOUND,
    );
    keychain.fail_reads(false);
    assert!(cache.read_failed(claude) && !cache.unreadable(claude));
    cache
}

#[test]
fn a_firm_plan_arriving_while_a_key_read_has_failed_gives_the_launch_no_firm_url() {
    let scratch = Scratch::folder("hook-firm-failed-read", &[]);
    let claude = id("claude-acp");
    let keychain = Keychain::memory();
    let slot = Arc::new(OnceLock::new());
    let _set = slot.set(Arc::new(failed_read_cache(&scratch, &keychain, &claude)));
    let plans = Arc::new(RwLock::new(BTreeMap::new()));
    let hook = SetupHook::new(Arc::clone(&slot), Arc::clone(&plans), Arc::default());
    // No firm plan: the keyless launch the M-d rule allows.
    same(&hook.launch_env(&claude), &BTreeMap::new());
    // A settings change brings a firm plan; the host's refusal has not landed yet. The launch
    // gets the plan's own variables but never the endpoint or the provider.
    for provider in [
        None,
        Some(PlanProvider::Bedrock),
        Some(PlanProvider::Vertex),
    ] {
        let plan = Plan {
            endpoint: Some("https://gateway.firm.example".to_owned()),
            provider,
            env: [("AWS_REGION".to_owned(), "eu-west-2".to_owned())].into(),
        };
        plans.write().unwrap().insert(claude.clone(), plan);
        same(
            &hook.launch_env(&claude),
            &env(&[("AWS_REGION", "eu-west-2")]),
        );
    }
}

#[test]
fn a_key_save_ends_a_failed_read_and_the_firm_plan_comes_back_with_the_key() {
    let scratch = Scratch::folder("hook-failed-read-save", &[]);
    let claude = id("claude-acp");
    let keychain = Keychain::memory();
    let cache = Arc::new(failed_read_cache(&scratch, &keychain, &claude));
    let slot = Arc::new(OnceLock::new());
    let _set = slot.set(Arc::clone(&cache));
    let plan = Plan {
        endpoint: Some("https://gateway.firm.example".to_owned()),
        ..Plan::default()
    };
    let plans = Arc::new(RwLock::new(BTreeMap::from([(claude.clone(), plan)])));
    let hook = SetupHook::new(Arc::clone(&slot), Arc::clone(&plans), Arc::default());
    same(&hook.launch_env(&claude), &BTreeMap::new());
    let secret = Secret::new(Zeroizing::new(CLAUDE_KEY.to_owned())).unwrap();
    block_on(cache.set(&claude, secret, KeyWrite::Replace, |_| {})).unwrap();
    assert!(!cache.read_failed(&claude) && !cache.unreadable_for(&claude, true));
    same(
        &hook.launch_env(&claude),
        &env(&[
            ("ANTHROPIC_API_KEY", CLAUDE_KEY),
            ("ANTHROPIC_BASE_URL", "https://gateway.firm.example"),
        ]),
    );
}
