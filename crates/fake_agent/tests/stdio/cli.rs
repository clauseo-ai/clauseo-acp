//! `--check`, `--list`, `--pace-ms`, bad arguments, and validation of every bundled scenario.

use std::path::Path;

use clauseo_fake_agent::{EXIT_LOAD, EXIT_OK, SCENARIOS, Scenario, bin_path, scenario_path};
use serde_json::json;

use crate::harness::{Agent, Scratch, prompt, result, update};
use crate::write;

const USAGE: &str = "fake_agent: usage: fake_agent --scenario <name or path> [--pace-ms <n>] \
     | --check <path>... | --list | --hold\n";
const NUMBER: &str = "fake_agent: --pace-ms takes a whole number of milliseconds from 0 to 60000\n";
const NEEDS: &str = "fake_agent: --pace-ms needs --scenario\n";
const TWICE: &str = "fake_agent: --pace-ms may be given only once\n";

fn scenarios_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios"))
}

#[test]
fn every_bundled_scenario_validates() {
    for name in SCENARIOS {
        let path = scenario_path(name);
        Scenario::load(&path).unwrap_or_else(|error| panic!("{error}"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(Scenario::parse(name, &text).unwrap().name(), *name);
    }
    let dir = Scratch::new("every_bundled_scenario_validates");
    let agent = Agent::start_args(&["--check", scenarios_dir().to_str().unwrap()], dir.path());
    let finished = agent.finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert_eq!(finished.stdout, format!("ok {}\n", SCENARIOS.len()));
}

/// Every `agentInfo.version` in `value`, with its JSON pointer.
fn agent_versions(value: &serde_json::Value, pointer: &str, found: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let at = format!("{pointer}/{key}");
                if key == "agentInfo" {
                    found.push((at.clone(), child["version"].to_string()));
                }
                agent_versions(child, &at, found);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                agent_versions(child, &format!("{pointer}/{index}"), found);
            }
        }
        _ => {}
    }
}

/// The crate version has one home, Cargo.toml; every `agentInfo` in every bundled scenario and
/// common include must carry it.
#[test]
fn every_agent_info_names_the_crate_version() {
    let mut files: Vec<std::path::PathBuf> =
        SCENARIOS.iter().map(|name| scenario_path(name)).collect();
    for entry in std::fs::read_dir(scenarios_dir().join("common")).unwrap() {
        files.push(entry.unwrap().path());
    }
    let want = format!("\"{}\"", env!("CARGO_PKG_VERSION"));
    let mut carrying = Vec::new();
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(file).unwrap();
        for (index, line) in text.lines().enumerate() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            let mut found = Vec::new();
            agent_versions(&value, "", &mut found);
            for (pointer, version) in found {
                assert_eq!(version, want, "{name}:{}: {pointer}", index + 1);
                carrying.push(name.clone());
            }
        }
    }
    carrying.dedup();
    for name in ["handshake.jsonl", "initialize-caps.jsonl"] {
        assert!(
            carrying.iter().any(|c| c == name),
            "{name} has no agentInfo: {carrying:?}"
        );
    }
}

#[test]
fn list_prints_the_bundled_names() {
    let dir = Scratch::new("list_prints_the_bundled_names");
    let finished = Agent::start_args(&["--list"], dir.path()).finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    let lines: Vec<&str> = finished.stdout.lines().collect();
    assert_eq!(lines, SCENARIOS);
}

/// Planting check 5 of the brief: a `ToolKind` the schema does not have fails `--check`, naming
/// `session/update`, the file and the line.
#[test]
fn edit_file_kind_fails_validation_naming_session_update() {
    let dir = Scratch::new("edit_file_kind_fails_validation");
    let source = std::fs::read_to_string(scenario_path("files-multi-turn")).unwrap();
    let line = source
        .lines()
        .position(|l| l.contains(r#""kind":"edit""#))
        .unwrap()
        + 1;
    let planted = source.replacen(r#""kind":"edit""#, r#""kind":"edit_file""#, 1);
    write::create_dir(dir.path().join("common")).unwrap();
    write::copy(
        scenarios_dir().join("common/handshake.jsonl"),
        dir.path().join("common/handshake.jsonl"),
    )
    .unwrap();
    write::write(dir.path().join("files-multi-turn.jsonl"), planted).unwrap();
    let finished = Agent::start_args(&["--check", "."], dir.path()).finish();
    assert_eq!(finished.status.code(), Some(EXIT_LOAD));
    assert_eq!(finished.stdout, "");
    assert_eq!(
        finished.stderr,
        format!(
            "fake_agent: files-multi-turn.jsonl:{line}: session/update: /update/kind: \"edit_file\" \
             does not survive the SDK schema (became \"other\")\n"
        )
    );
}

#[test]
fn check_of_an_empty_folder_fails() {
    let dir = Scratch::new("check_of_an_empty_folder_fails");
    let finished = Agent::start_args(&["--check", "."], dir.path()).finish();
    assert_eq!(finished.status.code(), Some(EXIT_LOAD));
    assert_eq!(
        finished.stderr,
        "fake_agent: .: no .jsonl files in this folder\n"
    );
}

#[test]
fn bad_arguments_exit_2_with_usage() {
    let dir = Scratch::new("bad_arguments_exit_2_with_usage");
    for args in [
        &[][..],
        &["--scenario"],
        &["--check"],
        &["--list", "x"],
        &["--frob"],
    ] {
        let finished = Agent::start_args(args, dir.path()).finish();
        assert_eq!(finished.status.code(), Some(EXIT_LOAD), "{args:?}");
        assert_eq!(finished.stderr, USAGE, "{args:?}");
    }
    let finished = Agent::start_args(&["--scenario", "no-such"], dir.path()).finish();
    assert_eq!(finished.status.code(), Some(EXIT_LOAD));
    assert_eq!(
        finished.stderr,
        "fake_agent: unknown scenario `no-such`; `fake_agent --list` prints the bundled names\n"
    );
}

#[test]
fn bin_path_finds_the_built_binary() {
    let path = bin_path().unwrap();
    assert_eq!(path, Path::new(env!("CARGO_BIN_EXE_fake_agent")));
}

/// A bare name is bundled; a path (one with a separator, or a `.jsonl` name) is a file. A
/// missing file is an I/O error naming that path, never an unknown bundled name.
#[test]
fn scenario_takes_a_path_only_with_a_separator_or_jsonl() {
    let dir = Scratch::new("scenario_takes_a_path");
    write::copy(scenario_path("idle"), dir.path().join("idle.jsonl")).unwrap();
    // A `.jsonl` name in the working folder runs as a file.
    Agent::start_args(&["--scenario", "idle.jsonl"], dir.path()).expect_exit(EXIT_OK);
    // A path with a separator runs as a file, whatever its extension.
    let by_path = scenario_path("idle");
    Agent::start_args(&["--scenario", by_path.to_str().unwrap()], dir.path()).expect_exit(EXIT_OK);
    for target in ["sub/missing", "missing.jsonl"] {
        let finished = Agent::start_args(&["--scenario", target], dir.path()).finish();
        assert_eq!(finished.status.code(), Some(EXIT_LOAD), "{target}");
        assert!(
            finished
                .stderr
                .starts_with(&format!("fake_agent: {target}: ")),
            "{target}: {}",
            finished.stderr
        );
    }
}

/// `--check` of a folder takes its `.jsonl` files and ignores the rest.
#[test]
fn check_of_a_folder_takes_only_the_jsonl_files() {
    let dir = Scratch::new("check_takes_only_jsonl");
    write::copy(scenario_path("idle"), dir.path().join("idle.jsonl")).unwrap();
    write::write(dir.path().join("notes.txt"), "not a scenario\n").unwrap();
    let finished = Agent::start_args(&["--check", "."], dir.path()).finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    assert_eq!(finished.stdout, "ok 1\n");
}

/// One `prompt-stream` answer chunk, as the scenario writes it.
fn answer_chunk(text: &str) -> serde_json::Value {
    update(
        json!({"sessionUpdate": "agent_message_chunk", "messageId": "m-answer-1",
        "content": {"type": "text", "text": text}}),
    )
}

/// Plays `prompt-stream` started with `args` to its end, with the lines `prompt_stream_over_stdio`
/// checks.
fn prompt_stream_plays(args: &[&str], test: &str) {
    let dir = Scratch::new(test);
    let mut agent = Agent::start_args(args, dir.path());
    agent.handshake(&dir.cwd());
    agent.send(&prompt(2, "Summarise the indemnity clause"));
    for text in ["Reading the ", "indemnity clause ", "first."] {
        agent.expect_line(&update(json!({"sessionUpdate": "agent_thought_chunk",
            "messageId": "m-thought-1", "content": {"type": "text", "text": text}})));
    }
    for text in [
        "The clause ",
        "caps liability ",
        "at the fees ",
        "paid in the ",
        "last twelve months.",
    ] {
        agent.expect_line(&answer_chunk(text));
    }
    agent.expect_line(&result(2, json!({"stopReason": "end_turn"})));
    agent.expect_exit(EXIT_OK);
}

#[test]
fn pace_ms_before_the_scenario_plays_to_the_end() {
    prompt_stream_plays(
        &["--pace-ms", "1", "--scenario", "prompt-stream"],
        "pace_ms_before_the_scenario",
    );
}

#[test]
fn pace_ms_after_the_scenario_plays_to_the_end() {
    prompt_stream_plays(
        &["--scenario", "prompt-stream", "--pace-ms", "0"],
        "pace_ms_after_the_scenario",
    );
}

/// Each bad `--pace-ms` form exits `EXIT_LOAD` with its one-line diagnostic and plays nothing.
#[test]
fn pace_ms_bad_forms_exit_2_with_a_diagnostic() {
    let dir = Scratch::new("pace_ms_bad_forms");
    let cases: [(&[&str], &str); 14] = [
        (&["--pace-ms", "5"], NEEDS),
        (&["--pace-ms"], NEEDS),
        (&["--check", ".", "--pace-ms", "5"], NEEDS),
        (&["--pace-ms", "5", "--list"], NEEDS),
        (&["--scenario", "prompt-stream", "--pace-ms"], NUMBER),
        (&["--pace-ms", "x", "--scenario", "prompt-stream"], NUMBER),
        (&["--pace-ms", "-1", "--scenario", "prompt-stream"], NUMBER),
        (&["--pace-ms", "1.5", "--scenario", "prompt-stream"], NUMBER),
        (
            &["--pace-ms", "60001", "--scenario", "prompt-stream"],
            NUMBER,
        ),
        (&["--pace-ms", "", "--scenario", "prompt-stream"], NUMBER),
        (
            &[
                "--pace-ms",
                "1",
                "--scenario",
                "prompt-stream",
                "--pace-ms",
                "2",
            ],
            TWICE,
        ),
        (
            &[
                "--pace-ms",
                "1",
                "--pace-ms",
                "1",
                "--scenario",
                "prompt-stream",
            ],
            TWICE,
        ),
        (
            &["--scenario", "prompt-stream", "--pace-ms", "1", "extra"],
            USAGE,
        ),
        (
            &["--scenario", "prompt-stream", "--pace-ms", "1", "--list"],
            USAGE,
        ),
    ];
    for (args, diagnostic) in cases {
        let finished = Agent::start_args(args, dir.path()).finish();
        assert_eq!(finished.status.code(), Some(EXIT_LOAD), "{args:?}");
        assert_eq!(finished.stdout, "", "{args:?}");
        assert_eq!(finished.stderr, diagnostic, "{args:?}");
    }
}
