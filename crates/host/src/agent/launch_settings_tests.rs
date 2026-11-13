//! Claude's flag settings as a file (`launch_settings.rs`): written where and as the launch
//! says, the same JSON the call carried inline, a fresh name per call, removed with the run and
//! swept at the host's first launch; and a 60-matter launch carries only the path.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::{Target, remove, sweep};
use crate::agent::data_fence::big_folder::{self, OWN_RUN};
use crate::agent::data_fence::{LaunchDeny, claude_deny_rules, claude_deny_rules_on};
use crate::agent::loads::profile::{ClaudeKeys, Folders, Login};
use crate::agent::loads::{GuardHook, McpControl, session_meta};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// The target of run [`OWN_RUN`] under `data`.
fn target(data: &Path) -> Target {
    Target {
        folder: super::folder(data),
        run: OWN_RUN.to_owned(),
    }
}

/// A Claude session's `_meta` with the host's keys over `deny`, as `session_loads` builds it.
fn claude_meta(deny: &LaunchDeny, run: &Path, notes: &Path, matter: &Path) -> Map<String, Value> {
    let work = run.join("work");
    let folders = Folders::of(&work, notes).unwrap();
    let keys = ClaudeKeys {
        web_search: crate::agent::loads::browser::Switch {
            on: true,
            locked: false,
        },
        login: Login::FirstParty,
        folders,
        seed_sites: crate::browser::seed_sites::SEED_SITES,
        forced_ask: false,
        data: deny,
        case_law: true,
        browser: true,
        sandbox: None,
        matter: Some(matter),
        trusted_servers: &[],
    };
    let guard = Some(GuardHook {
        program: matter,
        matter,
        ask_all: false,
    });
    session_meta(None, McpControl::Clauseo, guard, Some(&keys))
}

/// `meta`'s `claudeCode.options.settings`.
fn settings(meta: &Map<String, Value>) -> &Value {
    &meta["claudeCode"]["options"]["settings"]
}

/// The settings go to `<data>/launch-settings/<run>.json`, private to the user, with the JSON
/// the call would have carried inline, byte for byte; the call carries the absolute path.
#[test]
fn the_settings_move_to_the_runs_file_and_the_call_carries_its_path() {
    let dir = TempDir::new("launch-settings-write");
    let data = dir.path().join("data");
    write::create_dir_all(&data).unwrap();
    let inline = json!({"permissions": {"deny": ["Read(//x/**)"]}, "hooks": {}, "z": 1, "a": [2]});
    let mut meta = json!({"claudeCode": {"options": {"settings": inline, "model": "m"}}});
    let meta = meta.as_object_mut().unwrap();
    let path = target(&data).write(meta).unwrap().unwrap();
    assert_eq!(
        path,
        data.join("launch-settings").join(format!("{OWN_RUN}.json"))
    );
    assert!(path.is_absolute());
    assert_eq!(
        settings(meta),
        &Value::String(path.to_str().unwrap().to_owned())
    );
    assert_eq!(meta["claudeCode"]["options"]["model"], json!("m"));
    let text = std::fs::read(&path).unwrap();
    assert_eq!(text, serde_json::to_vec(&inline).unwrap());
    assert_eq!(serde_json::from_slice::<Value>(&text).unwrap(), inline);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }
    // No temp file is left beside it.
    let names = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
    assert_eq!(names, 1);
}

/// A `_meta` without Claude's settings (Codex's) is left as it is, and no file is written.
#[test]
fn a_meta_without_settings_writes_nothing() {
    let dir = TempDir::new("launch-settings-none");
    let data = dir.path().join("data");
    for meta in [
        json!({"codex": {"config": {}}}),
        json!({"claudeCode": {"options": {}}}),
        json!({"claudeCode": {"options": {"settings": "/already/a/path.json"}}}),
    ] {
        let mut changed = meta.clone();
        let written = target(&data)
            .write(changed.as_object_mut().unwrap())
            .unwrap();
        assert_eq!(written, None);
        assert_eq!(changed, meta);
    }
    assert!(!data.join("launch-settings").exists());
}

/// A file already at the run's name is never replaced: a second session call of the run (a
/// failed `session/load`, then `session/new`) gets `<run>.2.json`, and the first stays.
#[test]
fn a_taken_name_is_kept_and_the_call_gets_a_fresh_one() {
    let dir = TempDir::new("launch-settings-taken");
    let data = dir.path().join("data");
    let first = data.join("launch-settings").join(format!("{OWN_RUN}.json"));
    write::create_dir_all(first.parent().unwrap()).unwrap();
    write::write_file(&first, b"earlier").unwrap();
    let mut meta = json!({"claudeCode": {"options": {"settings": {"a": 1}}}});
    let path = target(&data)
        .write(meta.as_object_mut().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        path.file_name().unwrap(),
        format!("{OWN_RUN}.2.json").as_str()
    );
    assert_eq!(std::fs::read(&first).unwrap(), b"earlier");
    assert_eq!(std::fs::read(&path).unwrap(), br#"{"a":1}"#);
}

/// A link where the folder should be fails the write, and nothing is written through it.
#[cfg(unix)]
#[test]
fn a_linked_folder_fails_the_write() {
    let dir = TempDir::new("launch-settings-link");
    let data = dir.path().join("data");
    let elsewhere = dir.path().join("elsewhere");
    write::create_dir_all(&data).unwrap();
    write::create_dir_all(&elsewhere).unwrap();
    write::symlink(&elsewhere, &data.join("launch-settings")).unwrap();
    let mut meta = json!({"claudeCode": {"options": {"settings": {"a": 1}}}});
    assert!(target(&data).write(meta.as_object_mut().unwrap()).is_err());
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

/// The run's end removes each of its files and no other run's, a run whose id its id starts
/// (`…-1` beside `…-10`) included; the first launch's sweep removes every file and leaves a
/// folder.
#[test]
fn the_runs_files_go_with_it_and_the_sweep_takes_the_rest() {
    let dir = TempDir::new("launch-settings-remove");
    let data = dir.path().join("data");
    let folder = super::folder(&data);
    write::create_dir_all(&folder.join("a-folder")).unwrap();
    let own = [format!("{OWN_RUN}.json"), format!("{OWN_RUN}.2.json")];
    let others = [
        format!("{}.json", big_folder::PREFIX_RUN),
        format!("{OWN_RUN}0.json"),
        format!("{OWN_RUN}.json.clauseo-1-0.new"),
        format!("{OWN_RUN}.x.json"),
    ];
    for name in own.iter().chain(&others) {
        write::write_file(&folder.join(name), b"{}").unwrap();
    }
    remove(&target(&data));
    let left = |folder: &PathBuf| {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    };
    let mut want: Vec<String> = others.to_vec();
    want.push("a-folder".to_owned());
    want.sort();
    assert_eq!(left(&folder), want);
    sweep(&folder);
    assert_eq!(left(&folder), ["a-folder"]);
    // No folder: nothing to do.
    sweep(&dir.path().join("none"));
}

/// A launch in a data folder with 60 matters: the call carries a path of a few hundred
/// characters at most, never the rules, and the file holds every rule, the same JSON as the
/// inline value. Claude Code reads a settings file of at most 2 MiB (`launch_settings.rs`).
#[test]
fn a_sixty_matter_launch_carries_only_the_path() {
    let dir = TempDir::new("launch-settings-sixty");
    let big = big_folder::big(dir.path());
    let matter = dir.path().join("matter");
    write::create_dir_all(&matter).unwrap();
    let mut meta = claude_meta(&big.deny, &big.run, &big.notes, &matter);
    let inline = settings(&meta).clone();
    let inline_text = serde_json::to_string(&inline).unwrap();
    let path = target(&big.data).write(&mut meta).unwrap().unwrap();
    let Value::String(sent) = settings(&meta) else {
        panic!("{meta:?}");
    };
    assert!(sent.len() < 400, "{sent}");
    assert!(!serde_json::to_string(&meta).unwrap().contains("Read("));
    let file = std::fs::read_to_string(&path).unwrap();
    assert_eq!(file, inline_text);
    let deny: Vec<&str> = inline["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let rules = claude_deny_rules(&big.deny);
    assert!(rules.len() > 2 * big_folder::MATTERS, "{}", rules.len());
    for rule in &rules {
        assert!(deny.contains(&rule.as_str()), "{rule}");
    }
    assert!(file.len() < 2 * 1024 * 1024, "{}", file.len());
    // The size the same launch on Windows would have sent inline, for the record.
    let windows = windows_deny(&big.deny);
    let mut windows_settings = inline.clone();
    windows_settings["permissions"]["deny"] = json!(windows);
    let windows_text = serde_json::to_string(&windows_settings).unwrap();
    eprintln!(
        "sixty matters: {} targets; {} Read/Edit rules; settings {} chars here, {} chars with \
         Windows paths (32,767 is Windows' whole command line)",
        big.deny.targets.len(),
        rules.len(),
        inline_text.len(),
        windows_text.len(),
    );
    assert!(windows_text.len() > 32_767, "the case this file is for");
}

/// [`claude_deny_rules`] for the same targets with the data folder at Windows' default place.
fn windows_deny(deny: &LaunchDeny) -> Vec<String> {
    let windows = LaunchDeny {
        spellings: vec![PathBuf::from(
            r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop",
        )],
        ..deny.clone()
    };
    claude_deny_rules_on(&windows, true)
}
