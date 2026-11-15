//! Claude's deny rules for a whole launch, matched as Claude Code matches them: every other
//! matter's folder and every place of `<data>` that is not the session's own are denied, each
//! spelling; the session's run folder, this matter's notes, its own tool results (a `WebFetch`'s
//! saved PDF among them), the plans folder and the matter folder are not.

use std::path::{Path, PathBuf};

use super::launch::claude_project_key;
use super::tests::layout;
use super::{
    LaunchDeny, claude_allow_rules, claude_deny_rules, claude_projects, launch_deny, rule_path,
};
use crate::agent::matter_fence::{MatterFence, Other};
use crate::write;

/// Whether one of `rules` denies `tool` on the absolute `path`. A `Tool(//p/**)` rule covers
/// what is under `p`; a `Tool(//p)` rule covers `p` and, read as broadly as gitignore reads a
/// folder's name, all under it too. A name with an unescaped `*` fits any name with its head and
/// tail; `\` escapes.
fn denied(rules: &[String], tool: &str, path: &Path) -> bool {
    let path = rule_path(path.to_str().unwrap(), cfg!(windows)).replace('\\', "");
    let names: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let open = format!("{tool}(//");
    rules
        .iter()
        .filter_map(|rule| rule.strip_prefix(&open)?.strip_suffix(')'))
        .any(|pattern| {
            let parts: Vec<&str> = pattern.split('/').collect();
            let (parts, below) = match parts.split_last() {
                Some((&"**", head)) => (head, true),
                _ => (parts.as_slice(), false),
            };
            let fits = |part: &str, name: &str| match part.split_once('*') {
                Some((head, tail)) if !part.contains("\\*") => {
                    name.len() >= head.len() + tail.len()
                        && name.starts_with(head)
                        && name.ends_with(tail)
                }
                _ => part.replace('\\', "") == name,
            };
            let long_enough = if below {
                names.len() > parts.len()
            } else {
                names.len() >= parts.len()
            };
            long_enough
                && parts
                    .iter()
                    .zip(&names)
                    .all(|(part, name)| fits(part, name))
        })
}

/// `path` canonical, then as given when that differs (macOS's `/var` is `/private/var`).
fn spellings(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![dunce::canonicalize(path).unwrap()];
    if out[0] != path {
        out.push(path.to_owned());
    }
    out
}

/// A matter id from `n`.
fn id(n: u128) -> clauseo_protocol::MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    clauseo_protocol::MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// The fence of `others` (folder, invited) for a session whose matter folder is spelled `own`
/// and `<data>` `data`.
fn matters(others: &[(PathBuf, bool)], own: &[&Path], data: &[&Path]) -> MatterFence {
    let asked = others
        .iter()
        .zip(1..)
        .map(|((folder, invited), n)| Other::new(id(n), format!("M{n}"), folder.clone(), *invited))
        .collect();
    MatterFence::asked(asked).resolve(own, data)
}

/// The rules of a Claude launch of session `s1` for matter `m1` at `root/matter`, with two
/// other matters at `root/other-a` and `root/other-b`, and `root` itself registered as a
/// matter too (it holds this one's folder and `<data>`, so its other entries are denied).
fn whole_launch(root: &Path) -> Vec<String> {
    let data = root.join("data");
    let matter = root.join("matter");
    let real_matter = dunce::canonicalize(&matter).unwrap();
    let home = data.join("agents/claude-acp/home");
    let own = [
        data.join("run").join("s1"),
        data.join("agent-state").join("m1").join("notes"),
        home.join("plans"),
        real_matter.clone(),
    ];
    let projects = claude_projects(&home, &[&real_matter, &matter]);
    let data_spellings = spellings(&data);
    let data_places: Vec<&Path> = data_spellings.iter().map(PathBuf::as_path).collect();
    let others = [root.join("other-a"), root.join("other-b"), root.to_owned()].map(|f| (f, false));
    let matters = matters(
        &others,
        &[real_matter.as_path(), matter.as_path()],
        &data_places,
    );
    let mut deny = launch_deny(data_spellings, &data, &own, &projects);
    deny.matters = matters;
    claude_deny_rules(&deny)
}

/// Every other matter's folder and its files, other sessions' run folders, other matters'
/// notes, Claude's login and every transcript are denied for Read and Edit under each spelling;
/// the session's own places, its own tool results and the matter folder are in no rule.
#[test]
fn a_launch_denies_other_matters_and_data_and_leaves_the_sessions_own() {
    let dir = layout("matters");
    let root = dir.path();
    let real_matter = dunce::canonicalize(root.join("matter")).unwrap();
    let own_key = claude_project_key(real_matter.to_str().unwrap());
    let project = |rest: &str| format!("data/agents/claude-acp/home/projects/{rest}");
    let deny = [
        "other-a/brief.pdf".to_owned(),
        "other-b/sub/notes.txt".to_owned(),
        "data/run/s2/work/a.txt".to_owned(),
        "data/agent-state/m2/notes/n.md".to_owned(),
        "data/agent-state/m1/scratch/x".to_owned(),
        "data/agents/claude-acp/home/.credentials.json".to_owned(),
        "data/agents/claude-acp/home/history.jsonl".to_owned(),
        "data/search.sqlite".to_owned(),
        "data/logs/host.log".to_owned(),
        project("-other-matter/s-2.jsonl"),
        project("-other-matter/s-2/tool-results/webfetch-2.pdf"),
        project(&format!("{own_key}/s-1.jsonl")),
        project(&format!("{own_key}/s-1/subagents/agent-a.jsonl")),
        project(&format!("{own_key}/memory/MEMORY.md")),
        // Beside the session's matter inside `root`, a registered matter: fenced.
        "outside/x.txt".to_owned(),
    ];
    let open = [
        "data/run/s1/work/draft.docx".to_owned(),
        "data/run/s1/skill-packs/p/SKILL.md".to_owned(),
        "data/agent-state/m1/notes/n.md".to_owned(),
        "data/agents/claude-acp/home/plans/plan.md".to_owned(),
        "data/profile/claude-plugin/skills/s/SKILL.md".to_owned(),
        project(&format!("{own_key}/s-1/tool-results/webfetch-1.pdf")),
        project(&format!("{own_key}/s-1/tool-results/toolu_1.txt")),
        "matter/sub/a.txt".to_owned(),
    ];
    for file in deny.iter().chain(&open) {
        let file = root.join(file);
        write::create_dir_all(file.parent().unwrap()).unwrap();
        write::write_file(&file, b"x").unwrap();
    }
    let rules = whole_launch(root);
    for base in spellings(root) {
        for file in deny
            .iter()
            .map(String::as_str)
            .chain(["other-a", "other-b"])
        {
            for tool in ["Read", "Edit"] {
                let path = base.join(file);
                assert!(denied(&rules, tool, &path), "{tool} {path:?}: {rules:#?}");
            }
        }
        for file in open
            .iter()
            .map(String::as_str)
            .chain(["matter", "data/run/s1/work"])
        {
            let path = base.join(file);
            assert!(!denied(&rules, "Read", &path), "{path:?}: {rules:#?}");
        }
        assert!(!denied(&rules, "Read", &base), "{base:?}: {rules:#?}");
    }
}

/// An other matter's folder is named by its canonical path and as stored; one that is the
/// session's matter folder or `<data>`, or is not absolute, is in no rule.
#[test]
fn other_matters_are_spelled_both_ways_and_never_cover_the_sessions_own() {
    let dir = layout("matters-spelled");
    let root = dir.path();
    let matter = root.join("matter");
    let data = root.join("data");
    let keep = [matter.as_path(), data.as_path()];
    let other = root.join("outside");
    let found = matters(&[(other.clone(), false)], &keep, &[]);
    assert_eq!(found.others[0].paths, spellings(&other));
    for left_out in [
        matter.clone(),
        root.join("data"),
        PathBuf::from("relative/matter"),
    ] {
        let found = matters(&[(left_out.clone(), false)], &keep, &[]);
        assert!(
            found.denied_edits().next().is_none(),
            "{left_out:?}: {found:?}"
        );
    }
}

/// A link registered as a matter's folder is denied as itself and where it leads.
#[cfg(unix)]
#[test]
fn a_linked_matter_folder_is_denied_at_both_places() {
    let dir = layout("matters-link");
    let root = dir.path();
    let real = root.join("outside");
    let link = root.join("linked-matter");
    write::symlink(&real, &link).unwrap();
    let found = matters(
        &[(link.clone(), false)],
        &[root.join("matter").as_path()],
        &[],
    );
    assert_eq!(
        found.others[0].paths,
        vec![dunce::canonicalize(&real).unwrap(), link.clone()]
    );
    let deny = LaunchDeny {
        matters: found,
        ..LaunchDeny::default()
    };
    let rules = claude_deny_rules(&deny);
    for path in [link.join("a.pdf"), real.join("a.pdf"), link, real] {
        assert!(denied(&rules, "Read", &path), "{path:?}: {rules:#?}");
    }
}

/// The rules for another matter's folder are `Read` and `Edit` of the folder and all under it,
/// in Claude Code's `//` form, glob characters escaped.
#[test]
fn an_other_matters_rules_name_the_folder_and_all_under_it() {
    let mut other = Other::new(
        id(1),
        "Smith".into(),
        PathBuf::from("/m/Smith [2024]"),
        false,
    );
    other.spellings = vec![other.folder.clone()];
    other.paths = vec![other.folder.clone()];
    let mut deny = LaunchDeny {
        matters: MatterFence::asked(vec![other]),
        ..LaunchDeny::default()
    };
    let rules = claude_deny_rules(&deny);
    assert_eq!(
        rules,
        [
            r"Read(//m/Smith \[2024\])",
            r"Read(//m/Smith \[2024\]/**)",
            r"Edit(//m/Smith \[2024\])",
            r"Edit(//m/Smith \[2024\]/**)",
        ]
    );
    // Invited: its Read deny goes, a Read allow comes, its Edit deny stays.
    deny.matters.others[0].invited = true;
    assert_eq!(
        claude_deny_rules(&deny),
        [r"Edit(//m/Smith \[2024\])", r"Edit(//m/Smith \[2024\]/**)"]
    );
    let allow = claude_allow_rules(&deny);
    assert!(
        allow.contains(&r"Read(//m/Smith \[2024\])".to_owned()),
        "{allow:?}"
    );
    assert!(
        allow.contains(&r"Read(//m/Smith \[2024\]/**)".to_owned()),
        "{allow:?}"
    );
}

/// Stage D's own allows open the session's matter (Claude Code's working folder, and a
/// `Read` allow over it) and its own work and notes folders; a matter inside the session's
/// matter is still denied for Read and Edit, a deny beating every allow in Claude Code
/// (`own_allow.rs`), while the rest of the matter stays open.
#[test]
fn a_child_matter_stays_denied_inside_the_matters_own_allow() {
    let dir = layout("matters-child-allow");
    let root = dir.path();
    let matter = dunce::canonicalize(root.join("matter")).unwrap();
    crate::write::create_dir_all(&matter.join("child")).unwrap();
    crate::write::write_file(&matter.join("child/deed.pdf"), b"x").unwrap();
    let data = root.join("data");
    let fence = matters(
        &[(matter.join("child"), false)],
        &[matter.as_path()],
        &[data.as_path()],
    );
    let deny = LaunchDeny {
        spellings: vec![data],
        own_reads: vec!["run/s1/work".to_owned(), "agent-state/m1/notes".to_owned()],
        matters: fence,
        ..LaunchDeny::default()
    };
    let rules = claude_deny_rules(&deny);
    let mut allow = claude_allow_rules(&deny);
    let base = rule_path(matter.to_str().unwrap(), cfg!(windows));
    allow.push(format!("Read({base}/**)"));
    // Claude Code: a matching deny ends it; an allow counts only when no deny matches.
    let decide =
        |tool: &str, path: &Path| !denied(&rules, tool, path) && denied(&allow, tool, path);
    for tool in ["Read", "Edit"] {
        assert!(
            denied(&rules, tool, &matter.join("child/deed.pdf")),
            "{tool}: {rules:#?}"
        );
        assert!(denied(&rules, tool, &matter.join("child")), "{tool}");
    }
    assert!(!decide("Read", &matter.join("child/deed.pdf")));
    assert!(decide("Read", &matter.join("sub/a.txt")), "{rules:#?}");
}

/// The shared answer to "does this matter contain a denied folder?" (`LaunchDeny::denies_inside`):
/// a fenced child matter, the data folder, or an unlisted matter inside: yes; an invited child,
/// or nothing inside: no. Compared in the drive's other case too.
#[test]
fn denies_inside_answers_for_a_child_the_data_folder_and_nothing() {
    let dir = layout("matters-denies-inside");
    let root = dir.path();
    let matter = dunce::canonicalize(root.join("matter")).unwrap();
    crate::write::create_dir_all(&matter.join("child")).unwrap();
    let own = [matter.as_path()];
    let with = |others: &[(PathBuf, bool)], data: PathBuf| LaunchDeny {
        spellings: vec![data],
        matters: matters(others, &own, &[]),
        ..LaunchDeny::default()
    };
    let outside_data = root.join("data");
    assert!(with(&[(matter.join("child"), false)], outside_data.clone()).denies_inside(&own));
    assert!(!with(&[(matter.join("child"), true)], outside_data.clone()).denies_inside(&own));
    assert!(with(&[], matter.join("data")).denies_inside(&own));
    assert!(!with(&[(root.join("outside"), false)], outside_data.clone()).denies_inside(&own));
    let mut unlisted = with(&[], outside_data);
    unlisted.matters.own_unlisted = true;
    assert!(unlisted.denies_inside(&own));
}

/// Drive letters and `\\?\` forms compare as one (Windows).
#[cfg(windows)]
#[test]
fn denies_inside_folds_the_drive_and_verbatim_forms() {
    let deny = LaunchDeny {
        spellings: vec![PathBuf::from(r"\\?\c:\clients\acme\data")],
        ..LaunchDeny::default()
    };
    assert!(deny.denies_inside(&[Path::new(r"C:\Clients\Acme")]));
}

/// A fenced matter inside this one counts even when the fence dropped its paths (past the
/// launch's budget, or under a folder it could not list): `denies_inside` reads the matters'
/// folders, not the paths kept.
#[test]
fn denies_inside_counts_a_matter_whose_paths_were_dropped() {
    let dir = layout("matters-denies-dropped");
    let root = dir.path();
    let matter = dunce::canonicalize(root.join("matter")).unwrap();
    crate::write::create_dir_all(&matter.join("child")).unwrap();
    let own = [matter.as_path()];
    let mut deny = LaunchDeny {
        spellings: vec![root.join("data")],
        matters: matters(&[(matter.join("child"), false)], &own, &[]),
        ..LaunchDeny::default()
    };
    // Dropped by the budget or unlisted: no paths, named unfenced.
    let child = deny.matters.others[0].id;
    deny.matters.others[0].paths.clear();
    deny.matters.unfenced.push(child);
    assert!(deny.denies_inside(&own));
    assert!(!deny.denies_inside(&[]), "no spelling, no answer");
}
