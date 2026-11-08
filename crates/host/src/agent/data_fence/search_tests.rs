//! The code owner's second review: a search or a recursive command started in a folder above
//! the data folder reaches all of it, and a glob pattern can reach it past its literal start
//! (character classes, braces, Grep's `glob`). Each is denied; a search started in the matter
//! folder, in a folder that does not hold the data folder, or in the session's own folders
//! is not. And a third-party tool's path under any key is read.

use std::path::PathBuf;

use serde_json::json;

use super::command::recursive_roots;
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
