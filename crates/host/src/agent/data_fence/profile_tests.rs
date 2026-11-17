//! Clauseo's built-in plugin folder (`<data>/profile`, [`SHARED`]) is every session's to read,
//! never to write: Claude's rules refuse an edit there and let a read through, in each spelling
//! of the data folder and on every system, and the host's auto-deny counts it as own only for a
//! tool that only reads. The host's own launch-time writes there still land.

use std::path::PathBuf;

use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths};
use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;
use serde_json::json;

use super::super::{
    LaunchDeny, SHARED, claude_allow_rules_on, claude_deny_rules, claude_deny_rules_on,
    launch_deny, reads_only, rule_path,
};
use super::{check, check_read, layout, text};
use crate::write;

/// The plugin's file a built-in skill sends the model to read, under `<data>`.
const CHECKLISTS: &str =
    "profile/claude-plugin/skills/legal-drafting/references/section-checklists.md";

/// What an earlier version left in that file, which the next launch's write replaces.
const EARLIER: &[u8] = b"left by an earlier version";

/// Whether one of `rules` for `tool` names `path` (in rule form): the rule itself, or a folder
/// rule (`…/**`) above it.
fn names(rules: &[String], tool: &str, path: &str) -> bool {
    rules.iter().any(|rule| {
        let Some(inner) = rule
            .strip_prefix(tool)
            .and_then(|rest| rest.strip_prefix('('))
            .and_then(|rest| rest.strip_suffix(')'))
        else {
            return false;
        };
        match inner.strip_suffix("/**") {
            Some(folder) => path.starts_with(&format!("{folder}/")),
            None => inner == path,
        }
    })
}

/// For each system's form of the paths (`windows`: Windows; else macOS and Linux, with the shell
/// sandbox or without), and for each spelling of the data folder (its canonical path and the
/// path as configured), Claude's rules refuse an `Edit` (`Edit`, `Write`, `MultiEdit`,
/// `NotebookEdit`) of the plugin's files and let a `Read` of its skills through, allowed.
#[test]
fn claude_edits_of_the_built_in_plugin_are_refused_and_reads_pass_on_every_system() {
    assert!(SHARED.iter().any(|(place, _)| *place == "profile"));
    let systems = [
        (
            false,
            false,
            // unix-paths-ok: rule text only (`rule_path`), never opened or resolved
            ["/home/a/.local/share/clauseo", "/srv/link/clauseo"],
        ),
        (
            false,
            true,
            // unix-paths-ok: rule text only (`rule_path`), never opened or resolved
            ["/Users/a/Library/Clauseo", "/Volumes/Work/Clauseo"],
        ),
        (
            true,
            false,
            [r"C:\Users\a\AppData\Roaming\Clauseo", r"D:\Clauseo"],
        ),
    ];
    for (windows, sandboxed, spellings) in systems {
        let deny = LaunchDeny::fixed(spellings.iter().map(PathBuf::from).collect());
        let refused = claude_deny_rules_on(&deny, windows, sandboxed);
        let allowed = claude_allow_rules_on(&deny, windows);
        for spelling in spellings {
            let base = rule_path(spelling, windows);
            assert!(
                refused.contains(&format!("Edit({base}/profile/**)")),
                "{refused:?}"
            );
            for place in [
                CHECKLISTS,
                "profile/claude-plugin/.claude-plugin/plugin.json",
            ] {
                let file = format!("{base}/{place}");
                assert!(names(&refused, "Edit", &file), "{file}: {refused:?}");
                assert!(!names(&refused, "Read", &file), "{file}: {refused:?}");
            }
            let file = format!("{base}/{CHECKLISTS}");
            assert!(names(&allowed, "Read", &file), "{file}: {allowed:?}");
        }
    }
}

/// Only Claude Code's tools that only read count as reads; a command, an edit, a Codex patch
/// and a tool with no name may write.
#[test]
fn only_the_reading_tools_count_as_reads() {
    for tool in ["Read", "Glob", "Grep", "LS", "read", "grep"] {
        assert!(reads_only(Some(tool)), "{tool}");
    }
    for tool in [
        "Edit",
        "Write",
        "MultiEdit",
        "NotebookEdit",
        "Bash",
        "PowerShell",
        "apply_patch",
        "",
    ] {
        assert!(!reads_only(Some(tool)), "{tool}");
    }
    assert!(!reads_only(None));
}

/// The auto-deny lets a read of the plugin through and refuses every other request that names
/// it: an edit, a command, a Codex patch approval (no tool name). A read gets nothing more
/// than the plugin: Clauseo's own files stay refused to it.
#[test]
fn the_auto_deny_refuses_a_write_into_the_built_in_plugin_and_passes_a_read() {
    let dir = layout("plugin-write");
    let root = dir.path();
    let data = root.join("data");
    let file = data.join(CHECKLISTS);
    write::create_dir_all(file.parent().unwrap()).unwrap();
    write::write_file(&file, b"x").unwrap();
    let path = text(&file);
    let glob = json!({"path": text(&data.join("profile")), "pattern": "**/*.md"});
    assert!(!check_read(root, &[&path], None).touches_data());
    assert!(!check_read(root, &[], Some(&glob)).touches_data());
    assert!(check(root, &[], Some(&glob)).touches_data());
    let edit = json!({"file_path": path, "old_string": "x", "new_string": "y"});
    let command = json!({"command": format!("cat {path}")});
    let shell_write = json!({"command": format!("printf y > {path}")});
    assert!(check(root, &[&path], None).touches_data());
    assert!(check(root, &[], Some(&edit)).touches_data());
    assert!(check(root, &[], Some(&command)).touches_data());
    assert!(check(root, &[], Some(&shell_write)).touches_data());
    let new_file = text(&data.join("profile").join("claude-plugin").join("new.md"));
    assert!(check(root, &[&new_file], None).touches_data());
    write::create_dir_all(&data.join("logs")).unwrap();
    let logs = text(&data.join("logs").join("host.log"));
    assert!(check_read(root, &[&logs], None).touches_data());
}

/// The host writes the built-in plugin at each launch (`launch_checks`' `write_skills`, both
/// agents' first write) and still does, into a data folder whose launch rules refuse Claude's
/// edits there: the rules bind the agent's tools, never the host. A second launch's write
/// over an earlier one lands too.
#[test]
fn the_hosts_launch_writes_of_the_built_in_plugin_still_land() {
    let dir = layout("plugin-host-write");
    let root = dir.path();
    let locations = Locations::under_home(root);
    let data = locations.data_dir().to_path_buf();
    let paths = ProfilePaths::new(
        &locations,
        root.join("app").join("clauseo"),
        root.join("stetdoc").join("stetdoc"),
        BrowserSidecars {
            clauseo_mcp: root.join("app").join("clauseo-mcp"),
            agent_browser: root.join("app").join("agent-browser"),
        },
    )
    .unwrap();
    let (profile, errors) = ClauseoProfile::new(paths, None).with_builtin_skills(None);
    assert!(errors.is_empty(), "{errors:?}");
    write::create_dir_all(&data).unwrap();
    let rules = claude_deny_rules(&launch_deny(vec![data.clone()], &data, &[], &[]), false);
    let base = rule_path(&text(&data), cfg!(windows));
    assert!(
        rules.contains(&format!("Edit({base}/profile/**)")),
        "{rules:?}"
    );
    let file = data.join(CHECKLISTS);
    let mut shipped = None;
    for kind in [AgentKind::Claude, AgentKind::Codex, AgentKind::Claude] {
        let writes = profile.launch_writes(Some(kind));
        let first = writes.first().expect("the plugin's write");
        assert!(first.root().starts_with(data.join("profile")));
        let written = clauseo_profile::write_skills(first).unwrap();
        assert_eq!(written.root(), first.root());
        let bytes = std::fs::read(&file).unwrap();
        assert_ne!(bytes.as_slice(), EARLIER);
        assert_eq!(shipped.get_or_insert_with(|| bytes.clone()), &bytes);
        write::write_file(&file, EARLIER).unwrap();
    }
}
