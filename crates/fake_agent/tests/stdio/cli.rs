//! `--check`, `--list`, bad arguments, and validation of every bundled scenario.

use std::path::Path;

use clauseo_fake_agent::{SCENARIOS, Scenario, bin_path, scenario_path};

use crate::harness::{Agent, Scratch};

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
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert_eq!(finished.stdout, format!("ok {}\n", SCENARIOS.len()));
}

#[test]
fn handshake_names_the_crate_version() {
    let text = std::fs::read_to_string(scenarios_dir().join("common/handshake.jsonl")).unwrap();
    let version = format!(
        r#""agentInfo":{{"name":"fake_agent","version":"{}"}}"#,
        env!("CARGO_PKG_VERSION")
    );
    assert!(
        text.contains(&version),
        "handshake.jsonl must carry {version}"
    );
}

#[test]
fn list_prints_the_bundled_names() {
    let dir = Scratch::new("list_prints_the_bundled_names");
    let finished = Agent::start_args(&["--list"], dir.path()).finish();
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
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
    std::fs::create_dir(dir.path().join("common")).unwrap();
    std::fs::copy(
        scenarios_dir().join("common/handshake.jsonl"),
        dir.path().join("common/handshake.jsonl"),
    )
    .unwrap();
    std::fs::write(dir.path().join("files-multi-turn.jsonl"), planted).unwrap();
    let finished = Agent::start_args(&["--check", "."], dir.path()).finish();
    assert_eq!(finished.status.code(), Some(2));
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
    assert_eq!(finished.status.code(), Some(2));
    assert_eq!(
        finished.stderr,
        "fake_agent: .: no .jsonl files in this folder\n"
    );
}

#[test]
fn bad_arguments_exit_2_with_usage() {
    let dir = Scratch::new("bad_arguments_exit_2_with_usage");
    let usage = "fake_agent: usage: fake_agent --scenario <name or path> | --check <path>... | --list | \
                 --hold\n";
    for args in [
        &[][..],
        &["--scenario"],
        &["--check"],
        &["--list", "x"],
        &["--frob"],
    ] {
        let finished = Agent::start_args(args, dir.path()).finish();
        assert_eq!(finished.status.code(), Some(2), "{args:?}");
        assert_eq!(finished.stderr, usage, "{args:?}");
    }
    let finished = Agent::start_args(&["--scenario", "no-such"], dir.path()).finish();
    assert_eq!(finished.status.code(), Some(2));
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
