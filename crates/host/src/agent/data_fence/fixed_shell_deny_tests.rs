//! The fixed Windows shell rules (`fixed_shell_deny.rs`): the same short list whatever the data
//! folder holds or how long its path is, each target denied in every form, the own folders left
//! to the classifier, the settings file small, and the history's rules for this matter alone.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::super::tests::denied_by;
use super::{TARGETS, leads, windows_fixed_shell_deny_rules};
use crate::agent::data_fence::{LaunchDeny, claude_project_key, launch_deny, own_reads};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// A lawyer's default data folder with a space in the user name, and its 8.3 spelling.
const LONG: &str = r"C:\Users\Rohan Shiralkar\AppData\Roaming\ai.clauseo.desktop";
const LONG_83: &str = r"C:\Users\ROHANS~1\AppData\Roaming\ai.clauseo.desktop";
/// A short default data folder.
const SHORT: &str = r"C:\D\ai.clauseo.desktop";
/// A configured data folder, and its 8.3 spelling.
const CONFIGURED: &str = r"D:\Firm Data\Clauseo Store";
const CONFIGURED_83: &str = r"D:\FIRMDA~1\CLAUSE~1";

/// The session's own run, thread and matter.
const OWN_RUN: &str = "01a1d5be-5efa-73f6-a24b-dc90c01883d9-10";
const OWN_THREAD: &str = "01a1d5be-5efa-73f6-a24b-dc90c01883d9";
const OWN_MATTER: &str = "01a1d5b4-a291-7649-9fa4-000000000000";

/// The most shell deny rules a Windows launch may carry.
const MOST: usize = 200;

/// The largest settings file a Windows launch may write for the long default path.
const MOST_BYTES: usize = 150 * 1024;

/// A data folder under `root` with `matters` matters (the own one and others, each with notes
/// and a Claude project), `conversations` conversations' work folders in the own matter (the
/// own one first) and `runs` run folders beside the own one, enumerated for a launch and
/// spelled as on Windows by `spellings`.
fn launch(
    root: &Path,
    spellings: &[&str],
    matters: usize,
    conversations: usize,
    runs: usize,
) -> LaunchDeny {
    let data = root.join("data");
    let state = format!("agent-state/{OWN_MATTER}");
    let mut files = vec![
        "settings.json".to_owned(),
        "state.sqlite".to_owned(),
        "agents/claude-acp/home/.credentials.json".to_owned(),
        "agents/claude-acp/home/plans/p.md".to_owned(),
        "agents/codex-acp/home/auth.json".to_owned(),
        format!("run/{OWN_RUN}/relay-token"),
        format!("{state}/notes/notes.md"),
    ];
    for n in 1..matters.max(1) {
        let id = format!("01a1d5b4-a291-7649-9fa4-{n:012x}");
        files.push(format!("agent-state/{id}/notes/notes.md"));
        let key = claude_project_key(&format!(r"C:\Users\Rohan Shiralkar\Matters\Matter {n}"));
        files.push(format!("agents/claude-acp/home/projects/{key}/s.jsonl"));
    }
    for n in 0..conversations.max(1) {
        let thread = if n == 0 {
            OWN_THREAD.to_owned()
        } else {
            format!("01a1d5c4-90d5-7548-96d0-{n:012x}")
        };
        files.push(format!("{state}/work/{thread}/draft.md"));
    }
    for n in 0..runs {
        let run = format!("01a1d5c4-90d5-7548-96d0-4a16107449{n:02}-1");
        files.push(format!("run/{run}/work/draft.md"));
        files.push(format!("run/{run}/relay-token"));
    }
    for file in &files {
        let path = data.join(file);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, b"x").unwrap();
    }
    let run = data.join("run").join(OWN_RUN);
    let notes = data.join(&state).join("notes");
    let work = data.join(&state).join("work").join(OWN_THREAD);
    let plans = data.join("agents/claude-acp/home/plans");
    let own = [run, work.clone(), notes.clone(), plans];
    let mut deny = launch_deny(vec![data.clone()], &data, &own, &[]);
    deny.own_reads = own_reads(&data, &[&work, &notes]);
    deny.spellings = spellings.iter().map(PathBuf::from).collect();
    deny
}

/// The settings a Windows Claude launch of `deny` writes for its data folder, as JSON.
fn windows_settings(deny: &LaunchDeny) -> Map<String, Value> {
    let mut settings = Map::new();
    crate::agent::loads::profile::deny_data_folder_on(&mut settings, deny, false, true);
    crate::agent::one_shell::deny_bash(&mut settings, true);
    settings
}

/// The settings' `permissions.deny` and `permissions.allow`.
fn rules(settings: &Map<String, Value>, key: &str) -> Vec<String> {
    settings["permissions"][key]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|rule| rule.as_str().map(str::to_owned))
        .collect()
}

/// How many of the deny rules are `PowerShell(…)` or `Bash(…)`.
fn shell_count(settings: &Map<String, Value>) -> usize {
    rules(settings, "deny")
        .iter()
        .filter(|rule| rule.starts_with("PowerShell(") || rule.starts_with("Bash("))
        .count()
}

/// The most `PowerShell(…)` and `Bash(…)` deny rules a Windows launch may carry for each
/// spelling of `<data>`: the fixed list, the own folders' glob denies
/// (`own_shell_glob_deny_rules`, which guard their allow rules), the `Bash` tool's deny and
/// this matter's history denies (16, `history_rules`).
const MOST_PER_SPELLING: usize = 275;

/// The launch's shell deny counts for `spellings`, `matters` matters, `conversations`
/// conversations and `runs` run folders: the fixed list's, and all the settings carry, with
/// the settings' size.
fn counts(
    root: &Path,
    spellings: &[&str],
    matters: usize,
    conversations: usize,
    runs: usize,
) -> (usize, usize, usize) {
    let deny = launch(root, spellings, matters, conversations, runs);
    let settings = windows_settings(&deny);
    let fixed = windows_fixed_shell_deny_rules(&deny).len();
    let bytes = serde_json::to_string(&settings).unwrap().len();
    (fixed, shell_count(&settings), bytes)
}

/// The fixed list is the same for 1 or 30 matters, 1 or 50 conversations, 0 or 5 run folders,
/// a short or long default path with or without an 8.3 spelling, and under the cap; and all the
/// launch's shell denies are the same for any of those with the same number of spellings.
#[test]
fn the_shell_deny_count_is_fixed() {
    let dir = TempDir::new("fixed-shell-count");
    let mut n = 0;
    let mut count = |spellings: &[&str], matters, conversations, runs| {
        n += 1;
        let got = counts(
            &dir.path().join(n.to_string()),
            spellings,
            matters,
            conversations,
            runs,
        );
        eprintln!("{spellings:?} {matters} {conversations} {runs}: {got:?}");
        got
    };
    let one = count(&[LONG], 1, 1, 0);
    let two = count(&[LONG, LONG_83], 1, 1, 0);
    assert!(one.0 <= MOST, "{one:?}");
    assert_eq!(two.0, one.0);
    assert!(
        one.1 <= MOST_PER_SPELLING && two.1 <= 2 * MOST_PER_SPELLING,
        "{one:?} {two:?}"
    );
    for (spellings, matters, conversations, runs) in [
        (&[LONG][..], 30, 1, 0),
        (&[LONG][..], 1, 50, 0),
        (&[LONG][..], 1, 1, 5),
        (&[LONG][..], 30, 50, 5),
        (&[SHORT][..], 1, 1, 0),
        (&[SHORT][..], 30, 50, 5),
        (&[LONG, LONG_83][..], 30, 50, 5),
        (&[SHORT, LONG_83][..], 30, 50, 5),
    ] {
        let got = count(spellings, matters, conversations, runs);
        let want = if spellings.len() == 1 { one.1 } else { two.1 };
        assert_eq!(
            got.0, one.0,
            "{spellings:?} {matters} {conversations} {runs}"
        );
        assert_eq!(
            got.1, want,
            "{spellings:?} {matters} {conversations} {runs}"
        );
    }
    // A configured folder spelled two ways has two leads, and still does not grow.
    let configured = count(&[CONFIGURED, CONFIGURED_83], 1, 1, 0);
    assert!(configured.0 <= MOST, "{configured:?}");
    assert_eq!(
        count(&[CONFIGURED, CONFIGURED_83], 30, 50, 5).0,
        configured.0
    );
}

/// The busiest launch's settings at the long default path stay under 150 KB with its one
/// spelling (the canonical path, which a launch configured at the same place has alone); two
/// spellings (an 8.3 one beside it) double the `Read` and `Edit` rules and are reported.
#[test]
fn the_settings_for_the_long_path_stay_small() {
    let dir = TempDir::new("fixed-shell-size");
    let (_, one_shell, one) = counts(&dir.path().join("1"), &[LONG], 30, 50, 5);
    let (_, two_shell, two) = counts(&dir.path().join("2"), &[LONG, LONG_83], 30, 50, 5);
    eprintln!(
        "long path: one spelling {one_shell} shell denies, {one} B; two {two_shell}, {two} B"
    );
    assert!(one < MOST_BYTES, "{one}");
    assert!(two < 2 * MOST_BYTES, "{two}");
}

/// A file under each target, as a command names it under `<data>` written `lead` with `sep`.
fn commands(lead: &str, sep: char) -> Vec<String> {
    let lead = lead.replace(['\\', '/'], &sep.to_string());
    let paths = [
        "agents/codex-acp/home/auth.json",
        "agents/signin/shim.js",
        "agents/claude-acp/login-source",
        "agents/claude-acp/home/.credentials.json",
        "agents/claude-acp/home/.claude.json",
        "run/01a1d5c4-90d5-7548-96d0-4a1610744900-1/relay-token",
        "run/01a1d5c4-90d5-7548-96d0-4a1610744900-1/proxy-secret",
        "launch-settings/other-run.json",
        "settings.json",
        "state.sqlite",
        "search.sqlite",
    ];
    assert_eq!(paths.len(), TARGETS.len());
    paths
        .iter()
        .map(|path| format!("{lead}{sep}{}", path.replace('/', &sep.to_string())))
        .collect()
}

/// Each target is denied in every form: each spelling of `<data>` the launch has (and, for the
/// default folder, any path ending in its name), each separator, both shells, quoted or not.
#[test]
fn each_target_is_denied_in_every_form() {
    let cases: [(&[&str], &[&str]); 3] = [
        (
            &[LONG, LONG_83],
            &[LONG, LONG_83, r"%APPDATA%\ai.clauseo.desktop"],
        ),
        (&[SHORT], &[SHORT, "ai.clauseo.desktop"]),
        (&[CONFIGURED, CONFIGURED_83], &[CONFIGURED, CONFIGURED_83]),
    ];
    for (spellings, written) in cases {
        let deny = LaunchDeny {
            spellings: spellings.iter().map(PathBuf::from).collect(),
            ..LaunchDeny::default()
        };
        let rules = windows_fixed_shell_deny_rules(&deny);
        let leads = leads(&deny).len();
        assert_eq!(rules.len(), leads * 2 * TARGETS.len() * 2, "{rules:#?}");
        for lead in written {
            for sep in ['\\', '/'] {
                for path in commands(lead, sep) {
                    for (tool, verb) in [("PowerShell", "Get-Content"), ("Bash", "cat")] {
                        for command in [format!("{verb} {path}"), format!("{verb} \"{path}\"")] {
                            assert!(denied_by(&rules, tool, &command), "{tool}: {command}");
                        }
                    }
                }
            }
        }
    }
}

/// The own folders, other conversations' work folders, other matters' notes and the plans
/// folder are in no shell rule: they are the classifier's and the `Read` and `Edit` rules'.
#[test]
fn what_is_not_a_secret_is_left_to_the_classifier() {
    let deny = LaunchDeny {
        spellings: vec![PathBuf::from(LONG)],
        ..LaunchDeny::default()
    };
    let rules = windows_fixed_shell_deny_rules(&deny);
    for path in [
        format!(r"run\{OWN_RUN}\work\draft.md"),
        format!(r"agent-state\{OWN_MATTER}\notes\notes.md"),
        format!(r"agent-state\{OWN_MATTER}\work\{OWN_THREAD}\draft.md"),
        r"agent-state\01a1d5b4-a291-7649-9fa4-000000000001\notes\notes.md".to_owned(),
        r"agents\claude-acp\home\plans\plan.md".to_owned(),
        r"agents\claude-acp\home\projects\C--x\s\tool-results\r.txt".to_owned(),
        r"tools\agent-browser\agent-browser.exe".to_owned(),
    ] {
        let command = format!("Get-Content \"{LONG}\\{path}\"");
        assert!(!denied_by(&rules, "PowerShell", &command), "{command}");
    }
}

/// A Windows launch's rules name this matter's history alone, in its fixed PowerShell rules:
/// 12 allow and 16 deny for one spelling, the same for 3 matters or 30.
#[test]
fn a_windows_launch_has_this_matters_history_rules_alone() {
    let dir = TempDir::new("fixed-shell-history");
    let history = |rules: Vec<String>| -> Vec<String> {
        rules
            .into_iter()
            .filter(|rule| rule.contains("history.git"))
            .collect()
    };
    let mut seen = Vec::new();
    for matters in [3, 30] {
        let deny = launch(
            &dir.path().join(matters.to_string()),
            &[LONG],
            matters,
            2,
            1,
        );
        let settings = windows_settings(&deny);
        let (allow, deny) = (
            history(rules(&settings, "allow")),
            history(rules(&settings, "deny")),
        );
        assert_eq!((allow.len(), deny.len()), (12, 16), "{allow:#?} {deny:#?}");
        for rule in allow.iter().chain(&deny) {
            assert!(rule.starts_with("PowerShell(git --git-dir="), "{rule}");
            assert!(rule.contains(OWN_MATTER), "{rule}");
        }
        seen.push((allow, deny));
    }
    assert_eq!(seen[0], seen[1]);
}
