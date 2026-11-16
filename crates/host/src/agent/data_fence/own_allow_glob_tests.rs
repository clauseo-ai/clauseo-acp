//! The own folders' glob denies (`own_allow.rs`, "A glob") through the port of Claude Code
//! 2.1.293's parser and matcher: each rule loads; an allowed listing or read of an own folder
//! with a glob before or after its path is denied in every spelling the allow writes; a plain
//! own read, another command's glob and a matter glob with no own path are not; nor is #1098's
//! pinned PowerShell line; and the launch still fits its settings budget. Only a launch with no
//! shell sandbox (Windows, or Claude's sandbox `Off`) gets them.

use std::path::PathBuf;

use super::super::super::cross_tests::Rules;
use super::super::super::tests::denied_by;
use super::super::own_shell_glob_deny_rules;
use super::{
    DATA, LAPTOP_MATTER, LAPTOP_THREAD, Launch, MATTER, Verdict, launch, leads, own_reads_commands,
    parse, under, verdict,
};
use crate::agent::data_fence::{LaunchDeny, claude_allow_rules_on, claude_deny_rules_on};
use crate::pipeline::tests::support::TempDir;

/// A configured data folder, spelled as configured and in lower case.
const CONFIGURED: [&str; 2] = [r"D:\FirmData\Clauseo", r"d:\firmdata\clauseo"];

/// A PowerShell command's first word as Claude Code 2.1.293's `yt` compares it with a rule's:
/// a module qualifier dropped (`PJn`, for a deny), then an alias mapped to its cmdlet (`z_`),
/// the aliases `own_allow.rs` lists for the allowed cmdlets.
fn cmdlet(command: &str) -> String {
    let (first, rest) = command.split_once(' ').unwrap_or((command, ""));
    let name = first.rsplit('\\').next().unwrap_or(first).to_lowercase();
    let name = match name.as_str() {
        "gci" | "dir" | "ls" => "get-childitem",
        "gc" | "type" | "cat" => "get-content",
        _ => name.as_str(),
    };
    format!("{name} {rest}")
}

/// The verdict for `command` with the glob denies beside the launch's other deny rules: for
/// PowerShell, the worse of the command as written and with its first word mapped ([`cmdlet`]).
fn judged(deny: &Rules, allow: &Rules, tool: &str, command: &str) -> Verdict {
    let written = verdict(deny, allow, tool, command);
    if tool != "PowerShell" || written == Verdict::Denied {
        return written;
    }
    verdict(deny, allow, tool, &cmdlet(command))
}

/// The launch's shell deny rules with the glob denies, the glob denies alone, and the allow.
fn rules(spellings: &[&str]) -> (Rules, Rules, Rules, Vec<String>) {
    let dir = TempDir::new("own-glob");
    let Launch {
        deny, allow_rules, ..
    } = launch(&dir, spellings);
    let glob = own_shell_glob_deny_rules(&deny, true);
    let mut every = super::super::super::windows_shell_deny_rules(
        &deny,
        &super::super::super::cross_tests::env,
    );
    every.extend(glob.iter().cloned());
    (
        Rules::new(&every),
        Rules::new(&glob),
        Rules::new(&allow_rules),
        glob,
    )
}

/// Every glob deny loads as Claude Code reads it (`dCe`, `lYe`): none skipped, none ending in
/// `:*` or in an odd run of backslashes, each starting with an allowed command's name; and it
/// is in the launch's deny rules on Windows and off it (where only Bash's are written).
#[test]
fn every_glob_deny_loads_and_starts_with_an_allowed_command() {
    let dir = TempDir::new("own-glob-load");
    let base = launch(&dir, &[]).deny;
    let unix = crate::testing::abs("/Users/asha/Library/Application Support/ai.clauseo.desktop");
    for (spellings, windows) in [
        (vec![DATA], true),
        (CONFIGURED.to_vec(), true),
        (vec![unix.as_str()], false),
    ] {
        let deny = LaunchDeny {
            spellings: spellings.iter().map(PathBuf::from).collect(),
            ..base.clone()
        };
        let glob = own_shell_glob_deny_rules(&deny, windows);
        let all = claude_deny_rules_on(&deny, windows, false);
        assert!(!glob.is_empty() && glob.iter().all(|rule| all.contains(rule)));
        let size = serde_json::to_string(&glob).unwrap().len();
        eprintln!("{spellings:?}: {} glob denies, {size} B", glob.len());
        for rule in &glob {
            let (tool, content) = parse(rule).unwrap_or_else(|why| panic!("{why}: {rule}"));
            assert!(!content.ends_with(":*"), "a prefix rule: {rule}");
            let raw = rule.get(tool.len() + 1..rule.len() - 1).unwrap();
            let run = raw.len() - raw.trim_end_matches('\\').len();
            assert!(run % 2 == 0, "an odd backslash run at the end: {rule}");
            let commands: &[&str] = match tool.as_str() {
                "Bash" => &["ls *", "cat *", "head *"],
                "PowerShell" if windows => &["Get-ChildItem *", "Get-Content *"],
                _ => panic!("{rule}"),
            };
            assert!(commands.iter().any(|c| content.starts_with(c)), "{rule}");
            assert!(
                ["\\*", "?", "["].iter().any(|g| content.contains(g)),
                "{rule}"
            );
        }
    }
}

/// The exact rules for one path and command, as Claude Code reads them: the glob a literal
/// (`\*`, `?`, `[`), Claude's `*` on each side, each backslash of the path written for `Lho`.
#[test]
fn the_glob_deny_shapes_are_exact() {
    let (_, _, _, glob) = rules(&[DATA]);
    let notes = format!(r"{DATA}\agent-state\{LAPTOP_MATTER}\notes");
    let slashed = notes.replace('\\', "/");
    // Bash's `C:\\…`: two backslashes before a letter, which `Lho` reads from five (`hQ` then
    // sees `\\\`: an escaped backslash, then one before a letter, itself).
    let escaped = notes.replace('\\', r"\\\\\");
    let drive = format!("/c{}", slashed.get(2..).unwrap());
    for rule in [
        format!(r"Bash(cat *\**{notes}*)"),
        format!(r"Bash(cat *{notes}*\**)"),
        format!("Bash(cat *?*{notes}*)"),
        format!("Bash(head *{notes}*[*)"),
        format!("Bash(ls *[*{slashed}*)"),
        format!(r"Bash(ls *{escaped}*\**)"),
        format!("Bash(cat *{drive}*?*)"),
        format!(r"PowerShell(Get-Content *{notes}*\**)"),
        format!(r"PowerShell(Get-ChildItem *\**{slashed}*)"),
    ] {
        assert!(glob.contains(&rule), "{rule}");
    }
    let (_, content) = parse(&format!(r"Bash(ls *{escaped}*\**)")).unwrap();
    assert_eq!(content, format!(r"ls *{}*\**", notes.replace('\\', r"\\\")));
    assert_eq!(
        glob.len(),
        2 * 3 * (2 * 4 * 3 + 2 * 2 * 2),
        "{}",
        glob.len()
    );
}

/// An allowed listing or read of an own folder with a glob before or after its path, in every
/// lead of every spelling the allow writes, is matched by the allow and denied by a glob deny.
#[test]
fn an_own_folder_glob_is_denied_in_every_spelling() {
    let mut total = 0;
    for spellings in [vec![DATA], CONFIGURED.to_vec()] {
        let (deny, glob, allow, _) = rules(&spellings);
        for spelling in &spellings {
            for (lead, sep, powershell) in leads(spelling) {
                let notes = under(&lead, sep, &["agent-state", LAPTOP_MATTER, "notes"]);
                let work = under(
                    &lead,
                    sep,
                    &["agent-state", LAPTOP_MATTER, "work", LAPTOP_THREAD],
                );
                let mut commands = vec![
                    ("Bash", format!(r#"cat "{notes}"/*/x"#)),
                    ("Bash", format!("cat {notes}{sep}j?{sep}x")),
                    ("Bash", format!("cat {notes}{sep}[a-z]{sep}x")),
                    ("Bash", format!(r#"cat "{MATTER}"/*/x {notes}"#)),
                    ("Bash", format!("ls -la {work}{sep}*")),
                    ("Bash", format!("head -n 5 '{work}'/?.md")),
                ];
                if powershell {
                    commands.extend([
                        (
                            "PowerShell",
                            format!(r#"Get-Content "{notes}{sep}*{sep}x""#),
                        ),
                        ("PowerShell", format!(r"gci {work}{sep}* ")),
                        ("PowerShell", format!(r"gc {notes}{sep}[ab]{sep}x")),
                        ("PowerShell", format!(r#"type "{MATTER}\*\x" {notes}"#)),
                        ("PowerShell", format!("Get-ChildItem -Force {work}{sep}?")),
                    ]);
                }
                for (tool, command) in &commands {
                    let mapped = if *tool == "PowerShell" {
                        cmdlet(command)
                    } else {
                        command.clone()
                    };
                    assert!(
                        allow.denies(tool, &mapped),
                        "not allowed: {tool}: {command}"
                    );
                    assert!(glob.denies(tool, &mapped), "{tool}: {command}");
                    let got = judged(&deny, &allow, tool, command);
                    assert_eq!(got, Verdict::Denied, "{tool}: {command}");
                    total += 1;
                }
            }
        }
    }
    eprintln!("own globs: {total} commands, every one denied");
    // Off Windows, the path with `/`.
    let dir = TempDir::new("own-glob-unix");
    let unix = crate::testing::abs("/Users/asha/Library/Application Support/ai.clauseo.desktop");
    let deny = LaunchDeny {
        spellings: vec![PathBuf::from(&unix)],
        ..launch(&dir, &[]).deny
    };
    let glob = Rules::new(&own_shell_glob_deny_rules(&deny, false));
    let notes = format!("{unix}/agent-state/{LAPTOP_MATTER}/notes");
    for command in [
        format!(r#"cat "{notes}"/*/x"#),
        format!("cat {notes}/j?/x"),
        format!("cat {notes}/[a-z]/x"),
        format!(r#"cat "{unix}/../Matter"/*/x {notes}"#),
    ] {
        assert!(glob.denies("Bash", &command), "{command}");
    }
}

/// Not denied: an own read with no glob (every own read the allow tests check stays allowed),
/// a glob in a command that names no own path (Claude Code's classifier keeps that), and a
/// command with a glob that no allow rule names (it was never allowed and still goes to one).
#[test]
fn a_plain_own_read_and_other_globs_are_not_denied() {
    for spellings in [vec![DATA], CONFIGURED.to_vec()] {
        let (deny, glob, allow, _) = rules(&spellings);
        let mut allowed = Vec::new();
        for spelling in &spellings {
            allowed.extend(own_reads_commands(spelling));
        }
        let notes = format!(r"{}\agent-state\{LAPTOP_MATTER}\notes", spellings[0]);
        let work = format!(
            r"{}\agent-state\{LAPTOP_MATTER}\work\{LAPTOP_THREAD}",
            spellings[0]
        );
        allowed.extend([
            (
                "Bash",
                format!(r#"cat "{}/memo.md""#, notes.replace('\\', "/")),
            ),
            ("PowerShell", format!("Get-ChildItem {notes}")),
            ("PowerShell", format!(r"gc {notes}\memo.md")),
        ]);
        for (tool, command) in &allowed {
            let got = judged(&deny, &allow, tool, command);
            assert_eq!(got, Verdict::Allowed, "{tool}: {command}");
        }
        for (tool, command) in [
            ("Bash", format!(r#"cat "{MATTER}"/*.md"#)),
            ("Bash", format!(r#"ls "{MATTER}"/[a-c]?.docx"#)),
            ("PowerShell", format!(r#"Get-Content "{MATTER}\*.md""#)),
            ("PowerShell", format!(r#"gci "{MATTER}\*""#)),
            ("Bash", format!("find {notes} -name '*.md'")),
            ("Bash", format!(r#"cp {work}\*.docx "{MATTER}""#)),
            ("PowerShell", format!(r"Copy-Item {work}\*.docx '{MATTER}'")),
        ] {
            assert!(!glob.denies(tool, &command), "{tool}: {command}");
            assert!(!glob.denies(tool, &cmdlet(&command)), "{tool}: {command}");
        }
    }
}

/// #1098's pinned first line (open, branch `ps-read-allowlist`,
/// `crates/host/src/pre_write_guard/ps_read/parse.rs` `PIN`), copied as it stands there: it holds
/// `[` (`[IO.Path]`, `[Console]`) before the read's own path, so a deny keyed on no command
/// word would refuse every rewritten read. Keep this copy in step with #1098 until it lands.
const PIN_1098: &str = concat!(
    r"try { ",
    r#"Microsoft.PowerShell.Core\Import-Module -Name "$PSHOME\Modules\Microsoft.PowerShell.Management" "#,
    r"-Force -ErrorAction Stop; ",
    r"if (-not (Microsoft.PowerShell.Core\Get-Module -All -Name 'Microsoft.PowerShell.Utility')) ",
    r"{ Microsoft.PowerShell.Core\Import-Module -Name ",
    r"(Microsoft.PowerShell.Management\Join-Path -Path $PSHOME ",
    r"-ChildPath 'Modules\Microsoft.PowerShell.Utility') -ErrorAction Stop }; ",
    r"$ok = @(foreach ($f in @(",
    r"'Modules\Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psd1', ",
    r"'Modules\Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psm1'",
    r")) { [IO.Path]::GetFullPath((Microsoft.PowerShell.Management\Join-Path -Path $PSHOME ",
    r"-ChildPath $f)) }); ",
    r"foreach ($m in @(Microsoft.PowerShell.Core\Get-Module -All ",
    r"-Name 'Microsoft.PowerShell.Utility')) ",
    r"{ if ($ok -notcontains [IO.Path]::GetFullPath($m.Path)) { throw 'not from PSHOME' } } ",
    r"} catch { [Console]::Error.WriteLine('",
    r"Clauseo stopped this command: PowerShell''s built-in modules could not be loaded ",
    r"from its own folder, so nothing was read.'); exit 1 }; ",
);

/// #1098's rewritten reads of an own folder (its pinned line, then module-qualified cmdlets with
/// `-LiteralPath`) match no glob deny, whole or in any part, as written or with the first word
/// mapped ([`cmdlet`]).
#[test]
fn the_1098_pinned_read_is_not_denied() {
    let (_, glob, _, _) = rules(&[DATA]);
    let notes = format!(r"{DATA}\agent-state\{LAPTOP_MATTER}\notes");
    let work = format!(r"{DATA}\agent-state\{LAPTOP_MATTER}\work\{LAPTOP_THREAD}");
    let m = r"Microsoft.PowerShell.Management";
    for read in [
        format!(r"{m}\Get-Content -LiteralPath '{notes}\memo.md'"),
        format!(r"{m}\Get-Content -LiteralPath '{work}\d.md' -TotalCount 40 -Raw"),
        format!(r"{m}\Get-ChildItem -LiteralPath '{notes}' -Force -Name"),
        format!(
            r"{m}\Get-ChildItem -LiteralPath '{work}' -File | {}",
            r"Microsoft.PowerShell.Utility\Select-Object -First 5"
        ),
    ] {
        let command = format!("{PIN_1098}{read}");
        let own = command.find(&read).unwrap();
        assert!(command.find('[').is_some_and(|at| at < own), "{command}");
        let mut texts = vec![command.clone()];
        texts.extend(command.split([';', '|']).map(|part| part.trim().to_owned()));
        for text in &texts {
            assert!(!glob.denies("PowerShell", text), "{text}");
            assert!(!glob.denies("PowerShell", &cmdlet(text)), "{text}");
        }
    }
}

/// The launch with the glob denies still fits: 60 seeded matters, the default folder and a
/// configured one spelled two ways, Read and Edit with the glob denies, the allow rules and the
/// Windows shell rules with their variable forms, under `SHED_AT`.
#[test]
fn the_launch_with_the_glob_denies_fits_its_budget() {
    use crate::agent::loads::profile::SHED_AT;
    let size = |rules: &[String]| serde_json::to_string(rules).unwrap().len();
    let dir = TempDir::new("own-glob-size");
    for spellings in [vec![DATA], CONFIGURED.to_vec()] {
        let Launch { deny, ids, .. } = launch(&dir, &spellings);
        assert_eq!(ids.len(), 60);
        let glob = own_shell_glob_deny_rules(&deny, true);
        let read_edit = claude_deny_rules_on(&deny, true, false);
        let allow = claude_allow_rules_on(&deny, true);
        let shell = super::super::super::windows_shell_deny_rules(
            &deny,
            &super::super::super::cross_tests::env,
        );
        let total = size(&read_edit) + size(&allow) + size(&shell);
        eprintln!(
            "{spellings:?}: {} glob denies ({} B); deny, allow and shell {total} B of {SHED_AT}",
            glob.len(),
            size(&glob)
        );
        assert!(glob.iter().all(|rule| read_edit.contains(rule)));
        assert!(total < SHED_AT, "{total}");
    }
}

/// The settings' `permissions.deny` a Claude launch of `deny` gets with `sandbox` (Claude's
/// shell sandbox on, as on a Mac or Linux) or none (Windows, or the sandbox `Off`).
fn launch_deny_rules(
    deny: &LaunchDeny,
    sandbox: Option<&crate::agent::loads::sandbox::ClaudeSandbox>,
) -> Vec<String> {
    use crate::agent::loads::browser::Switch;
    use crate::agent::loads::profile::{ClaudeKeys, Folders, Login, claude_options};
    let keys = ClaudeKeys {
        web_search: Switch {
            on: true,
            locked: false,
        },
        login: Login::FirstParty,
        folders: Folders {
            work: "w",
            notes: "n",
            connectors: "",
            browser: "",
        },
        seed_sites: &[],
        forced_ask: false,
        data: deny,
        case_law: false,
        browser: false,
        sandbox,
        matter: None,
        trusted_servers: &[],
    };
    let mut options = serde_json::Map::new();
    claude_options(&mut options, &keys);
    options["settings"]["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|rule| rule.as_str().map(str::to_owned))
        .collect()
}

/// The glob denies go only to a launch with no shell sandbox: Windows, or a Mac or Linux launch
/// whose sandbox is `Off` (`agent::loads::sandbox`, which falls back to `acceptEdits`). A
/// sandboxed launch gets none: Seatbelt checks the real path a glob reaches, and the denies
/// would only refuse an own read piped through a glob there.
#[test]
fn only_a_launch_with_no_shell_sandbox_gets_the_glob_denies() {
    let dir = TempDir::new("own-glob-sandbox");
    let unix = crate::testing::abs("/Users/asha/Library/Application Support/ai.clauseo.desktop");
    let spelling = if cfg!(windows) { DATA } else { unix.as_str() };
    let deny = LaunchDeny {
        spellings: vec![PathBuf::from(spelling)],
        ..launch(&dir, &[]).deny
    };
    let glob = own_shell_glob_deny_rules(&deny, cfg!(windows));
    assert!(glob.len() >= 36, "{}", glob.len());
    // The rule list alone, either way of writing the paths.
    for windows in [true, false] {
        let all = own_shell_glob_deny_rules(&deny, windows);
        let with = claude_deny_rules_on(&deny, windows, false);
        let without = claude_deny_rules_on(&deny, windows, true);
        assert!(all.iter().all(|rule| with.contains(rule)));
        assert!(!all.iter().any(|rule| without.contains(rule)));
        assert_eq!(with.len(), without.len() + all.len());
    }
    // A whole launch's settings: no sandbox (Windows, or `Off`) and a sandbox.
    let none = launch_deny_rules(&deny, None);
    assert!(glob.iter().all(|rule| none.contains(rule)), "{none:?}");
    let sandbox = crate::agent::loads::sandbox::ClaudeSandbox {
        allow_write: Vec::new(),
        deny_write: Vec::new(),
        deny_read: Vec::new(),
        allow_read: Vec::new(),
        quiet: Vec::new(),
    };
    let sandboxed = launch_deny_rules(&deny, Some(&sandbox));
    assert!(
        !glob.iter().any(|rule| sandboxed.contains(rule)),
        "{sandboxed:?}"
    );
    assert!(sandboxed.iter().any(|rule| rule.starts_with("Read(")));
}

/// The cost on Windows, accepted until PR (b) drops the own folders' shell rules there after
/// #1098: Claude Code matches a deny on the whole command too, so an allowed own read followed
/// by a pipe or a test holding a glob character is refused outright, where it went to a
/// classifier before. Claude can search its notes with its Grep tool instead.
#[test]
fn on_windows_a_glob_after_an_own_read_is_refused_too() {
    let (_, _, _, glob) = rules(&[DATA]);
    let notes = format!(r"{DATA}\agent-state\{LAPTOP_MATTER}\notes");
    for (tool, command) in [
        ("Bash", format!(r"cat {notes}\a.md | grep 'a.*b'")),
        (
            "PowerShell",
            format!(r"Get-Content {notes}\a.md | Select-String 'x*'"),
        ),
        (
            "PowerShell",
            format!("Get-ChildItem {notes} | Where-Object Name -like '*.md'"),
        ),
        ("Bash", format!("ls {notes} && [ -f x ]")),
    ] {
        assert!(denied_by(&glob, tool, &command), "{tool}: {command}");
    }
}
