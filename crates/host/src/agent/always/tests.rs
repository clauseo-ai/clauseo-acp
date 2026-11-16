//! The rule a request keeps (`always.rs`): one exact rule per tool, none for anything that
//! would not stay exact, commands per agent. The file's read and write: `store_tests.rs`.

use clauseo_protocol::{AgentKind, AlwaysKind, ToolKind};
use serde_json::{Value, json};

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_connectors::MemoryStore;

use crate::pipeline::tests::support::TempDir;

/// A secret store for the key, process-wide (each test runs in a process of its own).
pub(super) fn keyed() -> Arc<MemoryStore> {
    let store = Arc::new(MemoryStore::new());
    super::use_secrets(Some(
        Arc::clone(&store) as Arc<dyn clauseo_connectors::SecretStore>
    ));
    store
}

/// `<dir>/agent-state/<matter>/claude-allow.json`, its folder made.
pub(super) fn in_matter(dir: &TempDir, matter: &str) -> PathBuf {
    let folder = dir.path().join("agent-state").join(matter);
    crate::write::create_dir_all(&folder).unwrap();
    folder.join(FILE)
}

use super::{
    EDIT_ALL, FILE, Kept, NotKept, Request, matter_roots, needs_real, rule, rule_path, run_parts,
    well_formed, within,
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
        ("https://8.8.8.8/", Some("WebFetch(domain:8.8.8.8)")),
        // This machine and the lawyer's own network are never kept (`site::public`).
        ("https://10.0.0.1/", None),
        ("http://localhost:3000/", None),
        ("http://intranet/", None),
        ("http://printer.local/", None),
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

/// An MCP tool's card names its server and its tool apart, never the whole
/// `mcp__<server>__<tool>`; the rule keeps the whole name.
#[test]
fn a_tool_offer_names_its_server_and_tool() {
    for (name, server, tool) in [
        ("mcp__case-law__search_law", "case-law", "search_law"),
        ("mcp__court__find__case", "court", "find__case"),
    ] {
        let kept = claude(name, Some(&json!({})), false).unwrap();
        assert_eq!(kept.rule, name);
        assert_eq!(kept.offer.kind, AlwaysKind::Tool);
        assert_eq!(kept.offer.server.as_deref(), Some(server), "{name}");
        assert_eq!(kept.offer.tool.as_deref(), Some(tool), "{name}");
        assert_eq!(kept.offer.shown, None, "{name}");
    }
}

#[test]
fn web_search_and_powershell_are_claudes() {
    let search = claude("WebSearch", Some(&json!({"query": "x"})), false).unwrap();
    assert_eq!(search.rule, "WebSearch");
    assert_eq!(search.offer.kind, AlwaysKind::Tool);
    assert_eq!(search.offer.tool.as_deref(), Some("WebSearch"));
    assert_eq!((search.offer.server, search.offer.shown), (None, None));
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
    // Codex's own shell wrapper is read through once, at its system path; its script must be
    // one plain command of a listed program.
    assert_eq!(
        run("/bin/zsh -lc 'wc -l a.txt'", "/m").as_deref(),
        Ok("Run(/bin/zsh -lc 'wc -l a.txt')")
    );
    assert_eq!(run("zsh -lc 'wc -l a.txt'", "/m"), Err(NotKept::Refused));
    assert_eq!(run("bash -lc 'ls && rm a'", "/m"), Err(NotKept::Refused));
    for (command, cwd, why) in [
        ("ls", "/elsewhere", NotKept::OutsideMatter),
        ("ls", "/m/.clauseo", NotKept::OutsideMatter),
        ("ls", "/m/../x", NotKept::OutsideMatter),
        ("cat ../other/a.txt", "/m", NotKept::Refused),
        ("cat /elsewhere/a.txt", "/m", NotKept::OutsideMatter),
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

/// A `Read` card shows the file relative to the matter folder, `/`-separated, nested or not, on
/// either system; the rule keeps the whole path.
#[test]
fn a_read_shows_the_file_relative_to_the_matter() {
    let cases = [
        ("/m/Drafts/2026/brief.docx", false, "Drafts/2026/brief.docx"),
        ("/m/notes.txt", false, "notes.txt"),
        (r"C:\m\Drafts\Brief.docx", true, "Drafts/Brief.docx"),
    ];
    for (file, windows, shown) in cases {
        let kept = claude("Read", Some(&json!({ "file_path": file })), windows).unwrap();
        assert_eq!(kept.offer.kind, AlwaysKind::Read);
        assert_eq!(kept.offer.shown.as_deref(), Some(shown), "{file}");
        assert!(kept.rule.starts_with("Read(//"), "{}", kept.rule);
    }
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

/// A Codex request that asks for more than the command (`additionalPermissions`, a site, any
/// key codex-acp adds) keeps nothing: a rule never grants more than the plain run it was kept
/// for.
#[test]
fn a_codex_command_with_more_permissions_keeps_nothing() {
    for extra in [
        json!({"command": "ls", "cwd": "/m", "additionalPermissions": {"fileSystem": {"write": ["/"]}}}),
        json!({"command": "ls", "cwd": "/m", "additionalPermissions": {}}),
        json!({"command": "ls", "cwd": "/m", "url": "https://a.in"}),
        json!({"command": "ls", "cwd": "/m", "sandbox": "none"}),
    ] {
        assert_eq!(
            codex(ToolKind::Execute, &extra),
            Err(NotKept::NoRule),
            "{extra}"
        );
    }
    assert!(codex(ToolKind::Execute, &json!({"command": "ls", "cwd": "/m"})).is_ok());
}

/// A command rule carries what the matter thread checks by real path: its program, its working
/// folder (Claude's is the matter folder) and its path words.
#[test]
fn a_command_rule_carries_what_it_reaches() {
    let kept = claude(
        "Bash",
        Some(&json!({"command": "cp a.txt Drafts/b.txt"})),
        false,
    )
    .unwrap();
    let command = kept.command.unwrap();
    assert_eq!(command.program.name, "cp");
    assert_eq!(command.cwd, "/m");
    assert_eq!(command.paths, ["a.txt", "Drafts/b.txt"]);
    assert!(!command.codex);
    assert!(needs_real(&kept.rule));
    let run = codex(
        ToolKind::Execute,
        &json!({"command": "grep -n term a.txt", "cwd": "/m/Drafts"}),
    )
    .unwrap();
    let command = run.command.unwrap();
    assert_eq!(command.program.name, "grep");
    assert_eq!(command.cwd, "/m/Drafts");
    assert_eq!(command.paths, ["a.txt"], "the pattern is not a path");
    assert!(command.codex);
    for rule in ["Bash(ls)", "PowerShell(Get-ChildItem)", "Run(ls) in a"] {
        assert!(needs_real(rule), "{rule}");
    }
    let site = of("WebFetch", &json!({"url": "https://sci.gov.in/"})).unwrap();
    assert!(!needs_real(&site));
}

/// No kept rule reaches a name an agent reads its own settings or instructions from.
#[test]
fn an_agents_own_files_are_never_within() {
    let roots = matter_roots(["/cases/m"], false);
    for name in super::real::OWN_NAMES {
        let rule = format!("Read(//cases/m/{name})");
        assert!(!within(&rule, &roots, false), "{rule}");
        let nested = format!("Read(//cases/m/sub/{name}/x)");
        assert!(!within(&nested, &roots, false), "{nested}");
    }
    let codex = |cwd: &str| codex(ToolKind::Execute, &json!({"command": "ls", "cwd": cwd}));
    assert_eq!(codex("/m/.codex"), Err(NotKept::OutsideMatter));
    assert_eq!(codex("/m/.agents/skills"), Err(NotKept::OutsideMatter));
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
