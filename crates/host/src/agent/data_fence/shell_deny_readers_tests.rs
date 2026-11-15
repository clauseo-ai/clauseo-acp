//! The Windows shell deny rules against the commands an agent reads a file with: every common
//! reader (`type`, `cat`, `Get-Content`, `gc`, `Select-String`, `findstr`, `copy`, `robocopy`,
//! `git --git-dir`, `sqlite3`, `python -c`) is denied when it names a protected place in any of
//! `<data>`'s spellings, the 8.3 short name among them; the own folders stay reachable through
//! the short name; and a deny still wins beside the own folders' allow (`own_allow.rs`). The rules
//! stay advisory (a path built at run time passes them), and none of them asks the lawyer.

use std::path::PathBuf;

use super::cross_tests::{Rules, env};
use super::own_shell_allow_rules;
use super::windows_shell_deny_rules;
use crate::agent::data_fence::LaunchDeny;
use crate::agent::data_fence::big_folder::{LAPTOP_MATTER, LAPTOP_RUN, laptop};
use crate::agent::data_fence::launch::alias_of;
use crate::pipeline::tests::support::TempDir;

/// The laptop's default data folder.
const DATA: &str = r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop";

/// The 8.3 alias Windows gives the data folder's name on the laptop.
const SHORT: &str = "AICLAU~1.DES";

/// The laptop's launch (60 seeded matters) with `<data>` spelled [`DATA`], `short` its short
/// names; the seeded matters' ids.
fn launch(dir: &TempDir, short: &[&str]) -> (LaunchDeny, Vec<String>) {
    let (deny, _, ids) = laptop(dir.path(), 1, 60, "clauseo-lawyer");
    let deny = LaunchDeny {
        spellings: vec![PathBuf::from(DATA)],
        short_names: short.iter().map(|name| (*name).to_owned()).collect(),
        ..deny
    };
    (deny, ids)
}

/// The ways a command writes `<data>`, each with the separator it goes on with and the shells
/// that write it (`true`: PowerShell too).
fn leads() -> Vec<(String, &'static str, bool)> {
    vec![
        (DATA.to_owned(), "\\", true),
        (DATA.replace('\\', "/"), "/", true),
        (DATA.replace('\\', r"\\"), r"\\", false),
        (r"%LOCALAPPDATA%\ai.clauseo.desktop".to_owned(), "\\", true),
        (
            r"$env:LOCALAPPDATA\ai.clauseo.desktop".to_owned(),
            "\\",
            true,
        ),
        (
            "$env:APPDATA/../Local/ai.clauseo.desktop".to_owned(),
            "/",
            true,
        ),
        (r"~\AppData\Local\ai.clauseo.desktop".to_owned(), "\\", true),
        ("~/AppData/Local/ai.clauseo.desktop".to_owned(), "/", true),
        (
            format!(r"C:\Users\CLAUSE~1\AppData\Local\{SHORT}"),
            "\\",
            true,
        ),
        (format!(r"%LOCALAPPDATA%\{SHORT}"), "\\", true),
        (
            format!("~/AppData/Local/{}", SHORT.to_lowercase()),
            "/",
            true,
        ),
    ]
}

/// The protected places the readers name, under `<data>`: the search index, a matter's version
/// store, the log, another matter's notes and another session's run folder.
fn protected(ids: &[String]) -> Vec<Vec<String>> {
    let names = |path: &str| path.split('\\').map(str::to_owned).collect::<Vec<_>>();
    vec![
        names("search.sqlite"),
        names(&format!(r"matters\{}\history.git", ids[0])),
        names(r"logs\host.log"),
        names(&format!(r"agent-state\{}\notes\n.md", ids[1])),
        names(r"run\01a1d672-1c0e-7d3b-8e11-5a2b9c0d4e6f-2\work\x.md"),
    ]
}

/// Each reader with the path `p` in it: the ones both shells write, then PowerShell's.
fn readers(p: &str) -> Vec<String> {
    vec![
        format!("type {p}"),
        format!("cat \"{p}\""),
        format!("findstr /s secret {p}"),
        format!("copy {p} out.txt"),
        format!("robocopy {p} C:\\out /E"),
        format!("git --git-dir={p} log -p"),
        format!("git --git-dir \"{p}\" show HEAD:a.docx"),
        format!("sqlite3 {p} \"select * from messages\""),
        format!("python -c \"print(open(r'{p}').read())\""),
        format!("Get-Content {p}"),
        format!("gc -Raw '{p}'"),
        format!("Select-String -Path {p} -Pattern secret"),
        format!("Get-Content -LiteralPath \"{p}\" | Out-String"),
    ]
}

/// `names` under `lead`, joined by `sep`.
fn under(lead: &str, sep: &str, names: &[String]) -> String {
    names
        .iter()
        .fold(lead.to_owned(), |path, name| format!("{path}{sep}{name}"))
}

/// Every reader that names a protected place in any spelling of `<data>`, the short name and
/// its lower case among them, is denied in Bash and, where PowerShell writes it, in PowerShell.
#[test]
fn every_reader_naming_a_protected_place_is_denied() {
    let dir = TempDir::new("readers-deny");
    let (deny, ids) = launch(&dir, &[SHORT]);
    let rules = Rules::new(&windows_shell_deny_rules(&deny, &env));
    let mut checked = 0;
    for (lead, sep, powershell) in leads() {
        for place in protected(&ids) {
            let path = under(&lead, sep, &place);
            for command in readers(&path) {
                let mut tools = vec!["Bash"];
                if powershell {
                    tools.push("PowerShell");
                }
                for tool in tools {
                    checked += 1;
                    assert!(rules.denies(tool, &command), "{tool}: {command}");
                }
            }
        }
    }
    eprintln!("readers: {checked} commands denied");
}

/// The short name is a form of its own only when the launch has it: without it, a command that
/// writes the data folder by its short name is in no rule (the residual the module doc lists).
/// With it, the session's own folders stay reachable through it, and every rule is a deny.
#[test]
fn the_short_name_denies_the_rest_and_keeps_the_own_folders() {
    let dir = TempDir::new("readers-short");
    let (without, _) = launch(&dir, &[]);
    let (with, _) = launch(&dir, &[SHORT]);
    let base = format!(r"C:\Users\CLAUSE~1\AppData\Local\{SHORT}");
    let index = format!(r"type {base}\search.sqlite");
    let before = Rules::new(&windows_shell_deny_rules(&without, &env));
    assert!(!before.denies("PowerShell", &index));
    let rules = windows_shell_deny_rules(&with, &env);
    assert!(rules.len() > windows_shell_deny_rules(&without, &env).len());
    assert!(
        rules
            .iter()
            .all(|rule| rule.starts_with("Bash(") || rule.starts_with("PowerShell("))
    );
    let after = Rules::new(&rules);
    for tool in ["Bash", "PowerShell"] {
        assert!(after.denies(tool, &index), "{tool}");
        assert!(after.denies(tool, &format!("dir {base}")), "{tool}");
        assert!(after.denies(tool, &format!(r"dir {base}\ /s")), "{tool}");
        for own in [
            format!(r"{base}\agent-state\{LAPTOP_MATTER}\notes\Notes.md"),
            format!(r"{base}\run\{LAPTOP_RUN}\work\memo.md"),
        ] {
            assert!(!after.denies(tool, &format!("type {own}")), "{tool}: {own}");
        }
    }
}

/// A deny still wins beside the own folders' allow (`own_allow.rs`, Claude Code's order: deny,
/// then ask, then allow): a command that lists or reads an own folder, which an allow rule
/// matches, and also names a protected place, is denied. Claude Code matches the allow rules'
/// commands by their aliases too (`gc`, `type`); this port does not, so those are checked
/// against the deny alone.
#[test]
fn a_deny_still_wins_beside_the_own_allow() {
    let dir = TempDir::new("readers-allow");
    let (deny, ids) = launch(&dir, &[SHORT]);
    let denies = Rules::new(&windows_shell_deny_rules(&deny, &env));
    let allow = Rules::new(&own_shell_allow_rules(&deny, true));
    let notes = format!(r"{DATA}\agent-state\{LAPTOP_MATTER}\notes\Notes.md");
    let work = format!(r"{DATA}\run\{LAPTOP_RUN}\work");
    let mut contested = 0;
    for (lead, sep, powershell) in leads() {
        for place in protected(&ids) {
            let path = under(&lead, sep, &place);
            let mut commands = vec![
                ("Bash", format!("cat {notes} {path}"), true),
                ("Bash", format!("ls -la {work} {path}"), true),
                ("Bash", format!("head -n 5 {notes} {path}"), true),
            ];
            if powershell {
                commands.extend([
                    ("PowerShell", format!("Get-Content {notes} {path}"), true),
                    ("PowerShell", format!("Get-ChildItem {work} {path}"), true),
                    ("PowerShell", format!("gc {notes} {path}"), false),
                    ("PowerShell", format!("type {notes} {path}"), false),
                ]);
            }
            for (tool, command, allowed) in commands {
                if allowed {
                    assert!(allow.denies(tool, &command), "no allow: {tool}: {command}");
                    contested += 1;
                }
                assert!(denies.denies(tool, &command), "{tool}: {command}");
            }
        }
    }
    assert!(contested > 0);
}

/// An 8.3 alias is the short path's last name when it holds a `~` and is not the long name in
/// another case; separators either way, a trailing one cut.
#[test]
fn an_alias_is_a_short_last_name_with_a_tilde() {
    let alias = |long: &str, short: &str| alias_of(&PathBuf::from(long), &PathBuf::from(short));
    assert_eq!(
        alias(DATA, &format!(r"C:\Users\CLAUSE~1\AppData\Local\{SHORT}")).as_deref(),
        Some(SHORT)
    );
    assert_eq!(
        alias(r"D:\FirmData\Clauseo Data\", r"D:\FIRMDA~1\CLAUSE~1\").as_deref(),
        Some("CLAUSE~1")
    );
    assert_eq!(alias(r"D:\Data\Clauseo", r"D:\Data\CLAUSEO"), None);
    assert_eq!(
        alias(r"C:\Users\CLAUSE~1\Data", r"C:\Users\CLAUSE~1\Data"),
        None
    );
    assert_eq!(alias(r"D:\a~1", r"D:\A~1"), None);
}
