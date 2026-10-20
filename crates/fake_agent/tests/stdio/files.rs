//! `files-multi-turn`: file steps under the session's cwd.

use std::path::Path;

use serde_json::{Value, json};

use crate::harness::{Agent, Scratch, prompt, result};

/// Reads one turn's updates up to the prompt result for request `id`, and returns the updates.
fn turn(agent: &mut Agent, id: u64) -> Vec<Value> {
    agent.send(&prompt(id, "Next"));
    let mut updates = Vec::new();
    loop {
        let line = agent.read_line();
        if line.get("id").is_some() {
            assert_eq!(line, result(id, json!({"stopReason": "end_turn"})));
            return updates;
        }
        updates.push(line["params"]["update"].clone());
    }
}

/// The `locations` paths of each `tool_call` in `updates`.
fn locations(updates: &[Value]) -> Vec<Vec<String>> {
    updates
        .iter()
        .filter(|u| u["sessionUpdate"] == "tool_call")
        .map(|u| {
            let paths = u["locations"].as_array().unwrap();
            paths
                .iter()
                .map(|p| p["path"].as_str().unwrap().to_owned())
                .collect()
        })
        .collect()
}

fn read(dir: &Path, path: &str) -> String {
    std::fs::read_to_string(dir.join(path)).unwrap()
}

/// The entries directly in `dir`, sorted by name.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The session folder: a subfolder of the scratch root with a space in its name. The agent runs
/// in the root, so a file step that ignored the session cwd would write beside it.
const MATTER: &str = "Matter folder";

#[test]
fn files_multi_turn_changes_cwd() {
    let root = Scratch::new("files_multi_turn_changes_cwd");
    let matter = root.path().join(MATTER);
    std::fs::create_dir(&matter).unwrap();
    let cwd = matter.to_str().unwrap().to_owned();
    let at = |path: &str| format!("{cwd}/{path}");
    let mut agent = Agent::start("files-multi-turn", root.path());
    agent.handshake(&cwd);

    let first = turn(&mut agent, 2);
    assert_eq!(read(&matter, "notes/a.txt"), "Alpha\n");
    assert_eq!(read(&matter, "notes/b.txt"), "Beta\n");
    assert_eq!(entries(root.path()), [MATTER]);
    assert_eq!(
        locations(&first),
        [vec![at("notes/a.txt")], vec![at("notes/b.txt")]]
    );
    assert_eq!(first[0]["content"][0]["path"], at("notes/a.txt"));

    let second = turn(&mut agent, 3);
    assert_eq!(read(&matter, "notes/a.txt"), "Alpha, revised\n");
    assert!(!matter.join("notes/b.txt").exists());
    assert_eq!(read(&matter, "archive/b.txt"), "Beta\n");
    assert_eq!(entries(root.path()), [MATTER]);
    assert_eq!(
        locations(&second),
        [
            vec![at("notes/a.txt")],
            vec![at("notes/b.txt"), at("archive/b.txt")]
        ]
    );

    let third = turn(&mut agent, 4);
    assert!(!matter.join("notes/a.txt").exists());
    assert_eq!(locations(&third), [vec![at("notes/a.txt")]]);
    assert_eq!(third[0]["kind"], "delete");
    agent.expect_exit(0);
    assert_eq!(entries(root.path()), [MATTER]);
    assert_eq!(entries(&matter), ["archive", "notes"]);
    assert_eq!(entries(&matter.join("notes")), Vec::<String>::new());
}

#[test]
fn delete_missing_file_exits_3() {
    let root = Scratch::new("delete_missing_file_exits_3");
    let matter = root.path().join(MATTER);
    std::fs::create_dir(&matter).unwrap();
    let mut agent = Agent::start("files-multi-turn", root.path());
    agent.handshake(matter.to_str().unwrap());
    turn(&mut agent, 2);
    turn(&mut agent, 3);
    std::fs::remove_file(matter.join("notes/a.txt")).unwrap();
    agent.send(&prompt(4, "Next"));
    let source =
        std::fs::read_to_string(clauseo_fake_agent::scenario_path("files-multi-turn")).unwrap();
    let line = source
        .lines()
        .position(|l| l.contains("delete_file"))
        .unwrap()
        + 1;
    assert_eq!(
        agent.expect_exit(3).stderr,
        format!("fake_agent: files-multi-turn.jsonl:{line}: no such file notes/a.txt\n")
    );
    assert_eq!(entries(root.path()), [MATTER]);
}
