//! Every flag settings object Clauseo sends Claude Code (`claudeCode.options.settings`, the
//! `--settings` source) fits the pinned SDK's settings schema, checked in, trimmed, at
//! `tests/fixtures/claude_settings/settings.schema.json` (its README names the pin,
//! claude-agent-sdk 0.3.293 with Claude Code 2.1.293, and how to make it again). The checker is
//! `check.rs` beside this file.
//!
//! The emitters covered, all through [`session_meta`]: `profile::claude_options` (with the
//! launch profile's own `_meta`, `clauseo_profile::claude::ClaudeOptions`), the data fence's and
//! the browser's rules, `sandbox::sandbox_settings`, forced ask's settings, `guard_entry`, and
//! `pinned::pin_security_keys`. `subagents` writes `options.agents`, not the settings.

mod check;

use std::path::Path;

use serde_json::{Map, Value, json};

use self::check::Schema;
use super::browser::Switch;
use super::profile::{ClaudeKeys, Folders, Login};
use super::sandbox::{ClaudeSandbox, SandboxOs};
use super::sandbox_tests::Session;
use super::session_canary::{
    CANARY_MATCHER, CANARY_TIMEOUT_SECONDS, CanaryHook, add_canary, canary_entry,
};
use super::{GuardHook, McpControl, session_meta};
use crate::agent::data_fence::LaunchDeny;
use crate::browser::seed_sites::SEED_SITES;

/// The trimmed schema.
const SCHEMA: &str = include_str!("../../../tests/fixtures/claude_settings/settings.schema.json");

/// The guard program's path, as text in the settings only.
const PROGRAM: &str = "/app/clauseo";

/// The settings canary's file and nonce (`session_canary`), as text in the settings only.
const CANARY_FILE: &str = "/data/run/s1/session-canary.0123456789abcdef0123456789abcdef";
const CANARY_NONCE: &str = "0123456789abcdef0123456789abcdef";

/// What else varies per case: the login, web search, the browser, case law, and whether the
/// launch profile's `_meta` comes first.
#[derive(Debug, Clone, Copy)]
struct Variant {
    login: Login,
    web_search: Switch,
    browser: bool,
    case_law: bool,
    profile: bool,
}

const VARIANTS: [Variant; 3] = [
    Variant {
        login: Login::FirstParty,
        web_search: Switch {
            on: true,
            locked: false,
        },
        browser: false,
        case_law: false,
        profile: true,
    },
    Variant {
        login: Login::Bedrock,
        web_search: Switch {
            on: false,
            locked: false,
        },
        browser: true,
        case_law: true,
        profile: true,
    },
    Variant {
        login: Login::OtherCloud,
        web_search: Switch {
            on: true,
            locked: false,
        },
        browser: true,
        case_law: false,
        profile: false,
    },
];

/// One launch's inputs beside the session's paths.
#[derive(Debug, Clone, Copy)]
struct Case {
    os: SandboxOs,
    control: McpControl,
    forced_ask: bool,
    guard: bool,
    variant: Variant,
}

fn schema_root() -> Value {
    serde_json::from_str(SCHEMA).unwrap()
}

/// The launch profile's own `_meta` for a Claude session.
fn profile_meta() -> Map<String, Value> {
    clauseo_profile::claude::ClaudeOptions::new("Instructions.".to_owned(), None).meta_fragment()
}

/// The flag settings `session_meta` gives `case` over `session`, if it gives any.
fn settings_for(session: &Session, case: Case) -> Option<Value> {
    let Ok(sandbox) = ClaudeSandbox::for_launch(case.os, &session.paths()) else {
        panic!("{case:?}: the ordinary session's launch was refused");
    };
    assert_eq!(sandbox.is_some(), case.os != SandboxOs::Windows, "{case:?}");
    let data = LaunchDeny::fixed(session.data.clone());
    let trusted = vec!["docs".to_owned()];
    let keys = ClaudeKeys {
        web_search: case.variant.web_search,
        login: case.variant.login,
        folders: Folders {
            work: session.work.to_str().unwrap(),
            notes: session.notes.to_str().unwrap(),
            connectors: "",
            browser: "",
        },
        seed_sites: SEED_SITES,
        forced_ask: case.forced_ask,
        data: &data,
        case_law: case.variant.case_law,
        browser: case.variant.browser,
        sandbox: sandbox.as_ref(),
        matter: Some(session.matter.as_path()),
        trusted_servers: &trusted,
    };
    let guard = case.guard.then_some(GuardHook {
        program: Path::new(PROGRAM),
        matter: session.matter.as_path(),
        ask_all: case.forced_ask,
    });
    let hook = case.variant.profile.then(profile_meta);
    let mut meta = session_meta(hook, case.control, guard, Some(&keys));
    if case.guard {
        // As `profile_loads::session_loads` adds it after `session_meta` and `fill_meta`.
        let canary = canary_entry(CanaryHook {
            program: Path::new(PROGRAM),
            file: Path::new(CANARY_FILE),
            nonce: CANARY_NONCE,
        });
        add_canary(&mut meta, canary.unwrap());
    }
    Value::Object(meta)
        .pointer("/claudeCode/options/settings")
        .cloned()
}

/// Every case: the OS's sandbox (macOS, Linux, none on Windows; each built on every OS, as
/// `SandboxOs` is a value, not a cfg) × who controls MCP × forced ask × the guard × the variants.
fn every_case() -> Vec<Case> {
    let mut cases = Vec::new();
    for os in [SandboxOs::Mac, SandboxOs::Linux, SandboxOs::Windows] {
        for control in [McpControl::Clauseo, McpControl::Managed] {
            for forced_ask in [false, true] {
                for guard in [false, true] {
                    for variant in VARIANTS {
                        cases.push(Case {
                            os,
                            control,
                            forced_ask,
                            guard,
                            variant,
                        });
                    }
                }
            }
        }
    }
    cases
}

/// The schema reads only keywords the checker knows, and every `$ref` names a definition.
#[test]
fn the_schema_uses_only_what_the_checker_reads() {
    let root = schema_root();
    Schema::new(&root).audit();
    let comment = root["$comment"].as_str().unwrap();
    for pin in [
        "claude-agent-sdk 0.3.293",
        "2.1.293",
        "claude-agent-acp 0.89.1",
    ] {
        assert!(comment.contains(pin), "{pin} not in the schema's $comment");
    }
}

/// Each Claude launch's flag settings fit the schema, in every case; the cases reach the guard's
/// hook entry and the settings canary's (`SessionStart`, added with the guard as production
/// does), the sandbox's lists, the data fence's rules and forced ask's rules.
#[test]
fn every_flag_settings_object_clauseo_sends_fits_the_sdks_schema() {
    let root = schema_root();
    let schema = Schema::new(&root);
    let session = Session::new("s-1");
    let cases = every_case();
    assert_eq!(cases.len(), 3 * 2 * 2 * 2 * VARIANTS.len());
    let mut failures = Vec::new();
    for case in cases {
        let settings = settings_for(&session, case).expect("a Claude launch sends settings");
        let errors = schema.errors(&settings);
        if !errors.is_empty() {
            failures.push(format!("{case:?}:\n  {}", errors.join("\n  ")));
        }
        let hook_type = settings.pointer("/hooks/PreToolUse/0/hooks/0/type");
        let command = case.guard.then(|| json!("command"));
        assert_eq!(hook_type, command.as_ref(), "{case:?}");
        let canary = settings.pointer("/hooks/SessionStart/0");
        let expected = case
            .guard
            .then(|| (json!(CANARY_MATCHER), json!(CANARY_TIMEOUT_SECONDS)));
        let found = canary.map(|entry| {
            (
                entry["matcher"].clone(),
                entry["hooks"][0]["timeout"].clone(),
            )
        });
        assert_eq!(found, expected, "{case:?}");
        let writes = settings.pointer("/sandbox/filesystem/allowWrite");
        assert_eq!(writes.is_some(), case.os != SandboxOs::Windows, "{case:?}");
        assert!(
            settings.pointer("/permissions/deny/0").is_some(),
            "{case:?}"
        );
        let asks = settings.pointer("/permissions/ask").is_some();
        assert_eq!(asks, case.forced_ask || case.variant.browser, "{case:?}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An agent with no kind gets Claude's keys only through the guard: its settings, when there are
/// any, fit too.
#[test]
fn a_kindless_agents_guard_settings_fit_the_schema() {
    let root = schema_root();
    let schema = Schema::new(&root);
    for control in [McpControl::Clauseo, McpControl::Managed] {
        for ask_all in [false, true] {
            let guard = GuardHook {
                program: Path::new(PROGRAM),
                matter: Path::new("/m"),
                ask_all,
            };
            let meta = session_meta(None, control, Some(guard), None);
            let settings = Value::Object(meta)
                .pointer("/claudeCode/options/settings")
                .cloned()
                .unwrap();
            assert_eq!(schema.errors(&settings), Vec::<String>::new(), "{ask_all}");
            let meta = session_meta(None, control, None, None);
            let none = Value::Object(meta)
                .pointer("/claudeCode/options/settings")
                .cloned();
            assert_eq!(none, None);
        }
    }
}

/// `settings` with `new` at the JSON pointer `pointer`: a key put in (or replaced) in an
/// object, an item replaced in an array.
fn with(mut settings: Value, pointer: &str, new: Value) -> Value {
    let (parent, last) = pointer.rsplit_once('/').unwrap();
    match settings.pointer_mut(parent).unwrap() {
        Value::Object(map) => {
            map.insert(last.to_owned(), new);
        }
        Value::Array(items) => {
            items[last.parse::<usize>().unwrap()] = new;
        }
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => {
            panic!("{pointer}: {other}")
        }
    }
    settings
}

/// The checker refuses what Claude Code would refuse or strip: a hook of an unknown type, a
/// timeout as text or at 0, an unknown key in a hook or a closed sandbox object, an unknown hook
/// event, a mode Claude Code lacks, a rule or variable of the wrong type. The top level,
/// `permissions` and `sandbox` stay open, as in the SDK.
#[test]
fn the_checker_refuses_a_broken_hook_or_key() {
    let root = schema_root();
    let schema = Schema::new(&root);
    let session = Session::new("s-1");
    let case = Case {
        os: SandboxOs::Mac,
        control: McpControl::Clauseo,
        forced_ask: false,
        guard: true,
        variant: VARIANTS[0],
    };
    let good = settings_for(&session, case).unwrap();
    assert_eq!(schema.errors(&good), Vec::<String>::new());
    let hook = "/hooks/PreToolUse/0/hooks/0";
    let canary = "/hooks/SessionStart/0/hooks/0";
    let broken = [
        (format!("{canary}/timeout"), json!("2")),
        (format!("{canary}/args/3"), json!(12_345)),
        ("/hooks/SessionStart/0/matcher".to_owned(), json!(null)),
        (format!("{hook}/type"), json!("script")),
        (format!("{hook}/timeout"), json!("10")),
        (format!("{hook}/timeout"), json!(0)),
        (format!("{hook}/args"), json!([1])),
        (format!("{hook}/shellCommand"), json!("x")),
        ("/hooks/PreToolUse/0/matcher".to_owned(), json!(["Edit"])),
        ("/hooks/PreToolUse/0/hooks".to_owned(), json!({})),
        ("/hooks/PreToolUse/0/when".to_owned(), json!("always")),
        ("/hooks/PreToolUs".to_owned(), json!([])),
        ("/permissions/defaultMode".to_owned(), json!("bypass")),
        ("/permissions/deny/0".to_owned(), json!({"rule": "Read"})),
        ("/sandbox/network/allowAll".to_owned(), json!(true)),
        ("/sandbox/filesystem/allowWrite".to_owned(), json!("/m")),
        ("/sandbox/enabled".to_owned(), json!("true")),
        (
            "/env/CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR".to_owned(),
            json!(1),
        ),
        ("/disableAllHooks".to_owned(), json!("false")),
    ];
    for (pointer, value) in broken {
        let settings = with(good.clone(), &pointer, value.clone());
        assert!(
            !schema.errors(&settings).is_empty(),
            "{pointer} = {value} passed"
        );
    }
    let mut hookless = good.clone();
    hookless
        .pointer_mut(hook)
        .and_then(Value::as_object_mut)
        .unwrap()
        .remove("command");
    assert!(
        !schema.errors(&hookless).is_empty(),
        "a hook without command"
    );
    for (pointer, value) in [
        ("/someLaterKey", json!(1)),
        ("/permissions/someLaterKey", json!(1)),
        ("/sandbox/someLaterKey", json!(1)),
    ] {
        let settings = with(good.clone(), pointer, value);
        assert_eq!(schema.errors(&settings), Vec::<String>::new(), "{pointer}");
    }
}
