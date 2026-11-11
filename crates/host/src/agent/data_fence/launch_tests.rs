//! Claude's deny rules enumerated per launch (`launch.rs`) and the host's auto-deny agree: a
//! launch for matter `m1` in a data folder that holds another matter's notes, other run folders
//! and the search index denies each, and leaves the session's own run folder, notes and plans
//! alone. And every data folder entry `Locations` names is denied or allowed for a reason.

use serde_json::json;

use super::launch::claude_project_key;
use super::tests::{check, layout, text};
use super::{
    DENIED, LaunchDeny, READ_ONLY, SHARED, claude_allow_rules, claude_deny_rules, claude_projects,
    launch_deny,
};
use crate::write;

/// What a data folder may hold beside the layout's own: other sessions' run folders (one set
/// aside), another matter's notes, this matter's other state, the search index and store, the
/// built-in plugin, the logs, a file Claude keeps in its home and one nobody named.
const OTHERS: [&str; 14] = [
    "run/s2/work/a.txt",
    "run.old-1700000000/s9/work/b.txt",
    "agent-state/m2/notes/n.md",
    "agent-state/m1/scratch/x",
    "search.sqlite",
    "search.sqlite-wal",
    "search.sqlite-shm",
    "state.sqlite",
    "state.sqlite-wal",
    "logs/host.log",
    "agents/claude-acp/home/history.jsonl",
    "agents/claude-acp/home/statsig/x",
    "agents/codex-acp/home/auth.json",
    "made-later[1].txt",
];

/// The patterns a launch of session `s1` for matter `m1` gets under `root/data`.
fn launch(root: &std::path::Path) -> LaunchDeny {
    let data = root.join("data");
    for other in OTHERS {
        let path = data.join(other);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, b"x").unwrap();
    }
    let own = [
        data.join("run").join("s1"),
        data.join("agent-state").join("m1").join("notes"),
        data.join("agents")
            .join("claude-acp")
            .join("home")
            .join("plans"),
        root.join("matter"),
    ];
    let home = data.join("agents/claude-acp/home");
    let matter = dunce::canonicalize(root.join("matter")).unwrap();
    let projects = claude_projects(&home, &[&matter]);
    launch_deny(vec![data.clone()], &data, &own, &projects)
}

/// Each other matter's notes, other run folder, `run.old-*` entry and search index file that
/// exists at the launch is denied, in the rules Claude gets; the session's own are not.
#[test]
fn a_launch_denies_what_exists_and_is_not_the_sessions_own() {
    let dir = layout("launch");
    let root = dir.path();
    let deny = launch(root);
    for target in [
        "run/s2/**",
        "run.old-1700000000/**",
        "agent-state/m2/**",
        "agent-state/m1/scratch/**",
        "search.sqlite",
        "search.sqlite-wal",
        "search.sqlite-shm",
        "agents/claude-acp/home/statsig/**",
        r"made-later\[1\].txt",
    ] {
        assert!(
            deny.targets.iter().any(|t| t == target),
            "{target}: {deny:?}"
        );
    }
    for own in ["run/s1", "agent-state/m1/notes", "home/plans"] {
        assert!(
            !deny.targets.iter().any(|t| t.contains(own)),
            "{own}: {deny:?}"
        );
    }
    let data = text(&root.join("data"));
    let rules = claude_deny_rules(&deny);
    for target in ["agent-state/m2/**", "run/s2/**", "search.sqlite-wal"] {
        let base = super::rule_path(&data, cfg!(windows));
        for tool in ["Read", "Edit"] {
            let rule = format!("{tool}({base}/{target})");
            assert!(rules.contains(&rule), "{rule}");
        }
    }
}

/// The host's auto-deny rejects a Read and a Bash `cat` of each of those, and of anything a
/// launch rule names, and leaves the session's own run folder, notes and plans alone.
#[test]
fn the_auto_deny_agrees_with_the_launch_rules() {
    let dir = layout("agrees");
    let root = dir.path();
    launch(root);
    let data = root.join("data");
    for other in OTHERS {
        let path = text(&data.join(other));
        assert!(check(root, &[&path], None).touches_data(), "Read {path}");
        let cat = json!({"command": format!("cat '{path}'")});
        assert!(check(root, &[], Some(&cat)).touches_data(), "cat {path}");
    }
    for own in [
        "run/s1/work/out.docx",
        "agent-state/m1/notes/n.md",
        "agents/claude-acp/home/plans/plan.md",
    ] {
        let path = text(&data.join(own));
        assert!(!check(root, &[&path], None).touches_data(), "Read {path}");
        let cat = json!({"command": format!("cat '{path}'")});
        assert!(!check(root, &[], Some(&cat)).touches_data(), "cat {path}");
    }
}

/// A link in the data folder is denied as itself and as a folder.
#[cfg(unix)]
#[test]
fn a_link_in_the_data_folder_is_denied_both_ways() {
    let dir = layout("launch-link");
    let data = dir.path().join("data");
    write::symlink(&dir.path().join("outside"), &data.join("elsewhere")).unwrap();
    let deny = launch_deny(vec![data.clone()], &data, &[], &[]);
    for target in ["elsewhere", "elsewhere/**"] {
        assert!(deny.targets.iter().any(|t| t == target), "{target}");
    }
}

/// Each data folder entry `Locations` names, and the store's and the search index's files, is
/// in [`DENIED`] or allowed below for a stated reason. A new or renamed entry fails this until
/// the fence is told about it.
#[test]
fn every_data_folder_entry_locations_names_is_denied_or_allowed_for_a_reason() {
    // Allowed whole, with the reason. Every entry here is still walked into per launch.
    let mut allowed = vec![(
        "agents",
        "holds Claude's plans folder, which Claude Code writes its plan note into; its other \
         entries are named in DENIED or enumerated per launch",
    )];
    allowed.extend(SHARED);
    assert!(SHARED.iter().any(|(place, _)| *place == "profile"));
    let files = [clauseo_store::DB_FILE, clauseo_search::FILE_NAME];
    let entries = clauseo_settings::DATA_ENTRIES.iter().chain(&files);
    for entry in entries {
        let denied = [
            entry.to_string(),
            format!("{entry}/**"),
            format!("{entry}*"),
        ];
        let denied = denied.iter().any(|form| DENIED.contains(&form.as_str()));
        let reason = allowed.iter().find(|(name, _)| name == entry);
        assert!(
            denied || reason.is_some_and(|(_, why)| !why.is_empty()),
            "{entry} is neither denied nor allowed for a reason"
        );
    }
}

/// The built-in plugin is the same for every matter and its skills send the model to Read their
/// files: no launch rule covers one, and the auto-deny lets a Read of it through.
#[test]
fn the_built_in_plugins_files_stay_readable() {
    let dir = layout("plugin");
    let root = dir.path();
    let data = root.join("data");
    let place = "profile/claude-plugin/skills/legal-drafting/section-checklists.md";
    let file = data.join(place);
    write::create_dir_all(file.parent().unwrap()).unwrap();
    write::write_file(&file, b"x").unwrap();
    let deny = launch(root);
    let covers = |target: &str| match target.strip_suffix("/**") {
        Some(folder) => place.starts_with(&format!("{folder}/")),
        None => target == place,
    };
    let covering: Vec<&String> = deny.targets.iter().filter(|t| covers(t)).collect();
    assert!(covering.is_empty(), "{covering:?}");
    let read = text(&file);
    assert!(!check(root, &[&read], None).touches_data());
}

/// Claude Code's project key for a working folder, as its `kI` computes it: every UTF-16 unit
/// that is not an ASCII letter or digit made `-`, and past 200 the first 200 and a hash. The
/// expected keys were computed by Claude Code 0.3.293's own function, run under Node.
#[test]
fn claudes_project_key_is_computed_as_claude_code_does() {
    assert_eq!(
        claude_project_key("/Users/a/My Matter_1.x"),
        "-Users-a-My-Matter-1-x"
    );
    assert_eq!(
        claude_project_key(r"C:\Users\A B\Matters\Acme"),
        "C--Users-A-B-Matters-Acme"
    );
    assert_eq!(claude_project_key("/m/\u{1F600}"), "-m---");
    let long = format!(
        "/Users/a/Matters/{}\u{e9}\u{1F600}",
        "Acme v Widget Holdings \u{2013} Disclosure ".repeat(7)
    );
    let want = "-Users-a-Matters-Acme-v-Widget-Holdings---Disclosure-Acme-v-Widget-Holdings---\
                Disclosure-Acme-v-Widget-Holdings---Disclosure-Acme-v-Widget-Holdings---Disclosure-\
                Acme-v-Widget-Holdings---Disclosure-Acm-93gu6b";
    assert_eq!(claude_project_key(&long), want);
}

/// The large tool results Claude Code saves in this session's project folder and has the model
/// Read back stay readable: no launch rule covers `<project>/<session>/tool-results`. Every
/// transcript (this project's too), subagent transcript and memory file is denied, by a fixed
/// pattern as well as by name, and so is every other project's folder, whole.
#[test]
fn the_sessions_own_tool_results_stay_readable_and_the_rest_of_projects_does_not() {
    let dir = layout("projects");
    let root = dir.path();
    let projects = root.join("data/agents/claude-acp/home/projects");
    let matter = dunce::canonicalize(root.join("matter")).unwrap();
    let own = claude_project_key(matter.to_str().unwrap());
    let place = |rest: &str| format!("agents/claude-acp/home/projects/{rest}");
    let readable = place(&format!("{own}/s-1/tool-results/toolu_1.txt"));
    let denied = [
        place(&format!("{own}/s-1.jsonl")),
        place(&format!("{own}/s-1/subagents/agent-a.jsonl")),
        place(&format!("{own}/s-1/other.bin")),
        place(&format!("{own}/memory/MEMORY.md")),
        place(&format!("{own}/notes.txt")),
        place("-other-matter/s-2.jsonl"),
        place("-other-matter/s-2/tool-results/toolu_2.txt"),
    ];
    for file in denied.iter().chain([&readable]) {
        let file = root.join("data").join(file);
        write::create_dir_all(file.parent().unwrap()).unwrap();
        write::write_file(&file, b"x").unwrap();
    }
    assert!(projects.join(&own).is_dir());
    let deny = launch(root);
    let covering: Vec<&String> = deny
        .targets
        .iter()
        .filter(|t| covers(t, &readable))
        .collect();
    assert!(covering.is_empty(), "{readable} is covered by {covering:?}");
    for file in &denied {
        assert!(
            deny.targets.iter().any(|t| covers(t, file)),
            "{file}: {deny:?}"
        );
    }
    assert!(
        deny.targets.iter().any(|t| t == &place("-other-matter/**")),
        "{deny:?}"
    );
    let later = [
        place("-made-later/s-3.jsonl"),
        place("-made-later/s-3/subagents/agent-b.jsonl"),
        place("-made-later/memory/MEMORY.md"),
    ];
    for file in later {
        assert!(DENIED.iter().any(|t| covers(t, &file)), "{file}");
    }
}

/// Whether the rule pattern `target` (`/`-separated, gitignore's `*` within a name and a
/// trailing `**`, glob characters escaped by `\\`) covers the place `file` under `<data>`.
fn covers(target: &str, file: &str) -> bool {
    let unescape = |text: &str| text.replace('\\', "");
    let parts: Vec<&str> = target.split('/').collect();
    let names: Vec<&str> = file.split('/').collect();
    let (parts, open) = match parts.split_last() {
        Some((&"**", head)) => (head, true),
        _ => (parts.as_slice(), false),
    };
    let fits = |part: &str, name: &str| match part.split_once('*') {
        Some((head, tail)) if !part.contains("\\*") => {
            name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail)
        }
        _ => unescape(part) == name,
    };
    let long_enough = if open {
        names.len() > parts.len()
    } else {
        names.len() == parts.len()
    };
    long_enough
        && parts
            .iter()
            .zip(&names)
            .all(|(part, name)| fits(part, name))
}

const PYTHON: &str = "clauseo-python-3.13.18-c2-macos-arm64";
const NODE: &str = "node-v24.22.0-darwin-arm64";

/// Clauseo's Python runtime and `<data>/tools` are in no `Read` deny and are allowed `Read`,
/// and are `Edit`-denied, all under them; Node, a half-unpacked folder, a link named like a
/// Python build, the sign-in shim and the rest of `agents` stay denied for both.
#[test]
fn python_and_tools_are_readable_never_editable_and_their_siblings_stay_denied() {
    let dir = layout("read-only");
    let root = dir.path();
    let data = root.join("data");
    let runtime = data.join("agents").join("runtime");
    for file in [
        runtime.join(PYTHON).join("bin").join("python3"),
        runtime.join(PYTHON).join("lib").join("os.py"),
        runtime.join(NODE).join("bin").join("node"),
        runtime
            .join(format!(".tmp-{NODE}"))
            .join("bin")
            .join("node"),
        data.join("agents").join("signin").join("shim"),
        data.join("agents").join("npm-cache").join("x"),
        data.join("tools").join("agent-browser"),
    ] {
        write::create_dir_all(file.parent().unwrap()).unwrap();
        write::write_file(&file, b"x").unwrap();
    }
    #[cfg(unix)]
    write::symlink(
        &data.join("agents").join("claude-acp").join("home"),
        &runtime.join("clauseo-python-link"),
    )
    .unwrap();
    let deny = launch(root);
    let mut denied = vec![
        format!("agents/runtime/{NODE}/**"),
        format!("agents/runtime/.tmp-{NODE}/**"),
        "agents/signin/**".to_owned(),
        "agents/npm-cache/**".to_owned(),
    ];
    if cfg!(unix) {
        denied.push("agents/runtime/clauseo-python-link".to_owned());
        denied.push("agents/runtime/clauseo-python-link/**".to_owned());
    }
    for target in &denied {
        assert!(deny.targets.contains(target), "{target}: {deny:?}");
    }
    for open in [PYTHON, "tools"] {
        assert!(
            !deny.targets.iter().any(|t| t.contains(open)),
            "{open}: {deny:?}"
        );
    }
    for whole in ["agents/runtime/**", "agents/**"] {
        assert!(
            !deny.targets.iter().any(|t| t == whole),
            "{whole}: {deny:?}"
        );
    }
    let base = super::rule_path(&text(&data), cfg!(windows));
    let rules = claude_deny_rules(&deny);
    let allow = claude_allow_rules(&deny);
    for place in ["agents/runtime/clauseo-python-*", "tools"] {
        assert!(READ_ONLY.contains(&place));
        let edit = format!("Edit({base}/{place}/**)");
        assert!(rules.contains(&edit), "{edit}");
        let read = format!("Read({base}/{place}/**)");
        assert!(allow.contains(&read), "{read}");
        assert!(!rules.contains(&read), "{read}");
    }
    assert!(
        !rules
            .iter()
            .any(|r| r.starts_with("Read(") && r.contains(PYTHON))
    );
    assert!(
        !rules
            .iter()
            .any(|r| r.starts_with("Read(") && r.contains("/tools"))
    );
    // Nothing else is allowed: the built-in skills and these two.
    assert_eq!(allow.len(), 1 + READ_ONLY.len());
    for target in &denied {
        assert!(
            rules.contains(&format!("Read({base}/{target})")),
            "{target}"
        );
        assert!(
            rules.contains(&format!("Edit({base}/{target})")),
            "{target}"
        );
    }
}

/// The fixed rules (before the launch lists anything): Node and half-unpacked folders under
/// `agents/runtime`, not the folder whole; `<data>/browser` still; and in every run folder,
/// the own one too, the shell browser's config file, which agent-browser reads itself.
#[test]
fn the_fixed_rules_deny_node_browser_and_every_shell_browser_config() {
    for target in [
        "agents/runtime/node-v*/**",
        "agents/runtime/.tmp-*/**",
        "browser/**",
        "run/*/agent-browser/config.json",
        "agents/signin/**",
    ] {
        assert!(DENIED.contains(&target), "{target}");
    }
    assert!(!DENIED.contains(&"agents/runtime/**"));
    let rules = claude_deny_rules(&LaunchDeny::fixed(vec![std::path::PathBuf::from("/d")]));
    for rule in [
        "Read(//d/run/*/agent-browser/config.json)",
        "Edit(//d/run/*/agent-browser/config.json)",
        "Read(//d/browser/**)",
        "Edit(//d/agents/runtime/clauseo-python-*/**)",
        "Edit(//d/tools/**)",
    ] {
        assert!(rules.iter().any(|r| r == rule), "{rule}");
    }
}

/// The own run folder's `agent-browser` folder (its profile and downloads) is in no launch
/// rule; another session's is, with its whole run folder.
#[test]
fn the_own_agent_browser_folder_is_open_and_another_sessions_is_denied() {
    let dir = layout("agent-browser");
    let root = dir.path();
    let data = root.join("data");
    for session in ["s1", "s2"] {
        let own = data.join("run").join(session).join("agent-browser");
        for file in [own.join("config.json"), own.join("downloads").join("a.pdf")] {
            write::create_dir_all(file.parent().unwrap()).unwrap();
            write::write_file(&file, b"x").unwrap();
        }
        write::create_dir_all(&own.join("profile")).unwrap();
    }
    let deny = launch(root);
    assert!(deny.targets.iter().any(|t| t == "run/s2/**"), "{deny:?}");
    assert!(
        !deny.targets.iter().any(|t| t.contains("run/s1")),
        "{deny:?}"
    );
    let own = data.join("run").join("s1").join("agent-browser");
    for path in [own.join("downloads").join("a.pdf"), own.join("profile")] {
        let path = text(&path);
        assert!(!check(root, &[&path], None).touches_data(), "{path}");
    }
    let theirs = text(&data.join("run/s2/agent-browser/downloads/a.pdf"));
    assert!(check(root, &[&theirs], None).touches_data(), "{theirs}");
}

/// The trust roots file (`<data>/tls/roots.pem`, `loads::tls_roots`) is denied to Claude's
/// Read and Edit, so its Write and Edit tools can't put a root of their own in it, which `curl`
/// and Python in later sessions would trust; the folder whole, so a file beside it can't stand in.
#[test]
fn the_trust_roots_are_denied_to_claudes_file_tools() {
    assert!(DENIED.contains(&"tls/**"));
    let [folder, file] = crate::agent::loads::tls_roots::ROOTS_FILE;
    assert_eq!(folder, "tls");
    assert_eq!(file, "roots.pem");
    let rules = claude_deny_rules(&LaunchDeny::fixed(vec![std::path::PathBuf::from("/d")]));
    for rule in ["Read(//d/tls/**)", "Edit(//d/tls/**)"] {
        assert!(rules.iter().any(|r| r == rule), "{rule}");
    }
}
