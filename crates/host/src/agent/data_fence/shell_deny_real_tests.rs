//! The Windows shell deny rules for a data folder like the laptop's in the Windows VM's
//! 60-matter run (#1035's B1 regression): the own matter's `UUIDv7` id beside seeded matters with
//! random v4 ids (several seeds), the laptop's run folder and Claude project keys. The VM's four
//! denied commands and its seven earlier ones pass, the own folders written every way pass, and
//! the protected places stay denied. And the size of the worst realistic launch.

use std::ffi::OsString;
use std::path::PathBuf;

use super::cross_tests::{
    Rules, default_leads, generated_over, lead_sep, own_commands_over, own_folders,
};
use super::tests::parse;
use super::{windows_shell_deny_rules, windows_shell_deny_rules_without_variables};
use crate::agent::data_fence::big_folder::{LAPTOP_MATTER, LAPTOP_RUN, laptop};
use crate::agent::data_fence::{LaunchDeny, claude_deny_rules_on};
use crate::pipeline::tests::support::TempDir;

/// The laptop's default data folder.
const DATA: &str = r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop";

/// The laptop's matter folder.
const MATTER: &str = r"C:\Users\clauseo-lawyer\Documents\Launch Matter";

/// The VM's commands, own folders only: the four the 60-matter run denied (B1), then the seven
/// of the first VM run (`scratch-S18/win-deny-commands.txt`), with the laptop's ids.
fn vm_commands() -> Vec<(&'static str, String)> {
    let notes = format!(r"{DATA}\agent-state\{LAPTOP_MATTER}\notes");
    let work = format!(r"{DATA}\run\{LAPTOP_RUN}\work");
    let notes_fwd = notes.replace('\\', "/");
    let work_fwd = work.replace('\\', "/");
    let memo = format!(r"{MATTER}\Memo on Shortening Notice Period.docx");
    vec![
        ("Bash", format!(r#"ls "{notes}"; ls -la "{MATTER}""#)),
        (
            "PowerShell",
            format!(
                r#"$d="{work}\dl"; New-Item -ItemType Directory -Force -Path $d | Out-Null; Invoke-WebRequest -Uri "https://www.indiacode.nic.in/x.pdf" -OutFile "$d\ica.pdf""#
            ),
        ),
        (
            "PowerShell",
            format!(r#"$w="{work}"; pandoc "$w/memo.md" -o "{memo}""#),
        ),
        (
            "PowerShell",
            format!(r#"pandoc "{work}/memo.md" -o "{memo}""#),
        ),
        (
            "Bash",
            format!(r#"ls "{notes_fwd}"; ls; pandoc "Notes.docx" -t plain"#),
        ),
        (
            "PowerShell",
            format!(r#"Get-ChildItem "{notes}"; Get-ChildItem; stetdoc skills get core"#),
        ),
        (
            "Bash",
            format!(r#"ls "{notes_fwd}"; ls; stetdoc skills get core"#),
        ),
        (
            "Bash",
            format!(
                r#"ls -la "{notes_fwd}"; echo ---; ls -laR "{}"; echo ---; ls -la "{work_fwd}""#,
                MATTER.replace('\\', "/")
            ),
        ),
        (
            "PowerShell",
            format!(
                r#"pandoc "{work}/reply.md" -o "Reply to legal notice.docx"; if ($?) {{ Remove-Item "{work}\reply.md" }}"#
            ),
        ),
        (
            "PowerShell",
            format!(r#"pandoc "{work}/reply.md" -o "Reply to legal notice.docx""#),
        ),
        (
            "Bash",
            format!(r#"head -100; echo ---; ls -la "{notes_fwd}"; echo ---; ls -la "{work_fwd}""#),
        ),
    ]
}

/// Commands that name the data folder or a folder on the way to the own ones with a separator
/// at the end, in both shells: a rule for them ends in a backslash, which must not escape its
/// own closing `)` (Claude Code would skip it as malformed). And ones that reach a folder
/// through a stream (`::$INDEX_ALLOCATION`), whose rules end in `:**`: `:*` would be Claude
/// Code's legacy prefix syntax, which never matches.
fn ending_in_a_separator() -> Vec<(&'static str, String)> {
    let forward = DATA.replace('\\', "/");
    let escaped = DATA.replace('\\', r"\\");
    let commands = [
        // Stage C's streams of a folder (`:**`, not the legacy prefix rule `:*`).
        format!(r"cat '{DATA}::$INDEX_ALLOCATION\settings.json'"),
        format!("cat {forward}::x/settings.json"),
        format!(r"cat '{DATA}\run::$INDEX_ALLOCATION\01a1d672-1c0e-7d3b-8e11-5a2b9c0d4e6f-2\x'"),
        format!(r"cat '{DATA}\agents\claude-acp\home::$INDEX_ALLOCATION\CLAUDE.md'"),
        format!(r"ls {DATA}\"),
        format!(r"Remove-Item -Recurse -Force {DATA}\"),
        format!(r"Remove-Item -Recurse -Force {DATA}\agent-state\"),
        format!(r"rm -rf {DATA}\run\"),
        format!("rm -rf {forward}/"),
        format!("rm -rf {forward}/agent-state/"),
        format!(r"ls {escaped}\\"),
        format!(r"ls {escaped}\\agent-state\\"),
        r"Remove-Item -Recurse $env:LOCALAPPDATA\ai.clauseo.desktop\".to_owned(),
    ];
    commands
        .into_iter()
        .flat_map(|command| [("Bash", command.clone()), ("PowerShell", command)])
        .collect()
}

/// For three seeds of 60 seeded matters: the VM's commands pass, the own folders written with
/// every lead, separator run and `.\` step pass, and the protected places, a seeded matter's
/// notes and another run among them, are denied.
#[test]
fn a_laptop_like_folder_lets_the_own_folders_through_and_denies_the_rest() {
    for seed in [1, 2, 3] {
        let dir = TempDir::new(&format!("shell-deny-laptop-{seed}"));
        let (deny, own_key, ids) = laptop(dir.path(), seed, 60, "clauseo-lawyer");
        let deny = LaunchDeny {
            spellings: vec![PathBuf::from(DATA)],
            ..deny
        };
        let all = windows_shell_deny_rules(&deny, &super::cross_tests::env);
        let rules = Rules::new(&all);
        let own = own_folders(LAPTOP_RUN, LAPTOP_MATTER, &own_key);
        let mut passed = vm_commands();
        for lead in default_leads(false) {
            passed.extend(own_commands_over(&lead, lead_sep(&lead), 2, &own));
        }
        for (tool, command) in &passed {
            assert!(
                !rules.denies(tool, command),
                "seed {seed}: {tool} denies: {command:?}"
            );
        }
        let names = |path: &str| path.split('\\').map(str::to_owned).collect::<Vec<_>>();
        let protected = [
            names(r"agents\claude-acp\home\.credentials.json"),
            names(r"agents\claude-acp\home\CLAUDE.md"),
            names(r"agents\claude-acp\home\commands\go.md"),
            names(r"tools\x"),
            names(r"profile\claude-plugin\skills\x\SKILL.md"),
            names(&format!(r"agent-state\{}\notes\x", ids[0])),
            names(&format!(r"agent-state\{}\notes\x", ids[ids.len() - 1])),
            names(r"run\01a1d672-1c0e-7d3b-8e11-5a2b9c0d4e6f-2\x"),
            names(r"run\01a1d673-4d5a-776d-9061-201035c7d75e-2\x"),
            names(clauseo_settings::SETTINGS_FILE),
            names(r"logs\host.log"),
        ];
        let mut denied = generated_over(&default_leads(true), &protected, 2);
        denied.extend(ending_in_a_separator());
        for (tool, command) in &denied {
            assert!(
                rules.denies(tool, command),
                "seed {seed}: {tool} passes: {command:?}"
            );
        }
        if seed == 1 {
            probe_cases(&deny, &all, &passed, &denied);
        }
    }
}

/// With `CLAUSEO_RULE_PROBE_OUT` set, writes a cases file for `scripts/claude-rule-probe`: the
/// whole permission settings of a laptop launch (`deny`'s Read and Edit rules, its shell rules
/// `rules`, its allow rules), every one of which the binary must accept; then the VM's
/// commands, the commands ending in a separator and a sample of the own and protected
/// spellings (Bash's), each with what the port says, so the pinned binary can be asked.
fn probe_cases(
    deny: &LaunchDeny,
    rules: &[String],
    passed: &[(&str, String)],
    denied: &[(&str, String)],
) {
    let Some(out) = std::env::var_os("CLAUSEO_RULE_PROBE_OUT") else {
        return;
    };
    let mut every = claude_deny_rules_on(deny, true);
    every.extend(rules.iter().cloned());
    let allow = super::super::claude_allow_rules(deny);
    let mut cases = Vec::new();
    for (n, (_, command)) in vm_commands().iter().enumerate() {
        cases.push(
            serde_json::json!({"name": format!("VM {}", n + 1), "tool": "Bash",
            "command": command, "denied": false}),
        );
    }
    let mut sample = |list: &[(&str, String)], denied: bool| {
        let bash: Vec<&String> = list
            .iter()
            .filter(|(tool, _)| *tool == "Bash")
            .map(|(_, command)| command)
            .collect();
        for command in bash.iter().step_by((bash.len() / 40).max(1)) {
            // An unquoted `\` after a variable: the binary may match Bash's parse, which drops
            // it, as Bash does when it runs (`scripts/claude-rule-probe/README.md`). Not checked.
            let quoted = command.contains('"') || command.contains('\'');
            let variable = command
                .split('$')
                .skip(1)
                .any(|after| after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '{'));
            let (name, expected) = match (denied, variable && !quoted && command.contains('\\')) {
                (true, true) => ("protected, unquoted \\ after a variable", None),
                (true, false) => ("protected", Some(true)),
                (false, _) => ("own", Some(false)),
            };
            cases.push(
                serde_json::json!({"name": name, "tool": "Bash", "command": command,
                "denied": expected}),
            );
        }
    };
    sample(passed, false);
    sample(denied, true);
    for (_, command) in ending_in_a_separator()
        .iter()
        .filter(|(tool, _)| *tool == "Bash")
    {
        cases.push(
            serde_json::json!({"name": "ending in a separator or a stream", "tool": "Bash",
            "command": command, "denied": true}),
        );
    }
    let settings = serde_json::json!({"permissions": {"deny": every, "allow": allow}});
    let spec = serde_json::json!({"settings": settings, "expect_skipped": [], "cases": cases});
    crate::write::write_file(std::path::Path::new(&out), spec.to_string().as_bytes()).unwrap();
}

/// A long profile's variables.
fn long_env(name: &str) -> Option<OsString> {
    let home = r"C:\Users\alexandra.montgomery-smith";
    match name {
        "LOCALAPPDATA" => Some(format!(r"{home}\AppData\Local").into()),
        "APPDATA" => Some(format!(r"{home}\AppData\Roaming").into()),
        "USERPROFILE" | "HOME" => Some(home.into()),
        _ => None,
    }
}

/// The worst realistic launch's rules: a long profile, a configured data folder under it spelled
/// two ways (one in lower case), 1,000 seeded matters.
fn worst() -> LaunchDeny {
    let dir = TempDir::new("shell-deny-worst");
    let (deny, _, _) = laptop(dir.path(), 7, 1000, "alexandra.montgomery-smith");
    let configured = r"C:\Users\alexandra.montgomery-smith\AppData\Roaming\Clauseo Data";
    LaunchDeny {
        spellings: vec![
            PathBuf::from(configured),
            PathBuf::from(configured.to_lowercase()),
        ],
        ..deny
    }
}

/// Every rule a launch emits, Read and Edit, allow and shell (with the variable forms and
/// without), parses as Claude Code 2.1.293's settings loader reads it ([`parse`]): none is
/// skipped as malformed, ambiguous or empty, and none ends in `:*`, Claude Code's legacy prefix
/// syntax, which would load and never match. Over the laptop-like folder and the worst launch.
#[test]
fn claude_code_accepts_every_rule_a_launch_emits() {
    let dir = TempDir::new("shell-deny-parse");
    let (laptop_deny, _, _) = laptop(dir.path(), 1, 60, "clauseo-lawyer");
    let laptop_deny = LaunchDeny {
        spellings: vec![PathBuf::from(DATA)],
        ..laptop_deny
    };
    let default_env: &dyn Fn(&str) -> Option<OsString> = &super::cross_tests::env;
    let worst_env: &dyn Fn(&str) -> Option<OsString> = &long_env;
    for (deny, var) in [(laptop_deny, default_env), (worst(), worst_env)] {
        let mut every = claude_deny_rules_on(&deny, true);
        every.extend(claude_deny_rules_on(&deny, false));
        every.extend(super::super::claude_allow_rules(&deny));
        every.extend(windows_shell_deny_rules(&deny, var));
        every.extend(windows_shell_deny_rules_without_variables(&deny, var));
        assert!(every.len() > 1000, "{}", every.len());
        for rule in &every {
            let read = parse(rule);
            assert_eq!(read.as_ref().err(), None, "skipped: {rule}");
            // `…:*` would be a legacy prefix rule, which no command matches.
            let ends_as_prefix = read.is_ok_and(|(_, content)| content.ends_with(":*"));
            assert!(!ends_as_prefix, "a prefix rule: {rule}");
        }
    }
}

/// The worst realistic launch: a long profile, a configured data folder under it spelled two
/// ways (one in lower case), 1,000 seeded matters with random ids and realistic project keys.
/// Its rules (Read and Edit with the shell rules, as JSON) stay under the size past which a
/// launch sheds its variable forms (`agent::loads::profile::SHED_AT`, about 1.8 MiB).
#[test]
fn the_worst_realistic_launch_fits_the_settings_file() {
    let deny = worst();
    let size = |rules: &[String]| serde_json::to_string(rules).unwrap().len();
    let read_edit = size(&claude_deny_rules_on(&deny, true));
    let full = size(&windows_shell_deny_rules(&deny, &long_env));
    let shed = size(&windows_shell_deny_rules_without_variables(
        &deny, &long_env,
    ));
    eprintln!(
        "worst launch: {} targets; Read/Edit {read_edit} B; shell {full} B with the variable \
         forms, {shed} B without; total {} B, or {} B shed",
        deny.targets.len(),
        read_edit + full,
        read_edit + shed,
    );
    assert!(read_edit + full < crate::agent::loads::profile::SHED_AT);
    assert!(shed < full);
}

/// The shed decision (`loads::profile::shell_rules_within`), driven off Windows: within its
/// budget a launch keeps the variable forms; past it, it gets the rules without them.
#[test]
fn a_launch_past_its_budget_sheds_the_variable_forms() {
    use crate::agent::loads::profile::shell_rules_within;
    let deny = worst();
    let full = windows_shell_deny_rules(&deny, &long_env);
    let shed = windows_shell_deny_rules_without_variables(&deny, &long_env);
    let size = serde_json::to_string(&full).unwrap().len();
    assert_eq!(
        shell_rules_within(&deny, &long_env, 1000, 1000 + size),
        full
    );
    let past = shell_rules_within(&deny, &long_env, 1001, 1000 + size);
    assert_eq!(past, shed);
    let variable = |rule: &String| {
        ["%APPDATA%", "$env:", "~/", "$HOME"]
            .iter()
            .any(|v| rule.contains(v))
    };
    assert!(full.iter().any(variable));
    assert!(!past.iter().any(variable));
}
