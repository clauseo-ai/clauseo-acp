//! The Windows shell deny rules for a real launch's targets (`launch.rs`): the commands Claude
//! Code denied on the Windows VM (main b386fbd50) now pass, and what is not the session's own
//! stays denied in every spelling, in a data folder with 60 matters; and the rules' size.

use std::path::{Path, PathBuf};

use super::tests::denied_by;
use super::windows_shell_deny_rules;
use crate::agent::data_fence::big_folder::{self, OWN_RUN, PREFIX_RUN, matter_id};
use crate::agent::data_fence::{LaunchDeny, claude_deny_rules_on, claude_projects, launch_deny};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// The VM's data folder and matter.
const VM_DATA: &str = r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop";
const VM_MATTER: &str = "01a1d5b4-a291-7649-9fa4-5e91c306e4a0";
const VM_FOLDER: &str = r"C:\Users\clauseo-lawyer\CloudProxy\Undo Matter";
/// The run folders of the VM's sessions: commands 1 to 6, then 7.
const VM_RUN_A: &str = "01a1d5be-5efa-73f6-a24b-dc90c01883d9-1";
const VM_RUN_B: &str = "01a1d5c4-90d5-7548-96d0-4a1610744921-3";

/// `deny` with `<data>` spelled as on Windows: the targets and kept places are relative.
fn on_windows(deny: LaunchDeny, data: &str) -> LaunchDeny {
    LaunchDeny {
        spellings: vec![PathBuf::from(data)],
        ..deny
    }
}

/// The variables of the VM's lawyer.
fn vm_env(name: &str) -> Option<std::ffi::OsString> {
    match name {
        "LOCALAPPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Local".into()),
        "APPDATA" => Some(r"C:\Users\clauseo-lawyer\AppData\Roaming".into()),
        _ => None,
    }
}

/// The VM's data folder, made under `root`, as the launch of the session with run folder `own`
/// enumerates it: its matter and three others, its two runs, `…-10` beside `…-1` (and `…-1`
/// beside `…-10`), Claude's home with a project per matter folder, the logs and stores.
fn vm_launch(root: &Path, own: &str) -> LaunchDeny {
    let data = root.join("data");
    let mut files = vec![
        "settings.json".to_owned(),
        "state.sqlite".to_owned(),
        "logs/host.log".to_owned(),
        "agents/claude-acp/home/.credentials.json".to_owned(),
        "agents/claude-acp/home/plans/p.md".to_owned(),
        "agents/codex-acp/home/auth.json".to_owned(),
        "tools/agent-browser/agent-browser.exe".to_owned(),
    ];
    for run in [
        VM_RUN_A,
        VM_RUN_B,
        format!("{VM_RUN_A}0").as_str(),
        "01a1d5be-5efa-73f6-a24b-dc90c01883d9-11",
    ] {
        files.push(format!("run/{run}/work/x.md"));
        files.push(format!("run/{run}/relay-token"));
    }
    let home = data.join("agents/claude-acp/home");
    let mut folders = vec![VM_FOLDER.to_owned()];
    for n in 1..4 {
        files.push(format!("agent-state/{}/notes/n.md", matter_id(n)));
        folders.push(format!("{VM_FOLDER} {n}"));
    }
    files.push(format!("agent-state/{VM_MATTER}/notes/Notes.md"));
    for folder in &folders {
        let key = super::super::claude_project_key(folder);
        files.push(format!("agents/claude-acp/home/projects/{key}/s.jsonl"));
    }
    for file in &files {
        let path = data.join(file);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, b"x").unwrap();
    }
    let own_folders = [
        data.join("run").join(own),
        data.join("agent-state").join(VM_MATTER).join("notes"),
        home.join("plans"),
    ];
    let projects = claude_projects(&home, &[Path::new(VM_FOLDER)]);
    on_windows(
        launch_deny(vec![data.clone()], &data, &own_folders, &projects),
        VM_DATA,
    )
}

/// The VM's 7 denied commands (`scratch-S18/win-deny-commands.txt`), each with its tool and its
/// session's run folder.
fn vm_commands() -> Vec<(&'static str, &'static str, String)> {
    let notes_fwd = format!(
        "C:/Users/clauseo-lawyer/AppData/Local/ai.clauseo.desktop/agent-state/{VM_MATTER}/notes"
    );
    let notes_back = notes_fwd.replace('/', "\\");
    let work = |run: &str| {
        format!("C:/Users/clauseo-lawyer/AppData/Local/ai.clauseo.desktop/run/{run}/work")
    };
    let work_back = work(VM_RUN_A).replace('/', "\\");
    vec![
        (
            "Bash",
            VM_RUN_A,
            format!(r#"ls "{notes_fwd}"; ls; pandoc "Notes.docx" -t plain"#),
        ),
        (
            "PowerShell",
            VM_RUN_A,
            format!(r#"Get-ChildItem "{notes_back}"; Get-ChildItem; stetdoc skills get core"#),
        ),
        (
            "Bash",
            VM_RUN_A,
            format!(r#"ls "{notes_fwd}"; ls; stetdoc skills get core"#),
        ),
        (
            "Bash",
            VM_RUN_A,
            format!(
                r#"ls -la "{notes_fwd}"; echo ---; ls -laR "C:/Users/clauseo-lawyer/CloudProxy/Undo Matter"; echo ---; ls -la "{}""#,
                work(VM_RUN_A)
            ),
        ),
        (
            "PowerShell",
            VM_RUN_A,
            format!(
                r#"pandoc "{work_back}/reply.md" -o "Reply to legal notice.docx"; if ($?) {{ Remove-Item "{work_back}\reply.md" }}"#
            ),
        ),
        (
            "PowerShell",
            VM_RUN_A,
            format!(r#"pandoc "{work_back}/reply.md" -o "Reply to legal notice.docx""#),
        ),
        (
            "Bash",
            VM_RUN_B,
            format!(
                r#"head -100; echo ---; ls -la "{notes_fwd}"; echo ---; ls -la "{}""#,
                work(VM_RUN_B)
            ),
        ),
    ]
}

/// Every one of the VM's commands passes every rule of its session's launch.
#[test]
fn the_vms_denied_commands_now_pass() {
    let dir = TempDir::new("shell-deny-vm");
    for (tool, own, command) in vm_commands() {
        let root = dir.path().join(own);
        let rules = windows_shell_deny_rules(&vm_launch(&root, own), &vm_env);
        let hit: Vec<&String> = rules
            .iter()
            .filter(|rule| rule.starts_with(&format!("{tool}(")))
            .filter(|rule| super::tests::matches(rule, &command))
            .collect();
        assert!(hit.is_empty(), "{tool}: {command}\n{hit:#?}");
    }
}

/// The spellings of `<data>` a command may use, each with its separator: the VM's.
fn spellings() -> Vec<(String, &'static str)> {
    let back = VM_DATA.to_owned();
    let fwd = back.replace('\\', "/");
    vec![
        (back.clone(), "\\"),
        (fwd.clone(), "/"),
        (back.replace('\\', "\\\\"), "\\\\"),
        (fwd.replacen("C:", "/c", 1), "/"),
        (r"%LOCALAPPDATA%\ai.clauseo.desktop".to_owned(), "\\"),
        ("%LOCALAPPDATA%/ai.clauseo.desktop".to_owned(), "/"),
        (r"$env:LOCALAPPDATA\ai.clauseo.desktop".to_owned(), "\\"),
        ("$env:LOCALAPPDATA/ai.clauseo.desktop".to_owned(), "/"),
        ("$LOCALAPPDATA/ai.clauseo.desktop".to_owned(), "/"),
        ("${LOCALAPPDATA}/ai.clauseo.desktop".to_owned(), "/"),
        ("~/AppData/Local/ai.clauseo.desktop".to_owned(), "/"),
    ]
}

/// What a command reads, each spelling, a 60-matter folder: what is not the session's own is
/// denied in both shells, quoted, bare or at the end, with the spelling's separator or with
/// `\` and `/` mixed (stage C's H1), what every session shares too (H2); the own folders pass,
/// mixed or not.
#[test]
fn in_a_sixty_matter_folder_only_the_own_folders_pass() {
    let dir = TempDir::new("shell-deny-sixty");
    let big = big_folder::big(dir.path());
    let deny = on_windows(big.deny, VM_DATA);
    let rules = windows_shell_deny_rules(&deny, &vm_env);
    let other_matter = matter_id(7);
    let key7 = super::super::claude_project_key(&big_folder::matter_folder(7));
    let denied_places = [
        clauseo_settings::SETTINGS_FILE.to_owned(),
        "logs/host.log".to_owned(),
        "logs".to_owned(),
        "state.sqlite".to_owned(),
        "state.sqlite-wal".to_owned(),
        "agents/claude-acp/home/.credentials.json".to_owned(),
        "agents/codex-acp/home/auth.json".to_owned(),
        format!("agent-state/{other_matter}/notes/notes.md"),
        format!("agent-state/{other_matter}"),
        format!("run/{PREFIX_RUN}/work/draft.md"),
        format!("run/{PREFIX_RUN}"),
        format!("run/{OWN_RUN}/relay-token"),
        "launch-settings/other-run.json".to_owned(),
        "launch-settings".to_owned(),
        "run".to_owned(),
        "agent-state".to_owned(),
        format!("agent-state/{}", matter_id(0)),
        format!("agents/claude-acp/home/projects/{key7}/0193-session.jsonl"),
        format!("agents/claude-acp/home/projects/{key7}"),
        "tools/agent-browser/agent-browser".to_owned(),
        "tools".to_owned(),
        "agents/runtime/clauseo-python-3.13.18-c2-windows-x64/python.exe".to_owned(),
        "profile/claude-plugin/skills/legal-drafting/SKILL.md".to_owned(),
        "profile".to_owned(),
        String::new(),
    ];
    let own_places = [
        format!("run/{OWN_RUN}/work/reply.md"),
        format!("run/{OWN_RUN}/work"),
        format!("agent-state/{}/notes", matter_id(0)),
        format!("agent-state/{}/notes/notes.md", matter_id(0)),
        "agents/claude-acp/home/plans/plan.md".to_owned(),
    ];
    let key = super::super::claude_project_key(&big_folder::matter_folder(0));
    let tool_results =
        format!("agents/claude-acp/home/projects/{key}/0193-session/tool-results/r.txt");
    for (data, sep) in spellings() {
        let other = if sep == "/" { "\\" } else { "/" };
        // The place with the spelling's separator, or with it and `other` in turn.
        let path = |place: &str, mixed: bool| {
            let mut at = data.clone();
            for (n, name) in place.split('/').filter(|name| !name.is_empty()).enumerate() {
                at.push_str(if mixed && n % 2 == 1 { other } else { sep });
                at.push_str(name);
            }
            at
        };
        let both = |place: &str| [path(place, false), path(place, true)];
        for at in denied_places.iter().flat_map(|place| both(place)) {
            for command in [
                format!(r#"type "{at}""#),
                format!("cat {at}"),
                format!("cat {at} | more"),
                format!("ls '{at}'"),
            ] {
                for tool in ["Bash", "PowerShell"] {
                    assert!(denied_by(&rules, tool, &command), "{tool}: {command}");
                }
            }
        }
        for at in own_places
            .iter()
            .chain([&tool_results])
            .flat_map(|place| both(place))
        {
            for command in [
                format!(r#"type "{at}""#),
                format!("cat {at}"),
                format!("cat {at} | more"),
            ] {
                for tool in ["Bash", "PowerShell"] {
                    assert!(!denied_by(&rules, tool, &command), "{tool}: {command}");
                }
            }
        }
    }
}

/// The settings file every session's Claude reads is in Claude's Read and Edit rules, in its
/// rule form, and in the shell rules, so no tool reads it.
#[test]
fn the_settings_files_are_denied_to_every_tool() {
    let deny = on_windows(LaunchDeny::fixed(vec![]), VM_DATA);
    let read_edit = claude_deny_rules_on(&deny, true);
    let base = "//c/Users/clauseo-lawyer/AppData/Local/ai.clauseo.desktop";
    for tool in ["Read", "Edit"] {
        let rule = format!("{tool}({base}/launch-settings/**)");
        assert!(read_edit.contains(&rule), "{rule}");
    }
    let rules = windows_shell_deny_rules(&deny, &vm_env);
    for (data, sep) in spellings() {
        let command = format!("cat {data}{sep}launch-settings{sep}{OWN_RUN}.json");
        assert!(denied_by(&rules, "Bash", &command), "{command}");
        assert!(denied_by(&rules, "PowerShell", &command), "{command}");
    }
}

/// The rules' size (Read and Edit with the shell rules, as JSON) for 60 and 1,000 matters, each
/// with notes and a Claude project, with the default folder and with a configured one spelled
/// two ways (no folder name to share): the siblings go in groups (`launch.rs`), so 1,000 matters
/// stay under half of Claude Code's 2 MiB settings file.
#[test]
fn a_thousand_matter_launch_stays_under_half_the_settings_file_cap() {
    let configured = [
        PathBuf::from(r"D:\Firm Data\Clauseo Store"),
        PathBuf::from(r"C:\Users\clauseo-lawyer\AppData\Roaming\Clauseo Store"),
    ];
    let size = |rules: &[String]| serde_json::to_string(rules).unwrap().len();
    let mut sizes = Vec::new();
    for matters in [60, 1000] {
        let dir = TempDir::new(&format!("shell-deny-size-{matters}"));
        let big = big_folder::big_with(dir.path(), matters, matters);
        for spellings in [vec![PathBuf::from(VM_DATA)], configured.to_vec()] {
            let deny = LaunchDeny {
                spellings,
                ..big.deny.clone()
            };
            let read_edit = claude_deny_rules_on(&deny, true);
            let shell = windows_shell_deny_rules(&deny, &vm_env);
            eprintln!(
                "{matters} matters, {} spelling(s): {} targets; Read/Edit {} rules {} bytes; \
                 shell {} rules {} bytes",
                deny.spellings.len(),
                deny.targets.len(),
                read_edit.len(),
                size(&read_edit),
                shell.len(),
                size(&shell),
            );
            sizes.push(size(&read_edit) + size(&shell));
        }
    }
    let [sixty_one, sixty_two, thousand_one, thousand_two] = sizes.as_slice() else {
        panic!("{sizes:?}");
    };
    assert!(*thousand_two < 1024 * 1024, "{sizes:?}");
    // At most a few dozen bytes a matter: the groups, not the matters.
    assert!(thousand_one - sixty_one < 100 * 940, "{sizes:?}");
    assert!(thousand_two - sixty_two < 100 * 940, "{sizes:?}");
}

/// A `..` after the data folder's name, with any separator, in every spelling and both shells,
/// is denied: it could climb out of an own folder into a sibling. The own folders' commands
/// without one still pass.
#[test]
fn a_dot_dot_after_the_data_folder_is_denied() {
    let dir = TempDir::new("shell-deny-dots");
    let big = big_folder::big(dir.path());
    let rules = windows_shell_deny_rules(&on_windows(big.deny, VM_DATA), &vm_env);
    let own = matter_id(0);
    for (data, sep) in spellings() {
        let at = |place: &str| format!("{data}{sep}{}", place.replace('/', sep));
        for command in [
            format!(
                r#"type "{}""#,
                at(&format!("run/{OWN_RUN}/../{PREFIX_RUN}/relay-token"))
            ),
            format!(
                "cat {}",
                at(&format!("agent-state/{own}/notes/../../x/notes/n.md"))
            ),
            format!(
                "cat {}/../../settings.json",
                at(&format!("run/{OWN_RUN}/work"))
            ),
            format!(
                r"cat {}\..\..\settings.json",
                at(&format!("run/{OWN_RUN}/work"))
            ),
            format!(
                r"cat {}\\..\\..\\settings.json",
                at(&format!("run/{OWN_RUN}/work"))
            ),
            format!("ls {}/..", at(&format!("run/{OWN_RUN}"))),
            // Mixed: `/..` after a path written with `\`, and the other way round.
            format!(
                "cat {}/../../{}/notes/n.md",
                at(&format!("agent-state/{own}/notes")),
                matter_id(7)
            ),
            format!(
                r"cat {}\..\{PREFIX_RUN}\relay-token",
                at(&format!("run/{OWN_RUN}"))
            ),
        ] {
            for tool in ["Bash", "PowerShell"] {
                assert!(denied_by(&rules, tool, &command), "{tool}: {command}");
            }
        }
        let work = at(&format!("run/{OWN_RUN}/work"));
        let matter = r"C:\Users\clauseo-lawyer\CloudProxy\Matter 00";
        for command in [
            format!(r#"pandoc "{work}{sep}reply.md" -o "{matter}\x.docx""#),
            format!(r#"pandoc "{work}/reply.md" -o "{work}{sep}x.docx""#),
        ] {
            for tool in ["Bash", "PowerShell"] {
                assert!(!denied_by(&rules, tool, &command), "{tool}: {command}");
            }
        }
    }
}

/// The run `…-1` beside the own `…-10`: followed by any boundary character it is denied, in
/// every spelling and both shells; the own run followed by the same stays allowed.
#[test]
fn a_prefix_sibling_is_denied_before_every_boundary_and_the_own_name_is_not() {
    let dir = TempDir::new("shell-deny-bounds");
    let big = big_folder::big(dir.path());
    let rules = windows_shell_deny_rules(&on_windows(big.deny, VM_DATA), &vm_env);
    for (data, sep) in spellings() {
        let marks = [
            ";", "|", "&", ">", "<", "(", ")", ",", "\t", "\n", "*", "?", "\"", "'", " ", "/",
        ];
        for after in marks.into_iter().chain([sep]) {
            let sibling = format!("ls {data}{sep}run{sep}{PREFIX_RUN}{after}x");
            let own = format!("ls {data}{sep}run{sep}{OWN_RUN}{after}x");
            for tool in ["Bash", "PowerShell"] {
                assert!(denied_by(&rules, tool, &sibling), "{tool}: {sibling:?}");
                assert!(!denied_by(&rules, tool, &own), "{tool}: {own:?}");
            }
        }
        let at_end = format!("ls {data}{sep}run{sep}{PREFIX_RUN}");
        assert!(denied_by(&rules, "Bash", &at_end), "{at_end}");
    }
}

/// What every session shares (the built-in plugin, the agents' commands, the Python runtime) is
/// denied to a command that names it, in every spelling, mixed separators too (stage C's H2): a
/// write there would reach every matter. Claude still reads the built-in skills with its Read
/// tool (the allow rules are unchanged) and runs the commands by name on its `PATH`.
#[test]
fn the_shared_places_are_denied_to_the_shell_and_still_readable() {
    let dir = TempDir::new("shell-deny-shared");
    let big = big_folder::big(dir.path());
    let deny = on_windows(big.deny, VM_DATA);
    let rules = windows_shell_deny_rules(&deny, &vm_env);
    let python = r"agents\runtime\clauseo-python-3.13.18-c2-windows-x64\python.exe";
    for (data, sep) in spellings() {
        let at = |place: &str| format!("{data}{sep}{}", place.replace('\\', sep));
        let mixed = |place: &str| {
            let place = place.replacen('\\', "/", 1);
            format!("{data}{sep}{}", place.replace('\\', sep))
        };
        for command in [
            format!(
                "echo x > {}",
                at(r"profile\claude-plugin\skills\legal-drafting\SKILL.md")
            ),
            format!("copy y {}", at(r"tools\agent-browser\agent-browser.exe")),
            format!(r#"{} -c "print(1)""#, at(python)),
            format!(r#"{} -c "print(1)""#, mixed(python)),
            format!("echo x > {}", mixed(r"profile\claude-plugin\skills\x.md")),
            format!("copy y {}", mixed(r"tools\agent-browser\x")),
        ] {
            for tool in ["Bash", "PowerShell"] {
                assert!(denied_by(&rules, tool, &command), "{tool}: {command}");
            }
        }
    }
    // The commands run by name stay allowed.
    for command in [
        r#"pandoc "reply.md" -o "x.docx""#,
        "python -c \"print(1)\"",
        "agent-browser open https://example.com",
        "stetdoc skills get core",
    ] {
        for tool in ["Bash", "PowerShell"] {
            assert!(!denied_by(&rules, tool, command), "{tool}: {command}");
        }
    }
    // The Read allow rules for the built-in skills and the shared commands are as they were.
    let base = super::super::rule_path(VM_DATA, cfg!(windows));
    let allow = super::super::claude_allow_rules(&deny);
    for place in [
        "profile/claude-plugin/skills",
        "tools",
        "agents/runtime/clauseo-python-*",
    ] {
        let rule = format!("Read({base}/{place}/**)");
        assert!(allow.contains(&rule), "{rule}: {allow:?}");
    }
}
