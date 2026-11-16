//! The estimate against the fake-API measurements of Claude Code 2.1.293 (private
//! `hostfu/b4l-autoctx.md`): main's launch at 8 matters (372,259 B sent) and #1085's at 200
//! (943,153 B sent), each within 5%; its parts on small settings; the Claude Code pin the
//! factors were measured on; and that nothing but the estimate meets the limits.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::{
    ASK_MODE_AT, CONTEXT_LIMIT, TEST_BUDGET_AT_8, TEST_BUDGET_AT_200, estimate, settings_of,
};
use crate::agent::data_fence::big_folder::laptop;
use crate::agent::data_fence::{
    LaunchDeny, claude_allow_rules_on, claude_deny_rules_on, rule_path, windows_shell_deny_rules,
};
use crate::pipeline::tests::support::TempDir;

/// The laptop's default data folder (as `shell_deny_real_tests.rs`).
const DATA: &str = r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop";

/// The laptop's own matter folder.
const MATTER: &str = r"C:\Users\clauseo-lawyer\Documents\Launch Matter";

/// The laptop's variables (as `shell_deny_cross_tests.rs`).
fn env(name: &str) -> Option<OsString> {
    match name {
        "LOCALAPPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Local".into()),
        "APPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Roaming".into()),
        "USERPROFILE" | "HOME" => Some(r"C:\Users\clauseo-lawyer".into()),
        _ => None,
    }
}

/// Flag settings holding `deny` and `allow` as `permissions`.
fn settings(deny: &[String], allow: &[String]) -> Map<String, Value> {
    let value = json!({"permissions": {"deny": deny, "allow": allow}});
    value.as_object().cloned().unwrap_or_default()
}

/// Main's Windows launch in the laptop-like folder with `matters` registered matters (the own
/// one and `matters - 1` seeded): the data folder's Read and Edit denies, its shell denies, the
/// matter marker's two, and its allow rules, as `profile::deny_data_folder` builds them; with
/// the own folders' glob denies (`own_shell_glob_deny_rules`) only when `globs`, which came
/// after the measured render (f67787007).
fn main_launch(dir: &TempDir, matters: usize, globs: bool) -> (Vec<String>, Vec<String>) {
    let (deny, _, _) = laptop(dir.path(), 1, matters - 1, "clauseo-lawyer");
    let deny = LaunchDeny {
        spellings: vec![PathBuf::from(DATA)],
        ..deny
    };
    let mut rules = claude_deny_rules_on(&deny, true, !globs);
    for rule in windows_shell_deny_rules(&deny, &env) {
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    let marker = rule_path(MATTER, true);
    rules.push(format!("Edit({marker}/.clauseo)"));
    rules.push(format!("Edit({marker}/.clauseo/**)"));
    (rules, claude_allow_rules_on(&deny, true))
}

/// The shell content #1085's renderer (`shell_deny::render`) writes for `lead` followed and
/// led by anything: a `\` before a `*` or `\` doubled in the pattern, then each run of `n`
/// backslashes written `2n - 1` times.
fn any_around(lead: &str) -> String {
    let chars: Vec<char> = lead.chars().collect();
    let mut pattern = String::from("*");
    for (n, c) in chars.iter().enumerate() {
        if *c == '\\' {
            let doubled = matches!(chars.get(n + 1), None | Some('\\' | '*'));
            pattern.push_str(if doubled { "\\\\" } else { "\\" });
        } else {
            pattern.push(*c);
        }
    }
    pattern.push('*');
    let mut content = String::new();
    let mut it = pattern.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            content.push(c);
            continue;
        }
        let mut run = 1;
        while it.peek() == Some(&'\\') {
            it.next();
            run += 1;
        }
        content.extend(std::iter::repeat_n('\\', 2 * run - 1));
    }
    content
}

/// #1085's (b0d68aad7) rules for one other matter at `path` under the laptop's home, no name
/// a prefix of another: `Read` and `Edit` of the folder and all in it, and one shell rule per
/// way a command writes it (`shell_deny_forms::forms` with the home's variables: 10 `Bash`,
/// 9 `PowerShell`, which compares `%USERPROFILE%` and `%userprofile%` as one).
fn other_matter(path: &str) -> Vec<String> {
    let base = rule_path(path, true);
    let mut rules = vec![
        format!("Read({base})"),
        format!("Read({base}/**)"),
        format!("Edit({base})"),
        format!("Edit({base}/**)"),
    ];
    let home = r"C:\Users\clauseo-lawyer";
    let rest = path
        .strip_prefix(home)
        .unwrap_or(path)
        .trim_start_matches('\\');
    let fwd = rest.replace('\\', "/");
    let both = [
        path.to_owned(),
        path.replace('\\', "/"),
        path.replace('\\', "\\\\"),
        format!("%USERPROFILE%\\{rest}"),
        format!("$HOME/{fwd}"),
        format!("${{HOME}}/{fwd}"),
        format!("~/{fwd}"),
    ];
    let bash = [
        format!("/c/{}", path.get(3..).unwrap_or(path).replace('\\', "/")),
        format!("%userprofile%\\{rest}"),
        format!("{home}/{fwd}"),
    ];
    let powershell = [
        format!("$env:USERPROFILE\\{rest}"),
        format!("$env:USERPROFILE/{fwd}"),
    ];
    for lead in both.iter().chain(&bash) {
        rules.push(format!("Bash({})", any_around(lead)));
    }
    for lead in both.iter().chain(&powershell) {
        rules.push(format!("PowerShell({})", any_around(lead)));
    }
    rules
}

/// Within 5% of `measured`.
fn near(estimated: usize, measured: usize) -> bool {
    estimated.abs_diff(measured) * 20 <= measured
}

#[test]
fn main_at_8_matters_is_within_5_percent_of_what_claude_code_sent() {
    let dir = TempDir::new("auto-context-main-8");
    let (deny, allow) = main_launch(&dir, 8, false);
    // The measured render: 254 Read and Edit, 3,448 shell, 2 marker rules.
    assert_eq!(deny.len(), 3_704);
    let estimated = estimate(&settings(&deny, &allow));
    eprintln!("main-8 as measured: estimated {estimated}, sent 372,259");
    assert!(near(estimated, 372_259), "estimated {estimated}");
    // Main today adds the own folders' glob denies: still far under the budget.
    let (deny, allow) = main_launch(&dir, 8, true);
    let today = estimate(&settings(&deny, &allow));
    eprintln!("main-8 today: {} deny rules, estimated {today}", deny.len());
    assert!(today <= TEST_BUDGET_AT_8, "estimated {today}");
}

#[test]
fn pr_1085_at_200_matters_is_within_5_percent_of_what_claude_code_sent() {
    let dir = TempDir::new("auto-context-pr-200");
    let (mut deny, allow) = main_launch(&dir, 200, false);
    assert_eq!(deny.len(), 3_864);
    let mut others = Vec::new();
    for n in 1..200 {
        others.extend(other_matter(&format!(
            r"C:\Users\clauseo-lawyer\Documents\Matters\Seeded Matter {n:02}"
        )));
    }
    // #1085's render: 796 Read and Edit and 3,781 shell rules of other matters, 321,000 B of
    // settings (55,723 + 265,178); the synthetic ones match it within 3%.
    assert_eq!(others.len(), 796 + 3_781);
    let bytes = serde_json::to_string(&others).map_or(0, |text| text.len());
    eprintln!("pr-200 other matters' rules: {bytes} B of settings, measured 320,901");
    assert!(bytes.abs_diff(320_901) * 33 <= 320_901, "{bytes} B");
    deny.extend(others);
    let estimated = estimate(&settings(&deny, &allow));
    eprintln!("pr-200 as measured: estimated {estimated}, sent 943,153");
    assert!(near(estimated, 943_153), "estimated {estimated}");
    assert!(estimated > ASK_MODE_AT && estimated < CONTEXT_LIMIT);
    // As measured, #1085 at 200 matters is over the budget its own test must hold it to.
    assert!(estimated > TEST_BUDGET_AT_200);
}

#[test]
fn allow_rules_count_nothing_and_ask_rules_count_as_deny_rules() {
    let rule = r"Bash(*C:\Users\x*)".to_owned();
    let empty = estimate(&settings(&[], &[]));
    let allowed = estimate(&settings(&[], &vec![rule.clone(); 100]));
    assert_eq!(allowed, empty);
    let denied = estimate(&settings(std::slice::from_ref(&rule), &[]));
    let value = json!({"permissions": {"ask": [rule]}});
    let asked = estimate(value.as_object().unwrap());
    assert_eq!(asked, denied);
    // `"Bash(*C:\\\\Users\\\\x*)"`: 26 B, then the 33 B wrapper.
    assert_eq!(denied - empty, 26 + 33);
}

#[test]
fn a_read_rule_costs_its_root_and_pattern_too() {
    let empty = estimate(&Map::new());
    let read = estimate(&settings(&["Read(//c/a/**)".to_owned()], &[]));
    // The rule's JSON (16) and wrapper (33), the root `"/"` (3), the pattern `"/c/a/**"` (9)
    // and their wrapper (20).
    assert_eq!(read - empty, 16 + 33 + 3 + 9 + 20);
}

#[test]
fn settings_are_found_in_the_session_meta_only_as_an_object() {
    let meta = json!({"claudeCode": {"options": {"settings": {"permissions": {}}}}});
    assert!(settings_of(meta.as_object().unwrap()).is_some());
    let moved = json!({"claudeCode": {"options": {"settings": "/data/launch-settings/r.json"}}});
    assert!(settings_of(moved.as_object().unwrap()).is_none());
}

/// The Claude Code the factors were measured on: `@anthropic-ai/claude-agent-sdk` 0.3.293,
/// whose binary is Claude Code 2.1.293, in the one home of the adapters' npm pins.
const MEASURED_SDK: &str = "0.3.293";

#[test]
fn the_factors_were_measured_on_the_pinned_claude_code() {
    let lock: Value =
        serde_json::from_str(include_str!("../../../limits/tests/live/package-lock.json")).unwrap();
    let pinned = lock["packages"]["node_modules/@anthropic-ai/claude-agent-sdk"]["version"]
        .as_str()
        .unwrap_or("none");
    assert_eq!(
        pinned, MEASURED_SDK,
        "Claude Code's pin moved: re-measure auto_context factors (its module doc says how)"
    );
}

/// The names of the limits, and their values written out.
const LIMITS: [&str; 9] = [
    "ASK_MODE_AT",
    "ASK_ON_BLIP_AT",
    "CONTEXT_LIMIT",
    "832_000",
    "832000",
    "1_040_000",
    "1040000",
    "900_000",
    "900000",
];

/// Whether `line` holds `word` with no word character either side.
fn names(line: &str, word: &str) -> bool {
    let word_char = |c: char| c.is_alphanumeric() || c == '_';
    line.match_indices(word).any(|(at, _)| {
        let before = line.get(..at).and_then(|text| text.chars().next_back());
        let after = line
            .get(at + word.len()..)
            .and_then(|text| text.chars().next());
        !before.is_some_and(word_char) && !after.is_some_and(word_char)
    })
}

/// Every `.rs` file under `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The context limit is met by the estimate alone: outside this module, a line that names a
/// limit (or writes one out) must name `estimate` too, so no one weighs settings bytes against
/// it again; `SHED_AT` is the settings file's own limit.
#[test]
fn only_the_estimate_meets_the_context_limits() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    sources(&root.join("tests"), &mut files);
    let mut met = 0;
    let mut wrong = Vec::new();
    for file in &files {
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.starts_with("auto_context") {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        for (n, line) in text.lines().enumerate() {
            if !LIMITS.iter().any(|limit| names(line, limit)) || line.trim_start().starts_with("//")
            {
                continue;
            }
            if line.contains("estimate") {
                met += 1;
            } else {
                wrong.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "a limit met by something but the estimate: {wrong:#?}"
    );
    // The ask-mode decision is one of them.
    assert!(met > 0, "no line meets a limit with the estimate");
}
