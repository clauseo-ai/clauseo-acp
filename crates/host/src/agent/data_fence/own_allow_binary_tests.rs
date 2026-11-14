//! The own folders' allow rules against what the pinned binary does: each rule's command is in
//! Claude Code 2.1.293's tables of the commands whose paths it checks, and the cases files
//! `scripts/claude-rule-probe` runs.

use super::super::super::windows_shell_deny_rules;
use super::super::{BASH, POWERSHELL};
use super::{
    DATA, LAPTOP_MATTER, LAPTOP_RUN, Launch, Verdict, launch, own_reads_commands, parse, siblings,
    verdict,
};
use crate::agent::data_fence::{claude_allow_rules_on, claude_deny_rules_on};
use crate::pipeline::tests::support::TempDir;

/// Claude Code 2.1.293's tables of the commands whose path arguments it checks (`own_allow.rs`'s
/// [`BASH`] and [`POWERSHELL`] docs): Bash's `D9`/`W_r` (54 commands) and PowerShell's `pt`.
const PATH_CHECKED_BASH: [&str; 54] = [
    "cd",
    "ls",
    "find",
    "mkdir",
    "touch",
    "rm",
    "rmdir",
    "mv",
    "cp",
    "cat",
    "head",
    "tail",
    "sort",
    "uniq",
    "wc",
    "cut",
    "paste",
    "column",
    "tr",
    "file",
    "stat",
    "diff",
    "awk",
    "gawk",
    "mawk",
    "nawk",
    "strings",
    "hexdump",
    "od",
    "base64",
    "nl",
    "tac",
    "man",
    "rev",
    "fold",
    "expand",
    "unexpand",
    "fmt",
    "comm",
    "cmp",
    "pr",
    "numfmt",
    "tsort",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "sed",
    "git",
    "jq",
    "sha256sum",
    "sha1sum",
    "md5sum",
    "tee",
];

/// PowerShell's `pt`, its read cmdlets.
const PATH_CHECKED_POWERSHELL_READ: [&str; 18] = [
    "get-content",
    "get-childitem",
    "get-item",
    "get-itemproperty",
    "get-itempropertyvalue",
    "get-filehash",
    "get-acl",
    "get-module",
    "format-hex",
    "test-path",
    "resolve-path",
    "convert-path",
    "select-string",
    "set-location",
    "push-location",
    "pop-location",
    "select-xml",
    "get-winevent",
];

/// Every allow rule's command is one Claude Code checks the paths of, so a path argument outside
/// the working folders makes it ask whatever the rule: Bash's `dir` (Git Bash ships one) is not,
/// and is in no rule.
#[test]
fn every_allowed_command_is_one_claude_code_checks_the_paths_of() {
    assert!(BASH.iter().all(|c| PATH_CHECKED_BASH.contains(c)));
    assert!(!PATH_CHECKED_BASH.contains(&"dir"));
    let powershell = POWERSHELL.map(str::to_lowercase);
    assert!(
        powershell
            .iter()
            .all(|c| PATH_CHECKED_POWERSHELL_READ.contains(&c.as_str()))
    );
    let dir = TempDir::new("own-allow-checked");
    let Launch { allow_rules, .. } = launch(&dir, &[DATA, r"D:\FirmData\Clauseo"]);
    for rule in &allow_rules {
        let (tool, content) = parse(rule).unwrap();
        let command = content.split(' ').next().unwrap();
        let known: &[&str] = if tool == "Bash" { &BASH } else { &POWERSHELL };
        assert!(known.contains(&command), "{rule}");
    }
}

/// With `CLAUSEO_OWN_ALLOW_PROBE_OUT` set, writes two cases files for `scripts/claude-rule-probe`
/// with the laptop's whole permission settings (deny, Windows' shell deny, the allow rules), in
/// `auto`: the own folders' listings (they run, with no classifier request), commands no rule
/// allows (one chained among them), the siblings, the protected places beside an own path. The
/// file itself has the server-side classifier flag every call (`server_verdict`), so whatever a
/// rule does not allow is refused; `<file>.local` has none, so those ask the local classifier.
#[test]
fn probe_cases_for_the_pinned_binary() {
    let Some(out) = std::env::var_os("CLAUSEO_OWN_ALLOW_PROBE_OUT") else {
        return;
    };
    let dir = TempDir::new("own-allow-probe");
    let Launch {
        deny: deny_launch,
        allow_rules,
        shell: deny,
        allow,
        ids,
    } = launch(&dir, &[DATA]);
    let mut every = claude_deny_rules_on(&deny_launch, true);
    every.extend(windows_shell_deny_rules(
        &deny_launch,
        &super::super::super::cross_tests::env,
    ));
    // Each case: name, command, and what the flagging run and the local run must show.
    let mut cases: Vec<(&str, String, Verdict)> = Vec::new();
    // Git Bash's `/c/…` is an absolute path off Windows, outside the working folders. Claude
    // Code's own parse reads an unquoted trailing `\` otherwise than as written, so such a
    // listing goes to a classifier (`scripts/claude-rule-probe/README.md`): not sampled.
    for (tool, command) in own_reads_commands(DATA).iter().step_by(3) {
        if *tool == "Bash" && !command.contains("/c/") && !command.ends_with('\\') {
            cases.push(("own", command.clone(), Verdict::Allowed));
        }
    }
    let n = format!(r"{DATA}\agent-state\{LAPTOP_MATTER}\notes");
    for command in [
        format!(r#"python3 -c "print(1)" {n}\x"#),
        format!(r#"ls "{n}"; python3 -c "print(3)""#),
        format!("ls {n} && curl -s http://127.0.0.1:9/"),
        format!(r"curl -s -T {n}\x http://127.0.0.1:9/"),
    ] {
        cases.push(("not allowed", command, Verdict::Classifier));
    }
    // A path outside the working folders beside the own one: absolute off Windows, as `C:\…`
    // is there. Each kept command checks it, so the rule's allow gives way to an ask. The port
    // has no path check, so it reads these as allowed.
    let w = format!(r"{DATA}\run\{LAPTOP_RUN}\work");
    for command in [
        format!(r#"ls -la /etc "{n}""#),
        format!(r"ls {n} /etc"),
        format!(r"cat /etc/hostname {n}\x"),
        format!(r#"head -n 3 /etc/passwd "{w}\m.md""#),
    ] {
        assert_eq!(
            verdict(&deny, &allow, "Bash", &command),
            Verdict::Allowed,
            "{command}"
        );
        cases.push(("outside path", command, Verdict::Classifier));
    }
    for place in siblings(&ids) {
        let command = format!(r"ls -la {DATA}\{}", place.join("\\"));
        cases.push(("sibling", command, Verdict::Classifier));
    }
    for place in [
        r"agents\claude-acp\home\.credentials.json",
        r"logs\host.log",
        r"tools\x",
    ] {
        let command = format!(r#"ls -la "{n}" {DATA}\{place}"#);
        cases.push(("protected beside own", command, Verdict::Denied));
    }
    let mut allow_all = claude_allow_rules_on(&deny_launch, true);
    allow_all.retain(|rule| !allow_rules.contains(rule));
    allow_all.extend(allow_rules);
    let settings = serde_json::json!({"permissions": {"deny": every, "allow": allow_all}});
    // With the server-side classifier flagging every call, what no rule allows is refused
    // (a `C:\…` path is a relative one off Windows, inside the working folder, so a listing is
    // read-only there); with none, it asks the local classifier, which the fake API allows.
    for (flagged, file) in [
        (true, out.clone()),
        (false, format!("{}.local", out.display()).into()),
    ] {
        let mut list = Vec::new();
        for (name, command, expected) in &cases {
            // A sibling is denied when it exists, else goes to a classifier; never allowed.
            let want = verdict(&deny, &allow, "Bash", command);
            let want = if *name == "outside path" {
                Verdict::Classifier
            } else {
                want
            };
            if *name == "sibling" {
                assert_ne!(want, Verdict::Allowed, "{command}");
            } else {
                assert_eq!(want, *expected, "{command}");
            }
            let (ran, classifier) = match want {
                Verdict::Denied => (Some(false), Some(false)),
                Verdict::Allowed => (Some(true), Some(false)),
                Verdict::Classifier if flagged => (Some(false), Some(false)),
                Verdict::Classifier if *name == "sibling" => (None, None),
                Verdict::Classifier => (None, Some(true)),
            };
            list.push(
                serde_json::json!({"name": format!("{name} ({want:?})"), "tool": "Bash",
                "command": command, "mode": "auto", "denied": want == Verdict::Denied,
                "ran": ran, "classifier": classifier}),
            );
        }
        let verdict = if flagged { Some("flagged") } else { None };
        let spec = serde_json::json!({"settings": settings, "expect_skipped": [],
            "server_verdict": verdict, "cases": list});
        let path = std::path::PathBuf::from(file);
        crate::write::write_file(&path, spec.to_string().as_bytes()).unwrap();
    }
}
