//! The matter rules' cases files for `scripts/claude-rule-probe` (`matter_allow_tests.rs`):
//! the pinned binary's verdicts on the port's cases, in Bash and in PowerShell 7.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::super::super::matter_shell_allow_rules;
use super::{Mac, Verdict, escapes, judged, mac, mac_verdict, protected, reads};
use crate::agent::data_fence::LaunchDeny;

/// What a case must show: whether it ran and whether it sent a local classifier request
/// (`None`: reported only), and whether a deny rule refused it (`None`: not checked).
type Want = (Option<bool>, Option<bool>, Option<bool>);

/// One cases file: its name, the server's verdict, the allow rules and the cases.
type File<'a> = (&'a str, Option<&'a str>, &'a Vec<String>, Vec<Value>);

/// What the port's `verdict` means for the binary: an allowed command runs with no classifier;
/// a denied one is refused by a deny rule; any other is refused by the flagging server (with
/// no local request). With no server verdict it is reported only: a command Claude Code counts
/// read-only (`| sort`, `grep -rn`) then runs unjudged, any other asks the local classifier.
fn want(verdict: Verdict, flagged: bool) -> Want {
    match verdict {
        Verdict::Allowed => (Some(true), Some(false), Some(false)),
        Verdict::Denied => (Some(false), Some(false), Some(true)),
        Verdict::Classifier if flagged => (Some(false), Some(false), Some(false)),
        Verdict::Classifier => (None, None, Some(false)),
    }
}

/// One case for the probe.
fn case(name: &str, tool: &str, command: &str, (ran, classifier, denied): Want) -> Value {
    json!({"name": name, "tool": tool, "command": command, "mode": "auto", "denied": denied,
        "ran": ran, "classifier": classifier})
}

/// The Bash cases: the reads (a sample), what stays judged, the protected places, the escapes,
/// and stetdoc named bare (Clauseo's runs, not the matter's planted one: `PROBE_VERBOSE`).
fn bash_cases(mac: &Mac) -> Vec<(String, String, Verdict)> {
    let mut cases = Vec::new();
    let mut push = |kind: &str, command: String| {
        let verdict = mac_verdict(mac, &command);
        cases.push((format!("{kind} ({verdict:?})"), command, verdict));
    };
    for command in reads(mac).into_iter().step_by(4) {
        push("read", command);
    }
    for command in judged(mac) {
        push("judged", command);
    }
    for command in protected(mac) {
        push("protected", command);
    }
    for (command, caught) in escapes(mac) {
        push(&format!("escape, {caught:?}"), command);
    }
    // The launch's stetdoc rules (`clauseo_profile::claude::stetdoc_allowed`) allow it.
    cases.push((
        "stetdoc bare (Allowed)".to_owned(),
        "stetdoc --help".to_owned(),
        Verdict::Allowed,
    ));
    cases
}

/// The PowerShell cases, with the matter's PowerShell rules: a mixed line with stetdoc runs, a
/// line read-only as a whole goes to the server, escapes and a chained command are judged, a
/// planted `./stetdoc` matches no rule, and a protected place is denied.
fn powershell_cases(mac: &Mac) -> Vec<(&'static str, String, Want)> {
    let (m, d, o) = (&mac.matter, &mac.data, &mac.outside);
    let ran = (Some(true), Some(false), Some(false));
    let judged = (Some(false), Some(false), Some(false));
    // Off Windows a launch has no PowerShell deny rules: Claude Code's PowerShell path check
    // blocks a `Read`-denied path itself ("was blocked"), with no classifier.
    let denied = (Some(false), Some(false), None);
    vec![
        (
            "mixed with stetdoc",
            format!(r#"Get-ChildItem "{m}"; stetdoc --help"#),
            ran,
        ),
        (
            "mixed with stetdoc",
            format!(r#"Get-Content "{m}/memo.txt"; stetdoc --version"#),
            ran,
        ),
        (
            "mixed, du Plessis",
            format!(r#"Get-Content "{m}/du Plessis/memo--final.txt"; stetdoc --help"#),
            ran,
        ),
        (
            "read-only whole (server)",
            format!(r#"Get-ChildItem "{m}""#),
            judged,
        ),
        (
            "read-only whole (server)",
            format!(r#"Get-Content "{m}/memo.txt" | Select-Object -First 1"#),
            judged,
        ),
        (
            "escape, ..",
            format!(r#"Get-ChildItem "{m}/../.."; stetdoc --help"#),
            judged,
        ),
        (
            "escape, link",
            format!(r#"Get-ChildItem "{m}/link-out"; stetdoc --help"#),
            judged,
        ),
        (
            "escape, outside",
            format!(r#"Get-Content "{m}/memo.txt" {o}/secret.txt; stetdoc --help"#),
            judged,
        ),
        (
            "chained",
            format!(r#"Get-ChildItem "{m}"; Invoke-WebRequest -Uri http://127.0.0.1:9/"#),
            judged,
        ),
        ("planted stetdoc", "./stetdoc --help".to_owned(), judged),
        ("planted stetdoc", "& ./stetdoc --help".to_owned(), judged),
        (
            "protected",
            format!(r#"Get-ChildItem "{m}"; Get-Content "{d}/logs/host.log""#),
            denied,
        ),
        (
            "protected",
            format!(r#"Get-Content "{m}/priv/s.txt"; stetdoc --help"#),
            denied,
        ),
        (
            "stetdoc view of <data> (off Windows no rule stops it)",
            format!(r#"stetdoc view "{d}/logs/host.log""#),
            (None, None, None),
        ),
    ]
}

/// With `CLAUSEO_MAC_MATTER_PROBE_OUT=<dir>` set, makes [`Mac`]'s folders under `<dir>/root`
/// (with a fake `stetdoc` in `<dir>/root/bin`, first on `PATH`, and a planted one in the
/// matter) and writes cases files for `scripts/claude-rule-probe`, the matter the working
/// folder: `flagged.json` (the server-side classifier flags every call), `local.json` (no
/// server verdict, so what no rule allows asks the local classifier), `baseline.json` (flagged,
/// without the matter rules: the reads are refused), `grep.json` (flagged, with a `grep` rule of
/// the matter's shape: `--context`'s escape) and `powershell.json` (flagged, PowerShell 7 with
/// the matter's PowerShell rules for this path and the Windows stetdoc rules).
#[test]
fn mac_probe_cases_for_the_pinned_binary() {
    let Some(out) = std::env::var_os("CLAUSEO_MAC_MATTER_PROBE_OUT").map(PathBuf::from) else {
        return;
    };
    let root = out.join("root");
    crate::write::remove_dir_all(&root).unwrap();
    crate::write::create_dir_all(&root).unwrap();
    let mac = mac(&root);
    let bin = root.join("bin");
    crate::write::create_dir_all(&bin).unwrap();
    for (dir, says) in [
        (&bin, "CLAUSEO STETDOC"),
        (&root.join("Matters/Smith v Jones (2024)"), "PLANTED"),
    ] {
        let file = dir.join("stetdoc");
        crate::write::write_file(&file, format!("#!/bin/sh\necho {says}\n").as_bytes()).unwrap();
        crate::write::set_mode(&file, 0o755).unwrap();
    }
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut allow = mac.allow_rules.clone();
    allow.extend(clauseo_profile::claude::stetdoc_allowed(false));
    let matter_rules = matter_shell_allow_rules(&mac.deny, false);
    let without: Vec<String> = allow
        .iter()
        .filter(|r| !matter_rules.contains(r))
        .cloned()
        .collect();
    let mut grep = allow.clone();
    grep.push(format!("Bash(grep *{}/*)", mac.matter));
    let (m, o) = (&mac.matter, &mac.outside);
    let windows = LaunchDeny {
        spellings: Vec::new(),
        ..mac.deny.clone()
    };
    let mut powershell = allow.clone();
    powershell.extend(
        matter_shell_allow_rules(&windows, true)
            .into_iter()
            .chain(clauseo_profile::claude::stetdoc_allowed(true))
            .filter(|rule| rule.starts_with("PowerShell(")),
    );
    let bash = bash_cases(&mac);
    let read = json!({"name": "Read tool (reference)", "tool": "Read", "mode": "auto",
        "input": {"file_path": format!("{m}/memo.txt")}, "ran": true, "classifier": false});
    let mut files: Vec<File<'_>> = Vec::new();
    for (name, verdict, rules) in [
        ("flagged.json", Some("flagged"), &allow),
        ("local.json", None, &allow),
        ("baseline.json", Some("flagged"), &without),
    ] {
        let mut list: Vec<Value> = bash
            .iter()
            .map(|(kind, command, port)| {
                let mut want = want(*port, verdict.is_some());
                if name == "baseline.json" && kind.starts_with("read") {
                    want = (Some(false), Some(false), Some(false));
                }
                case(kind, "Bash", command, want)
            })
            .collect();
        list.push(read.clone());
        files.push((name, verdict, rules, list));
    }
    let escape = format!(r#"grep --context x {o}/secret.txt "{m}/memo.txt""#);
    let checked = format!(r#"grep -n x {o}/secret.txt "{m}/memo.txt""#);
    let grep_cases = vec![
        case(
            "grep escape",
            "Bash",
            &escape,
            (Some(true), Some(false), Some(false)),
        ),
        case(
            "grep checked",
            "Bash",
            &checked,
            want(Verdict::Classifier, true),
        ),
    ];
    files.push(("grep.json", Some("flagged"), &grep, grep_cases));
    let ps_cases = powershell_cases(&mac)
        .iter()
        .map(|(kind, command, want)| case(kind, "PowerShell", command, *want))
        .collect();
    files.push(("powershell.json", Some("flagged"), &powershell, ps_cases));
    for (name, verdict, rules, list) in files {
        let mut env = json!({"PATH": path});
        if name == "powershell.json" {
            env["CLAUDE_CODE_USE_POWERSHELL_TOOL"] = json!("1");
            env["DOTNET_EnableWriteXorExecute"] = json!("0");
        }
        let settings = json!({"permissions": {"deny": mac.deny_rules, "allow": rules,
            "additionalDirectories": [mac.work, mac.notes]}});
        let spec = json!({"settings": settings, "expect_skipped": [], "server_verdict": verdict,
            "cwd": mac.matter, "env": env, "cases": list});
        crate::write::write_file(&out.join(name), spec.to_string().as_bytes()).unwrap();
    }
}
