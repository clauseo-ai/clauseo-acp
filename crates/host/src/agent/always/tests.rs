//! The rule a Claude request keeps (`always.rs`): one exact rule per tool, none for anything
//! that would not stay exact; the file's read and write; the rules in `_meta`.

use serde_json::{Value, json};

use crate::pipeline::tests::support::TempDir;

use super::{
    FILE, MAX_RULES, allow_in_meta, file_beside, matter_roots, read, rule, store_now, well_formed,
    within,
};

fn of(tool: &str, input: &Value) -> Option<String> {
    rule(tool, Some(input), false)
}

#[test]
fn bash_keeps_the_exact_command() {
    let cases = [
        ("npm test", Some("Bash(npm test)")),
        ("git status --short", Some("Bash(git status --short)")),
        ("cat a.txt | wc -l", Some("Bash(cat a.txt | wc -l)")),
        ("echo $(date)", None),
        ("ls *.docx", None),
        ("npm run:*", None),
        ("printf 'a\\n'", None),
        ("echo a\nrm -rf x", None),
        (" npm test", None),
        ("", None),
    ];
    for (command, want) in cases {
        let got = of("Bash", &json!({"command": command, "description": "d"}));
        assert_eq!(got.as_deref(), want, "{command:?}");
        if let Some(rule) = &got {
            assert!(well_formed(rule), "{rule}");
        }
    }
    assert_eq!(of("Bash", &json!({"cmd": "npm test"})), None);
    assert_eq!(of("Bash", &json!({"command": 1})), None);
    assert_eq!(rule("Bash", None, false), None);
    let long = "a".repeat(3_000);
    assert_eq!(of("Bash", &json!({ "command": long })), None);
}

#[test]
fn web_fetch_keeps_the_host() {
    let cases = [
        (
            "https://sci.gov.in/judgments/a",
            Some("WebFetch(domain:sci.gov.in)"),
        ),
        (
            "http://Example.COM:8080/x?y=1",
            Some("WebFetch(domain:example.com)"),
        ),
        ("https://10.0.0.1/", Some("WebFetch(domain:10.0.0.1)")),
        ("https://[::1]/", None),
        ("file:///etc/passwd", None),
        ("ftp://example.com/", None),
        ("not a url", None),
    ];
    for (url, want) in cases {
        let got = of("WebFetch", &json!({"url": url, "prompt": "p"}));
        assert_eq!(got.as_deref(), want, "{url}");
        if let Some(rule) = &got {
            assert!(well_formed(rule), "{rule}");
        }
    }
}

#[test]
fn an_mcp_tool_keeps_its_exact_name() {
    let input = json!({"query": "cheque bounce"});
    assert_eq!(
        of("mcp__case-law__search", &input).as_deref(),
        Some("mcp__case-law__search")
    );
    for name in [
        "mcp__x",
        "mcp____t",
        "mcp__s__*",
        "mcp__s__t(x)",
        "mcp__s__ t",
    ] {
        assert_eq!(of(name, &input), None, "{name}");
    }
}

#[test]
fn file_tools_keep_the_exact_path() {
    let cases = [
        ("Edit", "/m/brief.docx", Some("Edit(//m/brief.docx)")),
        ("Write", "/m/new file.txt", Some("Edit(//m/new file.txt)")),
        ("MultiEdit", "/m/a.md", Some("Edit(//m/a.md)")),
        ("Read", "/etc/hosts", Some("Read(//etc/hosts)")),
        ("Edit", "relative/a.txt", None),
        ("Edit", "/m/*.docx", None),
        ("Edit", "/m/a[1].txt", None),
        ("Edit", "/m/../x", None),
        ("Edit", "/m//x", None),
        ("Edit", "/m/draft (2).docx", None),
        ("Edit", "/m/trailing ", None),
        ("NotebookEdit", "/m/a.ipynb", None),
    ];
    for (tool, path, want) in cases {
        let got = of(tool, &json!({"file_path": path, "content": "x"}));
        assert_eq!(got.as_deref(), want, "{tool} {path}");
        if let Some(rule) = &got {
            assert!(well_formed(rule), "{rule}");
        }
    }
}

#[test]
fn windows_paths_become_posix_ones() {
    let input = |path: &str| json!({ "file_path": path });
    let windows = |path: &str| rule("Edit", Some(&input(path)), true);
    assert_eq!(
        windows(r"C:\Users\A\Matter\brief.docx").as_deref(),
        Some("Edit(//c/Users/A/Matter/brief.docx)")
    );
    assert_eq!(windows("D:/m/a.txt").as_deref(), Some("Edit(//d/m/a.txt)"));
    for path in [r"\\server\share\a.txt", "C:a.txt", "/m/a.txt", "relative"] {
        assert_eq!(windows(path), None, "{path}");
    }
}

#[test]
fn other_tools_keep_nothing() {
    let input = json!({"command": "npm test", "file_path": "/m/a", "url": "https://a.b/"});
    for tool in [
        "PowerShell",
        "Skill",
        "ExitPlanMode",
        "WebSearch",
        "Task",
        "bash",
        "",
    ] {
        assert_eq!(of(tool, &input), None, "{tool}");
    }
}

#[test]
fn only_rules_this_module_makes_are_well_formed() {
    for rule in [
        "Bash",
        "Bash(npm *)",
        "Bash(npm test:*)",
        "Edit(/m/a)",
        "Edit(//m/*.docx)",
        "Read(~/x)",
        "WebFetch(domain:*.gov.in)",
        "WebFetch",
        "mcp__*",
        "Write(//m/a)",
    ] {
        assert!(!well_formed(rule), "{rule}");
    }
}

#[test]
fn the_file_sits_beside_the_notes_and_keeps_each_rule_once() {
    let dir = TempDir::new("always_file");
    let notes = dir.path().join("agent-state").join("m1").join("notes");
    crate::write::create_dir_all(&notes).unwrap();
    let file = file_beside(&notes).unwrap();
    assert_eq!(file, dir.path().join("agent-state").join("m1").join(FILE));
    assert_eq!(read(&file), Vec::<String>::new());
    store_now(&file, "Bash(npm test)".to_owned()).unwrap();
    store_now(&file, "Bash(npm test)".to_owned()).unwrap();
    store_now(&file, "WebFetch(domain:sci.gov.in)".to_owned()).unwrap();
    store_now(&file, "Bash(*)".to_owned()).unwrap();
    assert_eq!(
        read(&file),
        ["Bash(npm test)", "WebFetch(domain:sci.gov.in)"]
    );
    // Nothing but the file was made, and nothing in the notes.
    assert_eq!(std::fs::read_dir(&notes).unwrap().count(), 0);
}

#[test]
fn a_hand_edited_file_gives_only_its_well_formed_rules() {
    let dir = TempDir::new("always_file");
    let file = dir.path().join(FILE);
    let many: Vec<String> = (0..MAX_RULES + 5)
        .map(|n| format!("Bash(echo {n})"))
        .collect();
    let mut rules = vec![json!("Bash(*)"), json!(1), json!("Read(//etc/hosts)")];
    rules.extend(many.iter().map(|rule| json!(rule)));
    crate::write::write_file(&file, json!({ "allow": rules }).to_string().as_bytes()).unwrap();
    assert!(
        read(&file).is_empty(),
        "a non-string entry fails the whole list"
    );
    rules.remove(1);
    crate::write::write_file(&file, json!({ "allow": rules }).to_string().as_bytes()).unwrap();
    let got = read(&file);
    assert_eq!(got.len(), MAX_RULES);
    assert_eq!(got[0], "Read(//etc/hosts)");
    crate::write::write_file(&file, b"not json").unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
}

#[test]
fn the_data_fence_denies_the_file_to_claude() {
    assert!(
        crate::agent::data_fence::DENIED.contains(&"agent-state/*/claude-allow.json"),
        "the fixed deny rules name the file, made after a launch or not"
    );
}

#[test]
fn rules_go_after_the_allow_rules_already_in_the_settings() {
    let mut meta = json!({"claudeCode": {"options": {"settings": {
        "permissions": {"allow": ["Read(//x/**)"], "deny": ["Edit(//d/**)"]}
    }}}})
    .as_object()
    .unwrap()
    .clone();
    allow_in_meta(&mut meta, ["Bash(npm test)", "Read(//x/**)"]);
    assert_eq!(
        meta["claudeCode"]["options"]["settings"]["permissions"],
        json!({"allow": ["Read(//x/**)", "Bash(npm test)"], "deny": ["Edit(//d/**)"]})
    );
    let mut empty = serde_json::Map::new();
    allow_in_meta(&mut empty, []);
    assert!(empty.is_empty(), "no rules, no change");
    let mut odd = json!({"claudeCode": "x"}).as_object().unwrap().clone();
    allow_in_meta(&mut odd, ["mcp__s__t"]);
    assert_eq!(
        odd["claudeCode"]["options"]["settings"]["permissions"]["allow"],
        json!(["mcp__s__t"])
    );
}

/// A path rule stays within the matter folder, so it never widens past the data fence: not the
/// data folder, not a sibling folder that only starts with the matter's name, not the matter's
/// `.clauseo` or a `.claude`; a rule that names no path passes.
#[test]
fn a_path_rule_outside_the_matter_is_never_within() {
    let roots = matter_roots(["/cases/m/", "/cases/m", "~/cases/m", "relative"], false);
    assert_eq!(roots, ["//cases/m"]);
    let cases = [
        ("Edit(//cases/m/brief.txt)", true),
        ("Read(//cases/m/sub/a.pdf)", true),
        ("Bash(npm test)", true),
        ("WebFetch(domain:example.com)", true),
        ("mcp__case_law__search", true),
        ("Edit(//cases/m)", false),
        ("Edit(//cases/m-other/a.txt)", false),
        ("Edit(//cases/other/a.txt)", false),
        ("Read(//data/agent-state/x/notes/a.md)", false),
        ("Edit(//data/agent-state/x/claude-allow.json)", false),
        ("Edit(//cases/m/.clauseo/matter.json)", false),
        ("Edit(//cases/m/.Claude/settings.local.json)", false),
        ("Edit(//cases/m/sub/.claude/settings.json)", false),
        ("Edit(//cases/M/brief.txt)", false),
    ];
    for (rule, want) in cases {
        assert_eq!(within(rule, &roots, false), want, "{rule}");
    }
    assert!(!within("Edit(//cases/m/a.txt)", &[], false));
    let windows = matter_roots([r"C:\Cases\M"], true);
    assert_eq!(windows, ["//c/Cases/M"]);
    assert!(within("Edit(//c/cases/m/brief.txt)", &windows, true));
    assert!(!within("Edit(//c/cases/m2/brief.txt)", &windows, true));
}
