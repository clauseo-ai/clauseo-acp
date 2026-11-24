//! The size of a launch's rules as a matter's conversations grow: each conversation ever
//! launched in a matter leaves its work folder (`agent-state/<matter>/work/<thread>`), and the
//! launch denies every one but its own. In groups ([`super::launch`]'s `GROUPED`), not by name,
//! so 500 of them stay far under Claude Code's 2 MiB settings file.

use std::path::{Path, PathBuf};

use super::big_folder::{self, matter_folder};
use super::{LaunchDeny, claude_deny_rules_on, claude_projects, launch_deny};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// The VM's data folder.
const VM_DATA: &str = r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop";

/// The variables of the VM's lawyer.
fn vm_env(name: &str) -> Option<std::ffi::OsString> {
    match name {
        "LOCALAPPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Local".into()),
        "APPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Roaming".into()),
        _ => None,
    }
}

/// `count` conversation ids as Clauseo makes them (`UUIDv7`): a millisecond time, a few hours
/// apart, around the big folder's own thread's, then random bits (from a fixed seed).
fn threads(count: usize) -> Vec<String> {
    let own_time: u64 = 0x01a1_d5be_5efa;
    let mut seed: u64 = 0x5eed;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        seed >> 16
    };
    (0..count)
        .map(|n| {
            let hours = u64::try_from(n).unwrap() * 3 + next() % 3;
            let time = own_time - 400 * 3_600_000 + hours * 3_600_000 + next() % 3_600_000;
            let variant = 0x8000 | (next() & 0x3fff);
            format!(
                "{:08x}-{:04x}-7{:03x}-{variant:04x}-{:012x}",
                time >> 16,
                time & 0xffff,
                next() & 0xfff,
                next() & 0xffff_ffff_ffff,
            )
        })
        .collect()
}

/// The JSON size of the Read and Edit rules with the Windows shell rules of `deny`, for `<data>`
/// spelled each way of `spellings`.
fn size(deny: &LaunchDeny, spellings: &[PathBuf]) -> usize {
    let deny = LaunchDeny {
        spellings: spellings.to_vec(),
        ..deny.clone()
    };
    // Windows: no shell sandbox, so the own folders' glob denies are in too.
    let read_edit = claude_deny_rules_on(&deny, true, false);
    let shell = super::windows_shell_deny_rules(&deny, &vm_env);
    serde_json::to_string(&read_edit).unwrap().len() + serde_json::to_string(&shell).unwrap().len()
}

/// The rules' size for the big folder (60 matters) whose own matter holds 1 and 500 other
/// conversations' work folders, with the default folder and with a configured one spelled two
/// ways; and, for the record, the size had each been named whole (as before the groups). On
/// Windows two spellings' rules are 914,264 bytes before any conversation (the data folder's,
/// about 87% of the 1 MiB bound here) and 946,984 at 500; named whole, 1,555,984.
#[test]
fn five_hundred_conversations_in_a_matter_stay_under_the_settings_file_cap() {
    let configured = [
        PathBuf::from(r"D:\Firm Data\Clauseo Store"),
        PathBuf::from(r"C:\Users\clauseo-lawyer\AppData\Roaming\Clauseo Store"),
    ];
    let mut sizes = Vec::new();
    for count in [0, 500] {
        let dir = TempDir::new(&format!("work-folders-{count}"));
        let big = big_folder::big(dir.path());
        let work = big.work.parent().unwrap();
        let threads = threads(count);
        for thread in &threads {
            let draft = work.join(thread).join("draft.md");
            write::create_dir_all(draft.parent().unwrap()).unwrap();
            write::write_file(&draft, b"x").unwrap();
        }
        let home = big.data.join("agents/claude-acp/home");
        let own = [big.run, big.work.clone(), big.notes, home.join("plans")];
        let projects = claude_projects(&home, &[Path::new(&matter_folder(0))]);
        let deny = launch_deny(vec![big.data.clone()], &big.data, &own, &projects);
        let rest = big.work.strip_prefix(&big.data).unwrap().parent().unwrap();
        let rest = rest.to_str().unwrap().replace('\\', "/");
        let whole = LaunchDeny {
            targets: (deny.targets.iter().cloned())
                .chain(threads.iter().map(|thread| format!("{rest}/{thread}/**")))
                .collect(),
            ..deny.clone()
        };
        for spellings in [vec![PathBuf::from(VM_DATA)], configured.to_vec()] {
            let (grouped, named) = (size(&deny, &spellings), size(&whole, &spellings));
            eprintln!(
                "{count} other conversations, {} spelling(s): {} targets, {grouped} bytes; \
                 each named whole: {named} bytes",
                spellings.len(),
                deny.targets.len(),
            );
            sizes.push((grouped, named));
        }
    }
    let [
        (none_one, _),
        (none_two, _),
        (five_one, named_one),
        (five_two, named_two),
    ] = sizes.as_slice()
    else {
        panic!("{sizes:?}");
    };
    eprintln!(
        "a conversation each: {} and {} bytes in groups, {} and {} bytes named whole",
        (five_one - none_one) / 500,
        (five_two - none_two) / 500,
        (named_one - none_one) / 500,
        (named_two - none_two) / 500,
    );
    assert!(*five_two < 1024 * 1024, "{sizes:?}");
    // The groups grow, not one rule a conversation: 46 and 65 bytes a conversation measured on
    // Windows (1,010 and 1,283 named whole), bound at 100 for the margin.
    assert!(five_one - none_one < 500 * 100, "{sizes:?}");
    assert!(five_two - none_two < 500 * 100, "{sizes:?}");
}

/// The own project's folder in the big folder: matter 0's Claude project (`launch.rs`).
fn own_project() -> String {
    let key = super::claude_project_key(&matter_folder(0));
    format!("agents/claude-acp/home/projects/{key}")
}

/// The big folder under `root` with `files` written into it, and its launch enumerated again:
/// the session's own folders left out and its project as `claude_projects` finds it.
fn enumerate(big: &big_folder::Big, files: &[String]) -> LaunchDeny {
    for file in files {
        let path = big.data.join(file);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, b"x").unwrap();
    }
    let home = big.data.join("agents/claude-acp/home");
    let own = [
        big.run.clone(),
        big.work.clone(),
        big.notes.clone(),
        home.join("plans"),
    ];
    let projects = claude_projects(&home, &[Path::new(&matter_folder(0))]);
    launch_deny(vec![big.data.clone()], &big.data, &own, &projects)
}

/// Six other conversations in the own project: each a transcript, a subagent's transcript and a
/// tool result, as Claude Code keeps them.
fn sessions(count: usize) -> Vec<String> {
    let project = own_project();
    threads(count)
        .iter()
        .flat_map(|id| {
            [
                format!("{project}/{id}.jsonl"),
                format!("{project}/{id}/subagents/a.jsonl"),
                format!("{project}/{id}/tool-results/x"),
            ]
        })
        .collect()
}

/// The own project's other conversations are covered by the fixed patterns (`DENIED`: each
/// transcript, subagent folder and memory folder), so a launch names none of them and its rules
/// are the same size, and as many, as with none.
#[test]
fn other_conversations_in_the_own_project_add_no_rules() {
    let (none_dir, six_dir) = (
        TempDir::new("work-folders-own-project-none"),
        TempDir::new("work-folders-own-project-six"),
    );
    let none = enumerate(&big_folder::big(none_dir.path()), &[]);
    let six_big = big_folder::big(six_dir.path());
    let six = enumerate(&six_big, &sessions(6));
    let project = own_project();
    for id in threads(6) {
        for target in [
            format!("{project}/{id}.jsonl"),
            format!("{project}/{id}/subagents/**"),
        ] {
            assert!(
                !six.targets.contains(&target),
                "{target}: {:?}",
                six.targets
            );
        }
    }
    assert_eq!(six.targets, none.targets);
    let spellings = [PathBuf::from(VM_DATA)];
    let (none_size, six_size) = (size(&none, &spellings), size(&six, &spellings));
    eprintln!(
        "0 sessions: {} targets, {none_size} bytes; 6 sessions: {} targets, {six_size} bytes",
        none.targets.len(),
        six.targets.len(),
    );
    assert_eq!(six_size, none_size);
}

/// An entry in the own project that no fixed pattern covers is still named: a file beside the
/// transcripts, and a folder in a conversation's folder beside its subagents and tool results.
#[test]
fn an_own_project_entry_no_pattern_covers_is_named() {
    let dir = TempDir::new("work-folders-own-project-named");
    let big = big_folder::big(dir.path());
    let project = own_project();
    let id = &threads(1)[0];
    let deny = enumerate(
        &big,
        &[
            format!("{project}/notes.txt"),
            format!("{project}/{id}/other-folder/x"),
        ],
    );
    for target in [
        format!("{project}/notes.txt"),
        format!("{project}/{id}/other-folder/**"),
    ] {
        assert!(
            deny.targets.contains(&target),
            "{target}: {:?}",
            deny.targets
        );
    }
}

/// A name that only looks like a covered one is still named: `<id>.jsonl.bak` is no transcript
/// (`*.jsonl` ends the name), and `<id>/subagents-old/a.jsonl` is no subagent folder.
#[test]
fn a_name_that_only_looks_covered_is_named() {
    let dir = TempDir::new("work-folders-own-project-similar");
    let big = big_folder::big(dir.path());
    let project = own_project();
    let id = &threads(1)[0];
    let backup = format!("{project}/{id}.jsonl.bak");
    let folder = format!("{project}/{id}/subagents-old");
    // `subagents/**` covers what is in such a folder, never a file of that name.
    let file = format!("{project}/{}/subagents", threads(2)[1]);
    let hidden = format!("{project}/.{id}.jsonl");
    let made = [
        backup.clone(),
        format!("{folder}/a.jsonl"),
        file.clone(),
        hidden.clone(),
    ];
    let deny = enumerate(&big, &made);
    for target in [backup, format!("{folder}/**"), file, hidden] {
        assert!(
            deny.targets.contains(&target),
            "{target}: {:?}",
            deny.targets
        );
    }
}
