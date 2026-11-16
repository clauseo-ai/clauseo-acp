//! The rule a Claude request keeps (`always.rs`): one exact rule per tool, none for anything
//! that would not stay exact; the file's read and write; the rules in `_meta`.

use serde_json::{Value, json};

use crate::pipeline::tests::support::TempDir;

use super::store::{read, remove_now, store_now};
use super::{
    FILE, MAX_RULES, NotKept, file_beside, matter_roots, rule, rule_path, well_formed, within,
};

fn of(tool: &str, input: &Value) -> Option<String> {
    rule(tool, Some(input), false).ok()
}

#[test]
fn bash_keeps_the_exact_command() {
    let cases = [
        ("wc -l notes.txt", Some("Bash(wc -l notes.txt)")),
        (
            "mdls -name kMDItemTitle 'Brief.pdf'",
            Some("Bash(mdls -name kMDItemTitle 'Brief.pdf')"),
        ),
        // One plain command only (`plain.rs`): no chain, pipe or wrapper.
        ("git status --short", None),
        ("cat a.txt | wc -l", None),
        ("ls && rm a", None),
        ("python3 x.py", None),
        ("bash -c ls", None),
        ("echo $(date)", None),
        ("ls *.docx", None),
        ("npm run:*", None),
        ("printf 'a\\n'", None),
        ("echo a\nrm -rf x", None),
        (" wc -l notes.txt", None),
        ("", None),
    ];
    for (command, want) in cases {
        let got = of("Bash", &json!({"command": command, "description": "d"}));
        assert_eq!(got.as_deref(), want, "{command:?}");
        if let Some(rule) = &got {
            assert!(well_formed(rule), "{rule}");
        }
    }
    assert_eq!(of("Bash", &json!({"cmd": "wc -l notes.txt"})), None);
    assert_eq!(of("Bash", &json!({"command": 1})), None);
    assert_eq!(rule("Bash", None, false), Err(NotKept::Refused));
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
    let windows = |path: &str| rule("Edit", Some(&input(path)), true).ok();
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
    let input = json!({"command": "wc -l notes.txt", "file_path": "/m/a", "url": "https://a.b/"});
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
        "Bash(wc -l notes.txt:*)",
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
    store_now(&file, "Bash(wc -l notes.txt)".to_owned(), 1).unwrap();
    store_now(&file, "Bash(wc -l notes.txt)".to_owned(), 2).unwrap();
    store_now(&file, "WebFetch(domain:sci.gov.in)".to_owned(), 3).unwrap();
    store_now(&file, "Bash(*)".to_owned(), 4).unwrap();
    assert_eq!(
        read(&file),
        ["Bash(wc -l notes.txt)", "WebFetch(domain:sci.gov.in)"]
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
    let kept = |rules: &[Value]| json!({ "version": 1, "allow": rules }).to_string();
    crate::write::write_file(&file, kept(&rules).as_bytes()).unwrap();
    assert!(
        read(&file).is_empty(),
        "a non-string entry fails the whole list"
    );
    rules.remove(1);
    crate::write::write_file(&file, kept(&rules).as_bytes()).unwrap();
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
        ("Bash(wc -l notes.txt)", true),
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

/// Why nothing was kept: no rule form for the tool, or text that would not stay one exact rule.
#[test]
fn a_request_with_no_rule_says_why() {
    let shell = json!({"command": "Get-ChildItem"});
    assert_eq!(rule("PowerShell", Some(&shell), true), Err(NotKept::NoRule));
    let star = json!({"command": "ls *.docx"});
    assert_eq!(rule("Bash", Some(&star), false), Err(NotKept::Refused));
    let relative = json!({"file_path": "a.txt"});
    assert_eq!(rule("Edit", Some(&relative), false), Err(NotKept::Refused));
}

/// Windows drops a name's trailing dots and spaces, so `.claude.` opens `.claude`: no rule is
/// made for such a name, and one read back is compared without them.
#[test]
fn windows_trailing_dots_never_open_claude() {
    let input = |path: &str| json!({ "file_path": path });
    let made = |path: &str| rule("Edit", Some(&input(path)), true).ok();
    assert_eq!(made(r"C:\Cases\M\.claude.\x"), None);
    assert_eq!(made(r"C:\Cases\M\a.\x"), None);
    assert_eq!(made(r"C:\Cases\M\.claude \x"), None);
    let roots = matter_roots([r"C:\Cases\M"], true);
    assert!(!within("Edit(//c/Cases/M/.claude./x)", &roots, true));
    assert!(!within("Edit(//c/Cases/M/.CLAUDE. ./x)", &roots, true));
    assert!(within("Edit(//c/Cases/M/work/x)", &roots, true));
    assert_eq!(
        rule_path("Edit(//c/Cases/M/x)", true),
        Some(std::path::PathBuf::from("c:/Cases/M/x"))
    );
    assert_eq!(rule_path("Bash(ls)", true), None);
}

/// The real path decides: a folder of the matter swapped for a link to another matter, or to a
/// `.claude` folder, is outside; a file not made yet is judged by its folder.
#[cfg(unix)]
#[test]
fn a_link_swapped_into_the_matter_is_outside() {
    use super::real_within;
    let dir = TempDir::new("always_real");
    let matter = dir.path().join("m");
    let other = dir.path().join("other");
    crate::write::create_dir_all(&matter.join("work")).unwrap();
    crate::write::create_dir_all(&matter.join(".claude")).unwrap();
    crate::write::create_dir_all(&other).unwrap();
    let file = matter.join("work/out.txt");
    crate::write::write_file(&file, b"x").unwrap();
    assert!(real_within(&file, &matter, false));
    assert!(real_within(&matter.join("work/new.txt"), &matter, false));
    assert!(!real_within(&matter.join("gone/new.txt"), &matter, false));
    assert!(!real_within(&matter, &matter, false));
    assert!(!real_within(
        &matter.join(".claude/settings.json"),
        &matter,
        false
    ));
    crate::write::remove_dir_all(&matter.join("work")).unwrap();
    crate::write::symlink(&other, &matter.join("work")).unwrap();
    assert!(!real_within(&file, &matter, false));
    crate::write::remove_file(&matter.join("work")).unwrap();
    crate::write::symlink(&matter.join(".claude"), &matter.join("work")).unwrap();
    assert!(!real_within(&file, &matter, false));
}

/// A file naming no `version`, or one other than 1, reads as no rules, and is set aside like
/// any file this host cannot take.
#[test]
fn a_file_of_another_version_reads_as_no_rules() {
    let dir = TempDir::new("always_version");
    let file = dir.path().join(FILE);
    for text in [
        json!({ "allow": ["Bash(ls)"] }),
        json!({ "version": 2, "allow": ["Bash(ls)"] }),
        json!({ "version": "1", "allow": ["Bash(ls)"] }),
    ] {
        crate::write::write_file(&file, text.to_string().as_bytes()).unwrap();
        assert_eq!(read(&file), Vec::<String>::new(), "{text}");
    }
    crate::write::write_file(
        &file,
        json!({ "version": 1, "allow": ["Bash(ls)"] })
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(read(&file), ["Bash(ls)"]);
}

/// A file this host cannot take is never written over: the next write sets it aside as
/// `.bad-<n>` and starts empty; a removal leaves it alone.
#[test]
fn a_bad_file_is_set_aside_not_overwritten() {
    let dir = TempDir::new("always_bad");
    let file = dir.path().join(FILE);
    crate::write::write_file(&file, b"not json").unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
    assert!(!remove_now(&file, "Bash(ls)", 5).unwrap());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "not json");
    store_now(&file, "Bash(ls)".to_owned(), 6).unwrap();
    let aside = dir.path().join(format!("{FILE}.bad-1"));
    assert_eq!(std::fs::read_to_string(&aside).unwrap(), "not json");
    assert_eq!(read(&file), ["Bash(ls)"]);
    crate::write::write_file(&file, &vec![b' '; (1 << 20) + 1]).unwrap();
    store_now(&file, "Bash(pwd)".to_owned(), 7).unwrap();
    assert!(dir.path().join(format!("{FILE}.bad-2")).exists());
    assert_eq!(read(&file), ["Bash(pwd)"]);
}

/// The lawyer's removal takes one rule out and keeps the rest.
#[test]
fn a_removed_rule_is_gone_and_the_rest_stay() {
    let dir = TempDir::new("always_remove");
    let file = dir.path().join(FILE);
    store_now(&file, "Bash(ls)".to_owned(), 8).unwrap();
    store_now(&file, "Bash(pwd)".to_owned(), 9).unwrap();
    assert!(remove_now(&file, "Bash(ls)", 10).unwrap());
    assert!(!remove_now(&file, "Bash(ls)", 11).unwrap());
    assert_eq!(read(&file), ["Bash(pwd)"]);
}

/// An add whose write was still pending when the lawyer removed its rule runs after the removal
/// and never puts the rule back; an add the lawyer chose after the removal does.
#[test]
fn a_pending_add_never_undoes_a_removal() {
    let dir = TempDir::new("always_pending");
    let file = dir.path().join(FILE);
    store_now(&file, "Bash(pwd)".to_owned(), 1).unwrap();
    // The add numbered 2 reaches the lock only after the removal numbered 3.
    assert!(!remove_now(&file, "Bash(ls)", 3).unwrap());
    store_now(&file, "Bash(ls)".to_owned(), 2).unwrap();
    assert_eq!(read(&file), ["Bash(pwd)"]);
    // A removal that comes after a later add of the same rule leaves it.
    store_now(&file, "Bash(ls)".to_owned(), 5).unwrap();
    assert!(!remove_now(&file, "Bash(ls)", 4).unwrap());
    assert_eq!(read(&file), ["Bash(pwd)", "Bash(ls)"]);
}
