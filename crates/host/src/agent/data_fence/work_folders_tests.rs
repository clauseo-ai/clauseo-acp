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
/// Windows two spellings' rules are 906,222 bytes before any conversation (the data folder's,
/// about 86% of the 1 MiB bound here) and 938,942 at 500; named whole, 1,547,942.
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
