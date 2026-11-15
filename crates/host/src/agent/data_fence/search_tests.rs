//! The code owner's second review: a search or a recursive command started in a folder above
//! the data folder reaches all of it, and a glob pattern can reach it past its literal start
//! (character classes, braces, Grep's `glob`). Each is denied; a search started in the matter
//! folder, in a folder that does not hold the data folder, or in the session's own folders
//! is not. And a third-party tool's path under any key is read.

use std::path::PathBuf;

use serde_json::json;

use super::ShellKind;
use super::command;

/// [`command::recursive_roots`] of a line from a tool whose shell is not known.
fn recursive_roots(line: &str, windows: bool) -> Vec<PathBuf> {
    command::recursive_roots(line, windows, ShellKind::Unknown)
}
use super::search::{literal_root, pattern_roots};

use super::tests::{check, layout, text};
use crate::write;

/// The review's seven requests from the data folder's parent, each denied.
#[test]
fn a_search_from_a_folder_above_the_data_folder_is_denied() {
    let dir = layout("above");
    let root = dir.path();
    let parent = text(root);
    let requests = [
        json!({"pattern": "privileged", "path": parent}),
        json!({"command": format!("grep -r privileged {parent}")}),
        json!({"command": format!("find {parent} -name SKILL.md -exec cat {{}} +")}),
        json!({"path": parent, "pattern": "dat[a]/run/*/**"}),
        json!({"pattern": format!("{parent}/dat[a]/skill-packs/**/SKILL.md")}),
        json!({"pattern": format!("{{{parent}/data,/x}}/skill-packs/**")}),
        json!({"pattern": "privileged", "path": parent, "glob": "data/run/**"}),
    ];
    let shown: Vec<_> = requests
        .iter()
        .filter(|request| !check(root, &[], Some(request)).touches_data())
        .collect();
    assert!(shown.is_empty(), "shown: {shown:#?}");
}

/// A pattern that climbs out of a folder that does not hold the data folder, through `..` in
/// its literal start, a brace or Grep's `glob`, and a recursive command started in the matter
/// folder when that holds the data folder, are denied too.
#[test]
fn a_pattern_or_command_that_climbs_to_the_data_folder_is_denied() {
    let dir = layout("climb");
    let root = dir.path();
    let matter = text(&root.join("matter"));
    let outside = text(&root.join("outside"));
    let requests = [
        json!({"path": matter, "pattern": "../dat[a]/run/**"}),
        json!({"path": outside, "pattern": "{a,../data}/**"}),
        json!({"pattern": "x", "path": matter, "glob": "../data/**", "output_mode": "content"}),
        json!({"pattern": format!("{matter}/../*/skill-packs/**")}),
        json!({"command": "grep -R privileged .."}),
        json!({"command": format!("ls -R {}", text(&root.join("data/run")))}),
        json!({"command": format!("rg secret {outside}/..")}),
        json!({"command": format!("tar czf out.tgz -C {} .", text(root))}),
        json!({"command": format!("du -sh {}", text(root))}),
        json!({"command": format!("sudo rsync -a {}/ /tmp/copy", text(root))}),
        json!({"command": format!("cp -r {} /tmp/copy", text(root))}),
    ];
    let shown: Vec<_> = requests
        .iter()
        .filter(|request| !check(root, &[], Some(request)).touches_data())
        .collect();
    assert!(shown.is_empty(), "shown: {shown:#?}");
}

/// Ordinary searches are not denied: in the matter folder (by path, by default, or by a
/// relative `.`), in a folder beside the data folder, in the session's own run folder and
/// notes, in the built-in skills, a Grep whose regex holds slashes, and a plain `ls` of the
/// data folder's parent, which lists its name and nothing in it.
#[test]
fn a_search_in_the_matter_or_an_unrelated_folder_is_not_denied() {
    let dir = layout("ordinary");
    let root = dir.path();
    let data = root.join("data");
    let skills = data.join("profile/claude-plugin/skills/legal-drafting");
    write::create_dir_all(&skills).unwrap();
    let matter = text(&root.join("matter"));
    let outside = text(&root.join("outside"));
    let run = text(&data.join("run/s1"));
    let notes = text(&data.join("agent-state/m1/notes"));
    let skills = text(&data.join("profile/claude-plugin/skills"));
    let requests = [
        json!({"pattern": "privileged", "path": matter}),
        json!({"pattern": "privileged", "path": matter, "glob": "*.{md,txt}"}),
        json!({"pattern": "**/*.md"}),
        json!({"path": matter, "pattern": "sub/**/*.txt"}),
        json!({"pattern": format!("{matter}/**/*.{{md,docx}}")}),
        json!({"pattern": format!("{outside}/[a-z]*.md")}),
        json!({"pattern": "privileged", "path": run}),
        json!({"path": notes, "pattern": "**"}),
        json!({"path": skills, "pattern": "**/*.md"}),
        json!({"pattern": "https://example.com/a/.*", "path": matter, "output_mode": "content"}),
        json!({"command": format!("grep -rn privileged {matter}")}),
        json!({"command": "grep -rn privileged ."}),
        json!({"command": "rg -l secret sub"}),
        json!({"command": format!("find {outside} -name '*.docx'")}),
        json!({"command": format!("ls -R {outside} && du -sh {matter}")}),
        json!({"command": format!("grep -r x {run}/work")}),
        json!({"command": format!("tree {notes}")}),
        json!({"command": format!("ls {}", text(root))}),
        json!({"command": format!("grep -c x {}/sub/a.txt", matter)}),
    ];
    for request in requests {
        assert!(
            !check(root, &[], Some(&request)).touches_data(),
            "{request}"
        );
    }
}

/// A third-party (MCP) tool's input is read under every key, nested ones too: a `filename`, a
/// `file://` `uri`, a path in an object in an array. A path in the matter folder, free text
/// and the text Claude's own tools write (`content`, `new_string`) are not denied.
#[test]
fn a_third_party_tools_path_under_any_key_is_read() {
    let dir = layout("mcp");
    let root = dir.path();
    let settings = text(&root.join("data/settings.json"));
    let skill = text(&root.join("data/skill-packs/x/skills/brief/SKILL.md"));
    let requests = [
        json!({"filename": settings}),
        json!({"uri": format!("file://{skill}")}),
        json!({"files": [{"name": "a"}, {"source": skill}]}),
        json!({"options": {"target": "~/data/settings.json"}}),
    ];
    let shown: Vec<_> = requests
        .iter()
        .filter(|request| !check(root, &[], Some(request)).touches_data())
        .collect();
    assert!(shown.is_empty(), "shown: {shown:#?}");
    let matter = text(&root.join("matter/sub/a.txt"));
    let fine = [
        json!({"filename": matter}),
        json!({"query": "and/or the /data/ question", "limit": 3}),
        json!({"file_path": matter, "content": format!("as {settings} says")}),
        json!({"file_path": matter, "old_string": "a", "new_string": settings}),
    ];
    for request in fine {
        assert!(
            !check(root, &[], Some(&request)).touches_data(),
            "{request}"
        );
    }
}

/// A pattern's literal root is cut at the first part with a glob character, not inside it, and
/// climbs once for each level a later `..` may go above it.
#[test]
fn a_patterns_root_is_its_literal_parts_climbed_by_later_dotdots() {
    for (pattern, root) in [
        ("/p/dat[a]/run/**", "/p"),
        ("/p/data/run/*.md", "/p/data/run"),
        ("/*", "/"),
        ("*.md", ""),
        ("**/x", ""),
        ("a/*/../../b/*", "a/.."),
        ("a/**/../x", "a/.."),
        ("~/x/{a,b}", "~/x"),
        ("/p/data/settings.json", "/p/data/settings.json"),
    ] {
        assert_eq!(literal_root(pattern, false), root, "{pattern}");
    }
    assert_eq!(literal_root(r"C:\p\dat?\x", true), r"C:\p");
    assert_eq!(literal_root(r"C:\*", true), r"C:\");
}

/// Braces are expanded, nested ones too, each choice rooted on its own; a relative one is taken
/// from the search's folder. Past 64 expansions the pattern is cut at its first brace.
#[test]
fn a_patterns_braces_are_expanded_and_each_choice_rooted() {
    let roots = pattern_roots("{/p/data,/x/{a,b}}/skill-packs/**", "/base", false);
    let want: Vec<PathBuf> = [
        "/p/data/skill-packs",
        "/x/a/skill-packs",
        "/x/b/skill-packs",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    assert_eq!(roots, want);
    let relative = pattern_roots("{a,../data}/**", "/base", false);
    assert_eq!(
        relative,
        [PathBuf::from("/base/a"), PathBuf::from("/base/../data")]
    );
    let many = "/p/{a,b,c,d,e}{a,b,c,d,e}{a,b,c,d,e}/x";
    assert_eq!(pattern_roots(many, "/base", false), [PathBuf::from("/p")]);
}

/// The folders a recursive command walks: its operands and the working folder, past wrappers
/// and assignments; a command that does not walk gives none.
#[test]
fn a_recursive_commands_operands_and_the_working_folder_are_its_roots() {
    let roots = |line: &str| recursive_roots(line, false);
    let paths = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();
    assert_eq!(roots("grep -rn x /p"), paths(&[".", "x", "/p"]));
    assert_eq!(
        roots("LC_ALL=C sudo -n timeout 5 find $HOME/p -name y"),
        paths(&[".", "~/p", "y"])
    );
    assert_eq!(roots("ls -lR /p; ls -lr /q"), paths(&[".", "/p"]));
    assert_eq!(roots("/usr/bin/rg x"), paths(&[".", "x"]));
    assert_eq!(roots("cp -a /p /q"), paths(&[".", "/p", "/q"]));
    for line in [
        "grep x /p",
        "cat /p",
        "ls /p",
        "grep --include=*.rs x /p",
        "cp /p /q",
    ] {
        assert!(roots(line).is_empty(), "{line}");
    }
    assert_eq!(
        recursive_roots(r"Get-ChildItem -Recurse C:\p", true),
        paths(&[".", r"C:\p"])
    );
}

/// Every spelling PowerShell takes for `-Recurse` makes its walkers walk: any prefix of the
/// name, its alias `-s`, `-Depth` from `-de`, with an attached value, after an en dash, em dash
/// or horizontal bar, in any case; `-d` alone (`-Directory`) does not.
#[test]
fn every_powershell_spelling_of_recurse_walks() {
    let paths = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();
    let mut spellings: Vec<String> = (1..="recurse".len())
        .map(|end| format!("-{}", "recurse".get(..end).unwrap_or_default()))
        .collect();
    spellings.extend(
        [
            "-RECURSE",
            "-Rec",
            "-s",
            "-S",
            "-de",
            "-dep",
            "-dept",
            "-depth",
            "-Depth",
            "-DE",
            "-Recurse:$true",
            "-r:$false",
            "-Depth:3",
            "-s:$true",
        ]
        .map(str::to_owned),
    );
    spellings.extend(
        ["\u{2013}", "\u{2014}", "\u{2015}"]
            .iter()
            .flat_map(|dash| {
                ["r", "Recurse", "Depth", "s:$true"].map(|name| format!("{dash}{name}"))
            }),
    );
    for spelling in &spellings {
        for command in ["gci", "Get-ChildItem", "GCI", "dir", "Remove-Item", "cpi"] {
            for windows in [true, false] {
                let line = format!("{command} {spelling} /p");
                assert_eq!(
                    recursive_roots(&line, windows),
                    paths(&[".", "/p"]),
                    "{line}"
                );
            }
        }
        let line = format!(r"Get-ChildItem {spelling} C:\p");
        assert_eq!(
            recursive_roots(&line, true),
            paths(&[".", r"C:\p"]),
            "{line}"
        );
    }
    assert_eq!(
        recursive_roots("Get-ChildItem -Depth 2 -Path:/p", false),
        paths(&[".", "2", "/p"])
    );
    for line in [
        "gci -d /p",
        "Get-ChildItem -Directory /p",
        "gci /p",
        "gci -Force /p",
    ] {
        assert!(recursive_roots(line, false).is_empty(), "{line}");
    }
}

/// `ls`, `cp` and `rm` take PowerShell's short spellings only in a line known to be
/// PowerShell (a `pwsh -Command` script, or a line running a cmdlet); in Bash `sort -r`, `ls -r`
/// and `tail -s` do not walk. `-Recurse` in full makes any command walk.
#[test]
fn powershell_short_spellings_apply_only_to_powershell() {
    let paths = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();
    for line in [
        "sort -r memo.txt",
        "ls -r /p",
        "ls -s /p",
        "tail -s 5 x.log",
        "rm -s /p",
        "cp -s /p /q",
        "bash -c 'ls -r /p'",
        "ls -dep /p",
    ] {
        for windows in [true, false] {
            assert!(recursive_roots(line, windows).is_empty(), "{line}");
        }
    }
    for line in [
        r#"pwsh -NoProfile -Command "ls -r /p""#,
        "powershell -Command \"rm \u{2013}Recurse:$true /p\"",
        "Set-Location /q; ls -r /p",
        "Write-Output x; rm -Depth 3 /p",
    ] {
        let roots = recursive_roots(line, false);
        assert!(roots.contains(&PathBuf::from("/p")), "{line}: {roots:?}");
    }
    for line in [
        "anything -recurse /p",
        "Get-Item -Recurse:$false /p",
        "x \u{2014}RECURSE /p",
    ] {
        assert_eq!(recursive_roots(line, false), paths(&[".", "/p"]), "{line}");
    }
}

/// Told the tool, the fence reads its line as that shell does: from PowerShell's tool `ls`,
/// `cp`, `rm`, `rmdir` and `dir` take PowerShell's spellings with no cmdlet beside them; from
/// Bash's, `ls -r` and GNU's `dir -r` sort in reverse and `dir -R` walks, while a script
/// given to `pwsh -Command` is still PowerShell's, and from PowerShell's a `bash -c` script
/// is Bash's.
#[test]
fn the_tools_shell_decides_how_its_line_is_read() {
    let roots = |line: &str, shell| command::recursive_roots(line, false, shell);
    let walks = |line: &str, shell| roots(line, shell).contains(&PathBuf::from("/p"));
    for line in [
        "ls -r /p",
        "dir -r /p",
        "cp -Recurse:$true /p /q",
        "rm -s /p",
        "rmdir \u{2013}r /p",
        "gci -de 2 /p",
        "ls -r /q; pwsh -c 'x'; ls -r /p",
        "bash -c 'ls -R /p'",
    ] {
        assert!(walks(line, ShellKind::PowerShell), "PowerShell: {line}");
    }
    for line in [
        "ls -r /p",
        "dir -r /p",
        "gci -r /p",
        "rm -s /p",
        "bash -c 'ls -r /p'",
    ] {
        assert!(!walks(line, ShellKind::Posix), "Bash: {line}");
    }
    for line in [
        "dir -R /p",
        "ls -lR /p",
        r#"pwsh -NoProfile -Command "ls -r /p""#,
        "find /p",
        "x -Recurse /p",
    ] {
        assert!(walks(line, ShellKind::Posix), "Bash: {line}");
    }
    assert!(!walks(
        "pwsh -c 'Write-Output x'; bash -c 'ls -r /p'",
        ShellKind::PowerShell
    ));
    assert!(walks("dir -r /p", ShellKind::Unknown));
    assert!(!walks("ls -r /p", ShellKind::Unknown));
    assert_eq!(
        ShellKind::of_tool(Some("PowerShell")),
        ShellKind::PowerShell
    );
    assert_eq!(
        ShellKind::of_tool(Some("powershell")),
        ShellKind::PowerShell
    );
    assert_eq!(ShellKind::of_tool(Some("Bash")), ShellKind::Posix);
    assert_eq!(ShellKind::of_tool(Some("exec_command")), ShellKind::Unknown);
    assert_eq!(ShellKind::of_tool(None), ShellKind::Unknown);
}
