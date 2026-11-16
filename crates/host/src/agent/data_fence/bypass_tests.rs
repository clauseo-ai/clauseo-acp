//! The security review's bypasses of the data folder fence, each kept as a test: a dangling
//! link, an own folder swapped for a link, `ln` targets, the data folder's text inside a word,
//! `~user`, Glob's pattern, Claude's cross-matter records and Windows' local shares.

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use serde_json::json;

use super::resolve::drive_form;
#[cfg(unix)]
use super::resolve::{Disk, Found, resolve_all};
use super::tests::{check, layout, text};
use super::{LaunchDeny, claude_deny_rules, rule_path};
#[cfg(unix)]
use crate::write;

/// A Write through a link whose target is missing in the data folder is denied, as written
/// absolute or relative; so is one through a link loop.
#[cfg(unix)]
#[test]
fn a_dangling_link_into_the_data_folder_is_denied() {
    let dir = layout("dangling");
    let root = dir.path();
    let missing = root.join("data/agents/codex-acp/home/config.toml");
    write::symlink(&missing, &root.join("matter/cfg")).unwrap();
    write::symlink(Path::new("../data/nowhere/x"), &root.join("matter/rel")).unwrap();
    for path in ["matter/cfg", "matter/rel", "matter/rel/deeper.md"] {
        let path = text(&root.join(path));
        assert!(check(root, &[&path], None).touches_data(), "{path}");
    }
    let write = json!({"file_path": text(&root.join("matter/cfg")), "content": "x"});
    assert!(check(root, &[], Some(&write)).touches_data());
    write::symlink(Path::new("b"), &root.join("matter/a")).unwrap();
    write::symlink(Path::new("a"), &root.join("matter/b")).unwrap();
    let looped = text(&root.join("matter/a/x"));
    assert!(check(root, &[&looped], None).touches_data());
    let outside = root.join("outside/missing");
    write::symlink(&outside, &root.join("matter/out")).unwrap();
    assert!(!check(root, &[&text(&root.join("matter/out"))], None).touches_data());
}

/// The notes, the run folder or the plans folder swapped for a link into the data folder (or
/// anywhere) counts for nothing: what is under it is judged by where it leads. So does a matter
/// folder that is a link into the data folder.
#[cfg(unix)]
#[test]
fn an_own_folder_swapped_for_a_link_counts_for_nothing() {
    let dir = layout("swapped");
    let root = dir.path();
    let data = root.join("data");
    let skill = "skill-packs/x/skills/brief/SKILL.md";
    for (folder, up) in [
        ("agent-state/m1/notes", "../.."),
        ("run/s1", ".."),
        ("agents/claude-acp/home/plans", "../../.."),
    ] {
        let folder = data.join(folder);
        write::remove_dir_all(&folder).unwrap();
        write::symlink(Path::new(up), &folder).unwrap();
        let through = text(&folder.join(skill));
        assert!(check(root, &[&through], None).touches_data(), "{through}");
        write::remove_file(&folder).unwrap();
        write::create_dir_all(&folder).unwrap();
    }
    write::remove_dir_all(&root.join("matter")).unwrap();
    write::symlink(&data, &root.join("matter")).unwrap();
    assert!(check(root, &[skill], None).touches_data());
}

/// An `ln` whose target, taken from the link's own folder, is in the data folder is denied: the
/// review's swap of the notes for a link, and a link made in the matter; another matter's notes
/// are not the session's. A link to a path elsewhere is not denied.
#[test]
fn an_ln_target_is_taken_from_the_links_folder() {
    let dir = layout("ln");
    let root = dir.path();
    let notes = text(&root.join("data/agent-state/m1/notes"));
    for command in [
        format!("rm -rf {notes} && ln -s ../../.. {notes}"),
        format!("rm -rf {notes}; ln -sfn ../.. {notes}"),
        "ln -s ../data/skill-packs matter-packs".to_owned(),
        format!(
            "ln -s -t {} ../../skill-packs",
            text(&root.join("data/run/s1/work"))
        ),
    ] {
        let shell = json!({ "command": command });
        assert!(check(root, &[], Some(&shell)).touches_data(), "{command}");
    }
    let other = text(&root.join("data/agent-state/m2/notes/n.md"));
    assert!(check(root, &[&other], None).touches_data());
    let fine = json!({"command": "ln -s ../outside/x.txt sub/x.txt"});
    assert!(!check(root, &[], Some(&fine)).touches_data());
}

/// The data folder's text anywhere in a word is read as a path: a script's `open('…')`,
/// `grep -f…`, a `file://` URL, its `~/…`, `$HOME/…` and `${HOME}/…` forms (single-quoted
/// too). One into the session's run folder is not denied.
#[test]
fn the_data_folders_text_inside_a_word_is_denied() {
    let dir = layout("embedded");
    let root = dir.path();
    let skill = text(&root.join("data/skill-packs/x/skills/brief/SKILL.md"));
    for command in [
        format!(r#"python3 -c "print(open('{skill}').read())""#),
        format!("grep -f{skill} x"),
        format!("curl file://{skill}"),
        r#"node -e "require('fs').readFileSync('~/data/settings.json')""#.to_owned(),
        r#"python3 -c "print(open('$HOME/data/skill-packs/x/skills/brief/SKILL.md').read())""#
            .to_owned(),
        r#"python3 -c "print(open('${HOME}/data/skill-packs/x/skills/brief/SKILL.md').read())""#
            .to_owned(),
        r#"python3 -c 'print(open("$HOME/data/settings.json").read())'"#.to_owned(),
    ] {
        let shell = json!({ "command": command });
        assert!(check(root, &[], Some(&shell)).touches_data(), "{command}");
    }
    let own = text(&root.join("data/run/s1/skill-packs/x/run.py"));
    let shell = json!({"command": format!(r#"python3 -c "exec(open('{own}').read())""#)});
    assert!(!check(root, &[], Some(&shell)).touches_data());
}

/// `~user` is never taken from the working folder: an unknown user's path is checked under
/// every known home, a known user's under that user's home.
#[test]
fn a_tilde_user_path_is_taken_from_a_home_not_the_working_folder() {
    let dir = layout("tilde-user");
    let root = dir.path();
    let unknown = json!({"command": "cat ~no-such-user-q7/data/settings.json"});
    assert!(check(root, &[], Some(&unknown)).touches_data());
    let read = "~no-such-user-q7/data/skill-packs/x/skills/brief/SKILL.md";
    assert!(check(root, &[read], None).touches_data());
    #[cfg(unix)]
    if let Ok(Some(me)) = nix::unistd::User::from_uid(nix::unistd::getuid()) {
        let cwd = root.join("matter");
        let net = crate::network_paths::NetEnv::current();
        let tilde = format!("~{}/q", me.name);
        let found = resolve_all(Path::new(&tilde), &cwd, Some(root), &Disk::real(&net));
        let home = dunce::canonicalize(&me.dir).unwrap_or(me.dir);
        assert_eq!(found, [Found::Path(home.join("q"))]);
    }
}

/// Glob's and Grep's `pattern` is read up to its first glob character, and Grep's `path`.
#[test]
fn a_glob_or_grep_pattern_into_the_data_folder_is_denied() {
    let dir = layout("pattern");
    let root = dir.path();
    let packs = text(&root.join("data/skill-packs"));
    let glob = json!({"pattern": format!("{packs}/**/SKILL.md")});
    assert!(check(root, &[], Some(&glob)).touches_data());
    let grep = json!({"pattern": "secret", "path": packs});
    assert!(check(root, &[], Some(&grep)).touches_data());
    let fine = json!({"pattern": "**/*.md", "path": text(&root.join("matter"))});
    assert!(!check(root, &[], Some(&fine)).touches_data());
}

/// What Claude's home keeps across sessions, so across matters, is denied to its tools.
#[test]
fn claudes_cross_matter_records_are_denied() {
    let rules = claude_deny_rules(&LaunchDeny::fixed(vec![PathBuf::from("/d")]), false);
    for target in [
        "history.jsonl",
        "file-history/**",
        "todos/**",
        "shell-snapshots/**",
    ] {
        for tool in ["Read", "Edit"] {
            let rule = format!("{tool}(//d/agents/claude-acp/home/{target})");
            assert!(rules.contains(&rule), "{rule}");
        }
    }
}

/// A Windows path to this machine's own share is put in its drive form, for the check and for
/// Claude's rules; another machine's share is not.
#[test]
fn a_local_share_path_is_put_in_its_drive_form() {
    for path in [
        r"\\localhost\c$\Users\a",
        r"\\127.0.0.1\C$\Users\a",
        r"\\?\UNC\localhost\c$\Users\a",
        r"\\?\unc\127.0.0.1\c$\Users\a",
        r"\\?\C:\Users\a",
        r"\\.\c:\Users\a",
        "//localhost/c$/Users/a",
    ] {
        assert_eq!(drive_form(path).as_deref(), Some(r"C:\Users\a"), "{path}");
    }
    assert_eq!(drive_form(r"\\localhost\c$").as_deref(), Some(r"C:\"));
    for other in [r"\\server\c$\x", r"\\localhost\share\x", r"C:\x", "/home/a"] {
        assert_eq!(drive_form(other), None, "{other}");
    }
    assert_eq!(rule_path(r"\\localhost\c$\Users\a", true), "//c/Users/a");
    assert_eq!(rule_path(r"\\?\C:\Users\a", true), "//c/Users/a");
}

/// A long path the walk gives in its verbatim form (`dunce` keeps `\\?\` past 260 characters)
/// keys as its plain form, so it is compared with the data folder's and the own folders' keys.
#[test]
fn a_long_verbatim_path_keys_as_its_plain_form() {
    let mut plain = String::from(r"C:\Users\a\AppData\Roaming\Clauseo\agents\");
    while plain.len() < 280 {
        plain.push_str(r"A long folder name\");
    }
    let verbatim = format!(r"\\?\{plain}");
    for insensitive in [false, true] {
        assert_eq!(
            super::key(std::path::Path::new(&verbatim), insensitive),
            super::key(std::path::Path::new(&plain), insensitive)
        );
    }
    let share = format!(r"\\?\UNC\server\c$\{}", plain.get(3..).unwrap_or_default());
    assert_ne!(
        super::key(std::path::Path::new(&share), true),
        super::key(std::path::Path::new(&plain), true)
    );
}

/// The bug's own shape on Windows: a deep path the walk gives verbatim keys under the shorter,
/// plain data folder's key, so the fence's text compare finds it.
#[cfg(windows)]
#[test]
fn a_deep_verbatim_path_keys_under_the_shorter_plain_data_folder() {
    let folder = r"C:\Users\a\AppData\Roaming\Clauseo";
    let mut file = format!(r"\\?\{folder}\agents\");
    while file.len() < 300 {
        file.push_str(r"A long folder name\");
    }
    file.push_str("state.json");
    for insensitive in [false, true] {
        let data = super::key(std::path::Path::new(folder), insensitive);
        assert!(super::key(std::path::Path::new(&file), insensitive).starts_with(&data));
        let other = super::key(
            std::path::Path::new(r"C:\Users\a\AppData\Roaming\Other"),
            insensitive,
        );
        assert!(!super::key(std::path::Path::new(&file), insensitive).starts_with(&other));
    }
}

/// Claude's home's `sessions` folder (Claude Code's per-session files) is denied to its tools:
/// Read and Edit rules on every system, the shell rules on Windows, and the check of a command or
/// a tool's path that names a file there.
#[test]
fn claudes_sessions_folder_is_denied() {
    let deny = LaunchDeny::fixed(vec![PathBuf::from("/d")]);
    let rules = claude_deny_rules(&deny, false);
    for tool in ["Read", "Edit"] {
        let rule = format!("{tool}(//d/agents/claude-acp/home/sessions/**)");
        assert!(rules.contains(&rule), "{rule}");
    }
    let shell = super::windows_shell_deny_rules(&deny, &|_| None);
    assert!(
        shell
            .iter()
            .any(|rule| rule.contains("agents/claude-acp/home/sessions")
                || rule.contains(r"agents\claude-acp\home\sessions")),
        "{shell:?}"
    );
    let dir = layout("sessions");
    let root = dir.path();
    let sessions = root.join("data/agents/claude-acp/home/sessions");
    #[cfg(unix)]
    {
        write::create_dir_all(&sessions).unwrap();
        write::write_file(&sessions.join("4242.json"), b"{}").unwrap();
    }
    for name in ["4242.json", "4242.0123abcd.key"] {
        let path = text(&sessions.join(name));
        assert!(check(root, &[&path], None).touches_data(), "{path}");
        let read = json!({"file_path": path});
        assert!(check(root, &[], Some(&read)).touches_data(), "{path}");
    }
}
