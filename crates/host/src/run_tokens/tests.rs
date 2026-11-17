//! Token-file parity: every deny list, Claude's and Codex's, names exactly the token files
//! [`RUN_TOKEN_FILES`] holds. A list that names one and not another fails here, and so does a
//! token file added to a list but not to [`RUN_TOKEN_FILES`] (or the other way round), whatever
//! its name.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::RUN_TOKEN_FILES;
use crate::agent::data_fence::big_folder::big;
use crate::agent::data_fence::{
    DENIED, LaunchDeny, claude_deny_rules_on, windows_shell_deny_rules,
    windows_shell_deny_rules_without_variables,
};
use crate::pipeline::tests::support::TempDir;

/// The token-file names in `texts`: every path part `<name>-token` after a `/` or a `\`.
fn token_names<'a>(texts: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for text in texts {
        for part in text.split(['/', '\\']).skip(1) {
            let name: String = part
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                .collect();
            if name.ends_with("-token") && name.len() > "-token".len() {
                names.insert(name);
            }
        }
    }
    names
}

fn expected() -> BTreeSet<String> {
    RUN_TOKEN_FILES
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
}

/// Every file under `dir`, as `/`-separated paths below it.
fn files_under(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = vec![dir.to_owned()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let below = path.strip_prefix(dir).unwrap();
                found.push(below.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    found
}

#[test]
fn the_token_finder_reads_any_name_and_skips_near_ones() {
    let found = token_names([
        "run/*/relay-token",
        r"Bash(*C:\d\run*\third-token*)",
        "Read(//d/run/*/versions-token)",
        "not/-token",
        "url-token-in-a-word",
        "run/*/x-tokens",
    ]);
    let want: BTreeSet<String> = ["relay-token", "third-token", "versions-token"]
        .map(str::to_owned)
        .into();
    assert_eq!(found, want);
}

/// Claude's data-folder patterns, its Read and Edit rules on both OSes, and the Windows shell
/// rules (with and without the variables' spellings) each name exactly the token files.
#[test]
fn every_deny_list_names_every_token_file_and_no_other() {
    // `DENIED` names none itself: `LaunchDeny::fixed` adds them from the list.
    assert!(token_names(DENIED).is_empty(), "DENIED");
    let fixed = LaunchDeny::fixed(vec![PathBuf::from("/d")]);
    assert_eq!(
        token_names(fixed.targets.iter().map(String::as_str)),
        expected(),
        "the fixed targets"
    );
    for (windows, sandboxed) in [(false, false), (false, true), (true, false), (true, true)] {
        let deny = LaunchDeny::fixed(vec![PathBuf::from(if windows { r"C:\d" } else { "/d" })]);
        let rules = claude_deny_rules_on(&deny, windows, sandboxed);
        assert_eq!(
            token_names(rules.iter().map(String::as_str)),
            expected(),
            "Claude's rules, windows {windows}, sandboxed {sandboxed}"
        );
    }
    let deny = LaunchDeny::fixed(vec![PathBuf::from(r"C:\d")]);
    for rules in [
        windows_shell_deny_rules(&deny, &|_| None),
        windows_shell_deny_rules_without_variables(&deny, &|_| None),
    ] {
        assert_eq!(
            token_names(rules.iter().map(String::as_str)),
            expected(),
            "the Windows shell rules"
        );
    }
}

/// Codex's fence (none on Windows, `codex_fence.rs`) denies every token file in every kind of
/// turn, escalated included: the own run folder's by its path, and a later session's, in a run
/// folder no listing named, by a glob per name. Its rules name exactly the token files.
#[test]
fn codex_denies_every_token_file_the_own_and_a_later_sessions() {
    use crate::agent::codex_fence::PROFILE;
    use crate::agent::codex_fence::tests::{
        access, data, escalated_access, fence_with, listed, run_root,
    };
    let keys = fence_with(&listed());
    let scoped = keys["permissions"][PROFILE]["filesystem"][data().to_str().unwrap()]
        .as_object()
        .unwrap();
    assert_eq!(
        token_names(scoped.keys().map(String::as_str)),
        expected(),
        "Codex's rules"
    );
    let later = data().join("run").join("s9");
    for name in RUN_TOKEN_FILES {
        for file in [run_root().join(name), later.join(name)] {
            assert_eq!(access(&keys, &file), "deny", "{}", file.display());
            assert_eq!(escalated_access(&keys, &file), "deny", "{}", file.display());
        }
    }
}

/// The big test folder, which the deny tests run against, holds each token file in each run
/// folder, and no other.
#[test]
fn the_test_data_folder_holds_every_token_file_in_each_run() {
    let dir = TempDir::new("run-token-parity");
    let big = big(dir.path());
    let runs = big.data.join("run");
    let files = files_under(&runs);
    assert_eq!(
        token_names(
            files
                .iter()
                .map(|file| format!("/{file}"))
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str)
        ),
        expected()
    );
    for run in std::fs::read_dir(&runs).unwrap() {
        let run = run.unwrap().path();
        let names = files_under(&run);
        let held = token_names(
            names
                .iter()
                .map(|name| format!("/{name}"))
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str),
        );
        assert_eq!(held, expected(), "{}", run.display());
    }
}
