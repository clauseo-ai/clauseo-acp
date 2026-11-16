//! The rule a request keeps (`always.rs`): one exact rule per tool, none for anything that
//! would not stay exact, commands per agent; the file's read and write.

use clauseo_protocol::{AgentKind, AlwaysKind, ToolKind};
use serde_json::{Value, json};

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_connectors::{MemoryStore, SecretStore as _};

use crate::pipeline::tests::support::TempDir;

/// A secret store for the key, process-wide (each test runs in a process of its own).
fn keyed() -> Arc<MemoryStore> {
    let store = Arc::new(MemoryStore::new());
    super::use_secrets(Some(
        Arc::clone(&store) as Arc<dyn clauseo_connectors::SecretStore>
    ));
    store
}

/// `<dir>/agent-state/<matter>/claude-allow.json`, its folder made.
fn in_matter(dir: &TempDir, matter: &str) -> PathBuf {
    let folder = dir.path().join("agent-state").join(matter);
    crate::write::create_dir_all(&folder).unwrap();
    folder.join(FILE)
}

use super::store::{read, remove_now, store_now};
use super::{
    EDIT_ALL, FILE, Kept, NotKept, Request, file_beside, matter_roots, needs_real, rule, rule_path,
    run_parts, well_formed, within,
};

pub(super) fn claude(tool: &str, input: Option<&Value>, windows: bool) -> Result<Kept, NotKept> {
    let matter = [if windows { "C:/m" } else { "/m" }.to_owned()];
    rule(&Request {
        agent: AgentKind::Claude,
        tool: Some(tool),
        kind: None,
        raw_input: input,
        matter: &matter,
        windows,
    })
}

pub(super) fn of(tool: &str, input: &Value) -> Option<String> {
    claude(tool, Some(input), false).ok().map(|kept| kept.rule)
}

fn codex(kind: ToolKind, input: &Value) -> Result<Kept, NotKept> {
    rule(&Request {
        agent: AgentKind::Codex,
        tool: None,
        kind: Some(kind),
        raw_input: Some(input),
        matter: &["/var/m".to_owned(), "/m".to_owned()],
        windows: false,
    })
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
    assert_eq!(claude("Bash", None, false), Err(NotKept::Refused));
    // An argument outside the matter, by its text.
    assert_eq!(
        claude("Bash", Some(&json!({"command": "cat /etc/hosts"})), false),
        Err(NotKept::OutsideMatter)
    );
    assert_eq!(
        of("Bash", &json!({"command": "cat /m/notes.txt"})).as_deref(),
        Some("Bash(cat /m/notes.txt)")
    );
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
fn file_tools_keep_the_whole_matter_and_read_the_exact_path() {
    for tool in ["Edit", "Write", "MultiEdit", "NotebookEdit"] {
        let kept = claude(tool, Some(&json!({"file_path": "/m/a.md"})), false).unwrap();
        assert_eq!(kept.rule, EDIT_ALL);
        assert_eq!(kept.offer.kind, AlwaysKind::FileEdit);
        assert_eq!(kept.offer.shown, None);
        assert!(needs_real(&kept.rule));
    }
    let cases = [
        ("/etc/hosts", Some("Read(//etc/hosts)")),
        ("relative/a.txt", None),
        ("/m/*.docx", None),
        ("/m/a[1].txt", None),
        ("/m/../x", None),
        ("/m//x", None),
        ("/m/draft (2).docx", None),
        ("/m/trailing ", None),
    ];
    for (path, want) in cases {
        let got = of("Read", &json!({"file_path": path}));
        assert_eq!(got.as_deref(), want, "{path}");
        if let Some(rule) = &got {
            assert!(well_formed(rule), "{rule}");
            assert!(needs_real(rule));
        }
    }
    assert!(well_formed(EDIT_ALL));
    assert!(
        !well_formed("Edit(//m/a.md)"),
        "per-file edits are no longer kept"
    );
}

#[test]
fn web_search_and_powershell_are_claudes() {
    let search = claude("WebSearch", Some(&json!({"query": "x"})), false).unwrap();
    assert_eq!(search.rule, "WebSearch");
    assert_eq!(search.offer.kind, AlwaysKind::Tool);
    assert!(well_formed("WebSearch"));
    let shell = json!({"command": "Get-ChildItem"});
    let kept = claude("PowerShell", Some(&shell), true).unwrap();
    assert_eq!(kept.rule, "PowerShell(Get-ChildItem)");
    assert!(well_formed(&kept.rule));
    for line in [
        "Get-ChildItem; rm x",
        "Invoke-Expression x",
        "$x",
        "iex (gc a)",
    ] {
        let input = json!({ "command": line });
        assert!(claude("PowerShell", Some(&input), true).is_err(), "{line}");
    }
}

#[test]
fn a_codex_command_keeps_its_text_and_folder() {
    let run = |command: &str, cwd: &str| {
        codex(ToolKind::Execute, &json!({"command": command, "cwd": cwd})).map(|kept| kept.rule)
    };
    assert_eq!(
        run("wc -l notes.txt", "/m").as_deref(),
        Ok("Run(wc -l notes.txt)")
    );
    assert_eq!(
        run("mdls a.pdf", "/m/Drafts/2026").as_deref(),
        Ok("Run(mdls a.pdf) in Drafts/2026")
    );
    let kept = codex(
        ToolKind::Execute,
        &json!({"command": "ls", "cwd": "/m/Drafts"}),
    )
    .unwrap();
    assert_eq!(kept.offer.kind, AlwaysKind::Command);
    assert_eq!(kept.offer.shown.as_deref(), Some("ls"));
    assert_eq!(kept.offer.cwd.as_deref(), Some("Drafts"));
    // Codex's own shell wrapper is read through once; its script must be one plain command.
    assert_eq!(
        run("zsh -lc 'wc -l a.txt'", "/m").as_deref(),
        Ok("Run(zsh -lc 'wc -l a.txt')")
    );
    assert_eq!(run("bash -lc 'ls && rm a'", "/m"), Err(NotKept::Refused));
    for (command, cwd, why) in [
        ("ls", "/elsewhere", NotKept::OutsideMatter),
        ("ls", "/m/.clauseo", NotKept::OutsideMatter),
        ("ls", "/m/../x", NotKept::OutsideMatter),
        ("cat ../other/a.txt", "/m", NotKept::OutsideMatter),
        ("cd a && ls", "/m", NotKept::Refused),
        ("python3 x.py", "/m", NotKept::Refused),
        ("git status", "/m", NotKept::Refused),
        // The three real Codex cards of the live record (`/tmp/proposals/result.md`, 0063).
        (
            r#"cp witness-statement.docx "/Users/Shared/outbox/witness-statement.docx""#,
            "/m",
            NotKept::OutsideMatter,
        ),
        (
            r#"python3 -c "from pathlib import Path; Path('/Users/Shared/o/n.txt').write_text('x')""#,
            "/m",
            NotKept::Refused,
        ),
        (
            r#"cupsfilter "/m/letter.txt" > "/Users/Shared/outbox/letter.pdf""#,
            "/m",
            NotKept::Refused,
        ),
    ] {
        assert_eq!(run(command, cwd), Err(why), "{command} in {cwd}");
    }
    let net = json!({"command": "ls", "cwd": "/m", "url": "https://a.in"});
    assert_eq!(codex(ToolKind::Execute, &net), Err(NotKept::NoRule));
    let edit = codex(ToolKind::Edit, &json!({})).map(|kept| kept.rule);
    assert_eq!(edit.as_deref(), Ok(EDIT_ALL));
    assert_eq!(codex(ToolKind::Fetch, &json!({})), Err(NotKept::NoRule));
    for rule in ["Run(wc -l notes.txt)", "Run(mdls a.pdf) in Drafts/2026"] {
        assert!(well_formed(rule), "{rule}");
    }
    for rule in [
        "Run(ls) in ../x",
        "Run(ls) in /abs",
        "Run(ls)x",
        "Run(ls | wc)",
        "Run(ls) in ",
    ] {
        assert!(!well_formed(rule), "{rule}");
    }
    assert_eq!(run_parts("Run(ls) in a/b"), Some(("ls", Some("a/b"))));
}

/// Commands are kept per agent: a Claude command's rule never answers a Codex card.
#[test]
fn commands_are_kept_per_agent() {
    let input = json!({"command": "wc -l notes.txt", "cwd": "/m"});
    let claude = of("Bash", &input).unwrap();
    let codex = codex(ToolKind::Execute, &input).unwrap().rule;
    assert_ne!(claude, codex);
}

#[test]
fn windows_paths_become_posix_ones() {
    let input = |path: &str| json!({ "file_path": path });
    let windows = |path: &str| {
        claude("Read", Some(&input(path)), true)
            .ok()
            .map(|k| k.rule)
    };
    assert_eq!(
        windows(r"C:\Users\A\Matter\brief.docx").as_deref(),
        Some("Read(//c/Users/A/Matter/brief.docx)")
    );
    assert_eq!(windows("D:/m/a.txt").as_deref(), Some("Read(//d/m/a.txt)"));
    for path in [r"\\server\share\a.txt", "C:a.txt", "/m/a.txt", "relative"] {
        assert_eq!(windows(path), None, "{path}");
    }
}

#[test]
fn other_tools_keep_nothing() {
    let input = json!({"command": "wc -l notes.txt", "file_path": "/m/a", "url": "https://a.b/"});
    for tool in ["Skill", "ExitPlanMode", "Task", "bash", ""] {
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
        "Read(//m/*.docx)",
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
    keyed();
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

/// A file this host did not write for this matter holds nothing: edited by hand, planted,
/// version 1, copied from another matter, or signed under another key.
#[test]
fn a_file_without_this_hosts_mac_for_this_matter_holds_nothing() {
    let store = keyed();
    let dir = TempDir::new("always_planted");
    let file = in_matter(&dir, "m1");
    store_now(&file, "Bash(ls)".to_owned(), 1).unwrap();
    assert_eq!(read(&file), ["Bash(ls)"]);
    let good = std::fs::read_to_string(&file).unwrap();
    let mut value: Value = serde_json::from_str(&good).unwrap();
    assert_eq!(value["version"], json!(2));
    // A rule added by hand, or one removed by hand: the MAC no longer fits.
    value["allow"] = json!(["Bash(ls)", "Bash(pwd)"]);
    crate::write::write_file(&file, value.to_string().as_bytes()).unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
    value["allow"] = json!([]);
    crate::write::write_file(&file, value.to_string().as_bytes()).unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
    // Version 1, as #1109 wrote it.
    let v1 = json!({"version": 1, "allow": ["Bash(ls)"]});
    crate::write::write_file(&file, v1.to_string().as_bytes()).unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
    // The same file copied into another matter: its MAC names the first.
    crate::write::write_file(&file, good.as_bytes()).unwrap();
    assert_eq!(read(&file), ["Bash(ls)"]);
    let other = in_matter(&dir, "m2");
    crate::write::write_file(&other, good.as_bytes()).unwrap();
    assert_eq!(read(&other), Vec::<String>::new());
    // Another key (another machine's Keychain) makes another MAC.
    store.set(super::store::KEY_ACCOUNT, &[7_u8; 32]).unwrap();
    super::use_secrets(Some(store));
    assert_eq!(read(&file), Vec::<String>::new());
    // Not JSON at all.
    crate::write::write_file(&file, b"not json").unwrap();
    assert_eq!(read(&file), Vec::<String>::new());
}

/// Without a secret store nothing is read, kept or offered.
#[test]
fn no_secret_store_keeps_nothing() {
    super::use_secrets(None);
    let dir = TempDir::new("always_no_store");
    let notes = dir.path().join("agent-state").join("m1").join("notes");
    crate::write::create_dir_all(&notes).unwrap();
    assert_eq!(super::read_beside(&notes), None);
    let file = file_beside(&notes).unwrap();
    assert!(store_now(&file, "Bash(ls)".to_owned(), 1).is_err());
    assert!(!file.exists());
    let store = keyed();
    assert_eq!(super::read_beside(&notes), Some(Vec::new()));
    // The key is made once and kept in the store.
    let key = store.get(super::store::KEY_ACCOUNT).unwrap().unwrap();
    assert_eq!(key.len(), 32);
}

/// A forgotten matter's file is deleted; a missing one is fine.
#[test]
fn a_forgotten_matters_file_is_deleted() {
    keyed();
    let dir = TempDir::new("always_forget");
    let file = in_matter(&dir, "m1");
    store_now(&file, "Bash(ls)".to_owned(), 1).unwrap();
    super::store::delete_now(&file).unwrap();
    assert!(!file.exists());
    super::store::delete_now(&file).unwrap();
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
        ("Read(//cases/m/brief.txt)", true),
        ("Read(//cases/m/sub/a.pdf)", true),
        ("Bash(wc -l notes.txt)", true),
        ("WebFetch(domain:example.com)", true),
        ("mcp__case_law__search", true),
        ("Read(//cases/m)", false),
        ("Read(//cases/m-other/a.txt)", false),
        ("Read(//cases/other/a.txt)", false),
        ("Read(//data/agent-state/x/notes/a.md)", false),
        ("Read(//data/agent-state/x/claude-allow.json)", false),
        ("Read(//cases/m/.clauseo/matter.json)", false),
        ("Read(//cases/m/.Claude/settings.local.json)", false),
        ("Read(//cases/m/sub/.claude/settings.json)", false),
        ("Read(//cases/M/brief.txt)", false),
    ];
    for (rule, want) in cases {
        assert_eq!(within(rule, &roots, false), want, "{rule}");
    }
    assert!(!within("Read(//cases/m/a.txt)", &[], false));
    let windows = matter_roots([r"C:\Cases\M"], true);
    assert_eq!(windows, ["//c/Cases/M"]);
    assert!(within("Read(//c/cases/m/brief.txt)", &windows, true));
    assert!(!within("Read(//c/cases/m2/brief.txt)", &windows, true));
}

/// Why nothing was kept: no rule form for the tool, or text that would not stay one exact rule.
#[test]
fn a_request_with_no_rule_says_why() {
    let task = json!({"prompt": "x"});
    assert_eq!(claude("Task", Some(&task), false), Err(NotKept::NoRule));
    let star = json!({"command": "ls *.docx"});
    assert_eq!(claude("Bash", Some(&star), false), Err(NotKept::Refused));
    let relative = json!({"file_path": "a.txt"});
    assert_eq!(
        claude("Read", Some(&relative), false),
        Err(NotKept::Refused)
    );
}

/// Windows drops a name's trailing dots and spaces, so `.claude.` opens `.claude`: no rule is
/// made for such a name, and one read back is compared without them.
#[test]
fn windows_trailing_dots_never_open_claude() {
    let input = |path: &str| json!({ "file_path": path });
    let made = |path: &str| claude("Read", Some(&input(path)), true).ok();
    assert_eq!(made(r"C:\Cases\M\.claude.\x"), None);
    assert_eq!(made(r"C:\Cases\M\a.\x"), None);
    assert_eq!(made(r"C:\Cases\M\.claude \x"), None);
    let roots = matter_roots([r"C:\Cases\M"], true);
    assert!(!within("Read(//c/Cases/M/.claude./x)", &roots, true));
    assert!(!within("Read(//c/Cases/M/.CLAUDE. ./x)", &roots, true));
    assert!(within("Read(//c/Cases/M/work/x)", &roots, true));
    assert_eq!(
        rule_path("Read(//c/Cases/M/x)", true),
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
    // A file inside the matter that is a link to a file outside it: its real path is outside,
    // so the matter-wide `Edit(*)` never covers it, nor a new file under a linked folder.
    let outside = other.join("plist.txt");
    crate::write::write_file(&outside, b"x").unwrap();
    crate::write::symlink(&outside, &matter.join("linked.txt")).unwrap();
    assert!(!real_within(&matter.join("linked.txt"), &matter, false));
    crate::write::symlink(&other, &matter.join("x")).unwrap();
    assert!(!real_within(&matter.join("x/new.plist"), &matter, false));
    assert!(super::needs_real(super::EDIT_ALL));
}

/// A file this host cannot take is never written over: the next write sets it aside as
/// `.bad-<n>` and starts empty; a removal leaves it alone.
#[test]
fn a_bad_file_is_set_aside_not_overwritten() {
    keyed();
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
    keyed();
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
    keyed();
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
