//! A question in Hindi about Hindi papers, through the host as the desktop runs a thread:
//! the fixture matter of `tests/fixtures/hindi/` (a Unicode PDF, a legacy-encoded PDF, a
//! scanned PDF and a `.docx`; its `README.md` says what each is), the question with the four
//! papers attached, and the answer judged by one rule: at least 60% of its letters
//! Devanagari, and the tenant's name or the deposit planted in two of the papers named.
//!
//! The fake-agent tests run in CI and test the harness itself. The two live tests are
//! ignored; they run on a Mac with the user's own agent logins:
//!
//! ```text
//! CLAUSEO_HINDI_LIVE_DATA=$HOME/clauseo-hindi-live \
//! cargo test -p clauseo-host --test host hindi::live -- --ignored --nocapture --test-threads 1
//! ```
//!
//! Each test finds its adapter the first way that works, and prints which: the executable
//! `CLAUSEO_HINDI_CLAUDE_ACP` or `CLAUSEO_HINDI_CODEX_ACP` names; else the pinned packages
//! in `$CLAUSEO_HINDI_LIVE_DATA/adapters` (an `npm ci` of `crates/limits/tests/live`), run
//! by the `node` that `CLAUSEO_HINDI_NODE` names; else it fails with the steps for either
//! (the app's own installer is not on main).
//!
//! `CLAUSEO_HINDI_LIVE_DATA` is an existing folder of the test's own (never the app's data
//! folder, which the test refuses); the host's data folder is `data/` in it and each run's
//! matter `matters/hindi-matter-<random>`. Codex keeps its login in
//! `data/agents/codex-acp/home`: on the first run the Codex test fails asking for a sign-in
//! there, once (`CODEX_HOME=<that home> codex login --device-auth`). Claude uses the user's
//! own settings (Vertex) through `HOME`.
//! `hindi::live::claude` alone runs only the Claude half. The models: Claude
//! `claude-haiku-5-5`, Codex `gpt-6-luna` at effort `low`; a test fails when the agent
//! reports any other.

#[cfg(test)]
#[path = "hindi/flow.rs"]
mod flow;
#[cfg(test)]
#[path = "hindi/live.rs"]
mod live_run;
#[cfg(test)]
#[path = "hindi/verdict.rs"]
mod verdict;

use std::time::Duration;

use flow::{Wanted, names};
use verdict::{AMOUNT, DATE, PAPERS, PLANTED_IN, TENANT, Verdict};

use crate::support::{Dir, Scenario};

/// Claude Haiku 5.5, never a 4.x model.
const CLAUDE: Wanted = Wanted {
    agent: "claude-acp",
    model: "claude-haiku-5-5",
    effort: None,
    patience: Duration::from_secs(20),
};

/// GPT-6 Luna at low effort.
const CODEX: Wanted = Wanted {
    agent: "codex-acp",
    model: "gpt-6-luna",
    effort: Some("low"),
    patience: Duration::from_secs(20),
};

/// Runs the flow against the local scenario `scenario` under `wanted.agent`'s id.
fn fake(test: &str, wanted: Wanted, scenario: &'static str) -> flow::Asked {
    let dir = Dir::new(test, &[(wanted.agent, Scenario::Local(scenario))]);
    let host = dir.start(1);
    let asked = flow::ask(&host.handle, dir.work(), wanted);
    host.shutdown();
    asked
}

#[test]
fn a_hindi_answer_naming_the_planted_fact_passes_through_the_host() {
    let asked = fake("hindi-claude", CLAUDE, "hindi-answer");
    assert!(asked.verdict.passed(), "{}", asked.record());
    assert!(
        asked.verdict.name && asked.verdict.amount,
        "{}",
        asked.record()
    );
    assert_eq!(
        asked.verdict.cited,
        PLANTED_IN.to_vec(),
        "{}",
        asked.record()
    );
    assert_eq!(asked.model.as_deref(), Some("claude-haiku-5-5 (Haiku 5.5)"));
    assert_eq!(asked.effort, None);
    assert_eq!(asked.read, vec!["kirayanama.pdf"]);
    assert_eq!(asked.permissions, 1);
}

#[test]
fn the_codex_flow_sets_low_effort_and_passes() {
    let asked = fake("hindi-codex", CODEX, "hindi-answer-codex");
    assert!(asked.verdict.passed(), "{}", asked.record());
    assert_eq!(asked.model.as_deref(), Some("gpt-6-luna (GPT-6 Luna)"));
    assert_eq!(asked.effort.as_deref(), Some("low (Low)"));
}

#[test]
fn an_english_answer_naming_the_fact_fails_the_devanagari_share() {
    let asked = fake("hindi-english", CLAUDE, "hindi-answer-english");
    assert!(
        asked.verdict.amount && asked.verdict.name,
        "{}",
        asked.record()
    );
    assert!(
        asked.verdict.ratio() < verdict::MIN_DEVANAGARI,
        "{}",
        asked.record()
    );
    assert!(!asked.verdict.passed(), "{}", asked.record());
}

#[test]
#[should_panic(expected = "offers no Model choice for claude-haiku-4-5")]
fn a_model_the_agent_does_not_offer_fails_the_run() {
    let wanted = Wanted {
        model: "claude-haiku-4-5",
        ..CLAUDE
    };
    fake("hindi-wrong-model", wanted, "hindi-answer-wrong-model");
}

#[test]
fn the_rule_counts_devanagari_letters_and_finds_the_fact_in_either_digits() {
    let hindi = Verdict::of("जमानत राशि ₹३,४७,२१५ थी, देखें patra.docx");
    assert!(hindi.amount && !hindi.name, "{hindi}");
    assert_eq!(hindi.cited, vec!["patra.docx"]);
    assert!(hindi.ratio() > 0.6 && hindi.passed(), "{hindi}");

    let named = Verdict::of(&format!("किरायेदार {TENANT} हैं।"));
    assert!(named.name && !named.amount && named.passed(), "{named}");

    // Hindi with no fact fails; so does an unrelated number that holds the digits.
    assert!(!Verdict::of("मुझे यह जानकारी नहीं मिली।").passed());
    assert!(!Verdict::of("राशि 13,47,2150 है").amount);

    let english = Verdict::of("The deposit was 3,47,215 and the lease says so.");
    assert!(
        english.amount && english.ratio() < 0.01 && !english.passed(),
        "{english}"
    );
    assert_eq!(Verdict::of("").ratio(), 0.0);
}

#[test]
fn model_names_match_loosely_and_never_another_version() {
    assert!(names("Claude Haiku 5.5", "claude-haiku-5-5"));
    assert!(names("GPT-6 Luna", "gpt-6-luna"));
    assert!(!names("claude-haiku-4-5 Haiku 4.5", "claude-haiku-5-5"));
}

#[test]
fn the_fixture_generator_plants_the_facts_the_check_looks_for() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hindi");
    let script = std::fs::read_to_string(folder.join("generate.py")).unwrap();
    for fact in [TENANT, AMOUNT, DATE] {
        assert!(
            script.contains(&format!("\"{fact}\"")),
            "generate.py lacks {fact}"
        );
    }
    let mut total = 0;
    for paper in PAPERS {
        let len = std::fs::metadata(folder.join(paper)).unwrap().len();
        assert!(len > 0, "{paper} is empty");
        total += len;
    }
    assert!(total < 1_000_000, "the fixture papers take {total} bytes");
}

/// The live tests (ignored): see this module's comment for the command.
mod live {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use super::flow::Wanted;
    use super::live_run;

    /// A live agent reads four papers and answers; five minutes per step is ample.
    const PATIENCE: Duration = Duration::from_secs(300);

    #[test]
    #[ignore = "live: Claude Haiku 5.5 through claude-acp; see the module comment"]
    fn claude() {
        let wanted = Wanted {
            patience: PATIENCE,
            ..super::CLAUDE
        };
        let env = BTreeMap::from([("ANTHROPIC_MODEL".to_owned(), wanted.model.to_owned())]);
        let adapter = live_run::Adapter {
            var: "CLAUSEO_HINDI_CLAUDE_ACP",
            script: "@agentclientprotocol/claude-agent-acp/dist/index.js",
        };
        live_run::run(wanted, adapter, &env);
    }

    #[test]
    #[ignore = "live: GPT-6 Luna at low effort through codex-acp; see the module comment"]
    fn codex() {
        let wanted = Wanted {
            patience: PATIENCE,
            ..super::CODEX
        };
        let config = serde_json::json!({
            "model": wanted.model,
            "model_reasoning_effort": wanted.effort,
        });
        let env = BTreeMap::from([("CODEX_CONFIG".to_owned(), config.to_string())]);
        let adapter = live_run::Adapter {
            var: "CLAUSEO_HINDI_CODEX_ACP",
            script: "@agentclientprotocol/codex-acp/dist/index.js",
        };
        live_run::run(wanted, adapter, &env);
    }
}
