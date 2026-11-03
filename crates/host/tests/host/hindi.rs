//! A question in Hindi about Hindi papers, through the host as the desktop runs a thread:
//! the fixture matter of `tests/fixtures/hindi/` (a Unicode PDF, a legacy-encoded PDF, a
//! scanned PDF and a `.docx`; its `README.md` says what each is), the question with the four
//! papers attached, and the answer judged by one rule: at least 60% of its letters
//! Devanagari, and the tenant's name or the deposit planted in two of the papers named.
//!
//! The fake-agent tests run in CI and test the harness itself. The two live tests are
//! ignored; they run on a Mac with the user's own agent logins, from one command line:
//!
//! ```text
//! CLAUSEO_HINDI_LIVE_DATA=$HOME/clauseo-hindi-live \
//! CLAUSEO_HINDI_NODE=$(command -v node) \
//! cargo test -p clauseo-host --test host hindi::live -- --ignored --nocapture --test-threads 1
//! ```
//!
//! `CLAUSEO_HINDI_LIVE_DATA` names a folder of the test's own, never the app's data folder
//! (the test refuses one that overlaps it). The test creates it when it is absent. The host's
//! data folder is `data/` in it, and each run's matter `matters/hindi-matter-<random>`. Codex
//! keeps its login in `data/agents/codex-acp/home`: on the first run the Codex test fails
//! asking for a sign-in there, once (`CODEX_HOME=<that home> codex login --device-auth`).
//! Claude uses the user's own settings (Vertex) through `HOME`.
//!
//! Each test finds its adapter the first way that works, and prints which:
//! 1. the executable `CLAUSEO_HINDI_CLAUDE_ACP` or `CLAUSEO_HINDI_CODEX_ACP` names;
//! 2. else, with `CLAUSEO_HINDI_NODE` naming a `node` (its `npm` must sit beside it), the test
//!    runs `npm ci` of the pinned packages (`crates/limits/tests/live`) into
//!    `$CLAUSEO_HINDI_LIVE_DATA/adapters` itself, and runs the adapter with that node. A failed
//!    install fails the test with the tail of npm's output;
//! 3. else it fails and says how to choose one of the two.
//!
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
use crate::write;

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

/// The Claude adapter: its variable, and the script of its pinned package.
const CLAUDE_ACP: live_run::Adapter = live_run::Adapter {
    var: "CLAUSEO_HINDI_CLAUDE_ACP",
    script: "@agentclientprotocol/claude-agent-acp/dist/index.js",
};

/// The Codex adapter: its variable, and the script of its pinned package.
const CODEX_ACP: live_run::Adapter = live_run::Adapter {
    var: "CLAUSEO_HINDI_CODEX_ACP",
    script: "@agentclientprotocol/codex-acp/dist/index.js",
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

#[test]
fn an_answers_messages_are_joined_with_a_newline() {
    let asked = fake("hindi-two-messages", CLAUDE, "hindi-answer-two-messages");
    assert_eq!(
        asked.answer,
        "Let me check the papers for the planted fact.\n\
         किरायेदार श्रीमती सुरेखा भार्गव हैं। उन्होंने ₹3,47,215 की जमानत राशि जमा की, और \
         किरायानामा 17 मार्च 2024 को हुआ। यह जानकारी kirayanama.pdf और patra.docx में मिली।",
        "{}",
        asked.record()
    );
    assert!(asked.verdict.passed(), "{}", asked.record());
}

/// The live test's adapter choice (no install runs): the named executable, or an error naming
/// both ways to choose.
#[test]
fn the_adapter_is_the_named_executable_or_an_error_names_both_ways() {
    let dir = Dir::new("hindi-choose", &[]);
    let root = dir.work_path("live");
    let exe = dir.work_path("claude-acp");
    write::write(&exe, b"");
    let command = live_run::choose(CLAUDE_ACP, &root, Some(exe.clone()), None).unwrap();
    assert_eq!(
        command,
        live_run::Command {
            program: exe.display().to_string(),
            args: Vec::new(),
            route: live_run::Route::Executable,
        }
    );

    let missing = dir.work_path("no-such-adapter");
    let error = live_run::choose(CLAUDE_ACP, &root, Some(missing), None).unwrap_err();
    assert!(error.contains("it names"), "{error}");

    let error = live_run::choose(CLAUDE_ACP, &root, None, None).unwrap_err();
    assert!(
        error.contains(CLAUDE_ACP.var) && error.contains(live_run::NODE_VAR),
        "{error}"
    );

    let error = live_run::choose(CLAUDE_ACP, &root, None, Some("node".into())).unwrap_err();
    assert!(error.contains("absolute path"), "{error}");
}

/// The live folder: created when absent, and a name that overlaps the app's data folder, or
/// is relative, creates nothing.
#[test]
fn the_live_folder_is_created_when_absent_and_an_overlapping_name_creates_nothing() {
    let dir = Dir::new("hindi-root", &[]);
    let app = dir.work_path("app-data");
    write::create_dir_all(&app);
    let root = dir.work_path("live");
    assert!(!root.exists());
    let made = live_run::prepare_root(Some(root.clone().into_os_string()), &app).unwrap();
    assert_eq!(made, root);
    assert!(root.is_dir());

    let inside = app.join("hindi");
    let error = live_run::prepare_root(Some(inside.clone().into_os_string()), &app).unwrap_err();
    assert!(error.contains("overlaps"), "{error}");
    assert!(
        !inside.exists(),
        "a refused name created {}",
        inside.display()
    );

    let error = live_run::prepare_root(Some(app.clone().into_os_string()), &app).unwrap_err();
    assert!(error.contains("overlaps"), "{error}");
    let error = live_run::prepare_root(Some("live".into()), &app).unwrap_err();
    assert!(error.contains("absolute"), "{error}");
    assert!(live_run::prepare_root(None, &app).is_err());
}

/// Route 2 against a fake `npm` (a shell script the test writes beside a fake `node`): `npm
/// ci` runs in the adapters folder with the pins copied, `node`'s folder first on `PATH`, and
/// the script it leaves is the adapter. A fake `npm` is a shell script, so this is unix only.
#[cfg(unix)]
mod pinned_install {
    use std::path::{Path, PathBuf};

    use super::*;

    /// The fake `npm` body that passes: it records its arguments, checks the pins and `PATH`,
    /// and leaves the Claude adapter's script where `npm ci` would.
    fn passing(bin: &Path) -> String {
        format!(
            "printf '%s\n' \"$@\" > '{args}'\n\
             case \":$PATH:\" in *\":{bin}:\"*) ;; *) echo \"PATH does not start with {bin}\" >&2; exit 3;; esac\n\
             for f in package.json package-lock.json; do [ -f \"$3/$f\" ] || {{ echo \"no $f in $3\" >&2; exit 4; }}; done\n\
             mkdir -p \"$3/node_modules/@agentclientprotocol/claude-agent-acp/dist\"\n\
             : > \"$3/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js\"\n",
            args = bin.join("args.txt").display(),
            bin = bin.display(),
        )
    }

    /// Writes `bin/node` (never run: the fake `npm` stands for `npm ci`) and `bin/npm` with
    /// `npm_body`; returns the node path.
    fn fake_node(dir: &Dir, npm_body: &str) -> PathBuf {
        let bin = dir.work_path("node-bin");
        let node = bin.join("node");
        write::write(&node, b"");
        let npm = bin.join("npm");
        write::write(&npm, format!("#!/bin/sh\n{npm_body}").as_bytes());
        write::make_executable(&npm);
        node
    }

    #[test]
    fn the_pinned_install_runs_npm_ci_beside_node_and_names_the_adapter_script() {
        let dir = Dir::new("hindi-install", &[]);
        let root = dir.work_path("live");
        let bin = dir.work_path("node-bin");
        let node = fake_node(&dir, &passing(&bin));

        let command = live_run::install(CLAUDE_ACP, &root, &node).unwrap();
        let adapters = root.join("adapters");
        let script = adapters
            .join("node_modules")
            .join("@agentclientprotocol/claude-agent-acp/dist/index.js");
        assert_eq!(
            command,
            live_run::Command {
                program: node.display().to_string(),
                args: vec![script.display().to_string()],
                route: live_run::Route::PinnedInstall,
            }
        );
        assert!(script.is_file(), "{}", script.display());
        assert_eq!(
            std::fs::read_to_string(bin.join("args.txt")).unwrap(),
            format!("ci\n--prefix\n{}\n", adapters.display())
        );
    }

    #[test]
    fn a_failed_pinned_install_fails_with_the_tail_of_npms_output() {
        let dir = Dir::new("hindi-install-fails", &[]);
        let root = dir.work_path("live");
        let node = fake_node(
            &dir,
            "echo 'npm ERR! code E404' >&2\necho 'npm ERR! 404 Not Found: the pinned adapter' >&2\nexit 1\n",
        );

        let error = live_run::install(CLAUDE_ACP, &root, &node).unwrap_err();
        assert!(
            error.contains("npm ci of the pinned adapters failed"),
            "{error}"
        );
        assert!(
            error.contains("404 Not Found: the pinned adapter"),
            "{error}"
        );
        assert!(
            !root.join("adapters/node_modules").exists(),
            "a failed install left packages"
        );
    }

    #[test]
    fn an_install_that_leaves_no_adapter_script_fails() {
        let dir = Dir::new("hindi-install-empty", &[]);
        let root = dir.work_path("live");
        let node = fake_node(&dir, "exit 0\n");

        let error = live_run::install(CLAUDE_ACP, &root, &node).unwrap_err();
        assert!(error.contains("is missing"), "{error}");
    }

    #[test]
    fn a_node_with_no_npm_beside_it_fails_before_any_install() {
        let dir = Dir::new("hindi-install-no-npm", &[]);
        let root = dir.work_path("live");
        let bin = dir.work_path("lone-node");
        write::write(&bin.join("node"), b"");

        let error = live_run::choose(CLAUDE_ACP, &root, None, Some(bin.join("node"))).unwrap_err();
        assert!(error.contains("no npm beside"), "{error}");
    }
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
        live_run::run(wanted, super::CLAUDE_ACP, &env);
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
        live_run::run(wanted, super::CODEX_ACP, &env);
    }
}
