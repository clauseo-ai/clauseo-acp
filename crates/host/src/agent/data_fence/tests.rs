use std::path::{Path, PathBuf};

use serde_json::json;

use super::command::command_paths;
use super::{
    FenceCheck, LaunchDeny, candidates, candidates_roots, candidates_words, claude_deny_rules,
    rule_path,
};
use crate::agent::state_folder;
use crate::pipeline::tests::support::TempDir;
use crate::write;
use clauseo_protocol::AgentKind;

/// A temp folder for `test`: `data` with a pack source, two sessions' run folders and the
/// matter's notes, the matter folder `matter` and an unrelated folder `outside`, beside it.
/// The folder removes itself when dropped, a failing test's too.
pub(super) fn layout(test: &str) -> TempDir {
    let dir = TempDir::new(&format!("fence-{test}"));
    let root = dir.path();
    for folder in [
        "data/skill-packs/x/skills/brief",
        "data/run/s1/work",
        "data/run/s1/skill-packs",
        "data/run/s2/work",
        "data/agent-state/m1/notes",
        "data/agent-state/m1/claude-acp/plans",
        "matter/sub",
        "outside",
    ] {
        write::create_dir_all(&root.join(folder)).unwrap();
    }
    write::write_file(&root.join("data/skill-packs/x/skills/brief/SKILL.md"), b"x").unwrap();
    write::write_file(&root.join("matter/sub/a.txt"), b"a").unwrap();
    dir
}

/// The check for session `s1` of the matter at `root/matter`, for `paths` and `raw_input`.
pub(super) fn check(
    root: &Path,
    paths: &[&str],
    raw_input: Option<&serde_json::Value>,
) -> FenceCheck {
    let data = root.join("data");
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    let state = data.join("agent-state").join("m1").join("claude-acp");
    let mut allowed = vec![
        data.join("run").join("s1"),
        data.join("agent-state").join("m1").join("notes"),
    ];
    // The matter's Claude state folder (R17 PR-C), as the host's `fence_check` gives it.
    allowed.extend(state_folder::fence_own(&state, AgentKind::Claude));
    FenceCheck {
        allowed,
        denied: state_folder::fence_denied(&state, AgentKind::Claude),
        data,
        cwd: root.join("matter"),
        home: Some(root.to_owned()),
        paths: candidates(&paths, raw_input, cfg!(windows)),
        roots: candidates_roots(raw_input, cfg!(windows)),
        words: candidates_words(raw_input, cfg!(windows)),
    }
}

pub(super) fn text(path: &Path) -> String {
    path.display().to_string()
}

/// Claude's rules name each Clauseo-only subtree and file for Read and Edit, in the `//` form, the
/// search index too, both login homes whole, every matter's Codex state folder and the config
/// surfaces and transcripts in every matter's Claude state folder (R17 PR-C), and never the
/// built-in plugin, the run folders, the notes, a Claude state folder whole or the plans a session
/// needs (those of other sessions are named per launch, `launch_tests.rs`).
#[test]
fn the_deny_rules_name_the_clauseo_only_subtrees_and_not_the_run_folder() {
    let rules = claude_deny_rules(&LaunchDeny::fixed(vec![PathBuf::from("/d/ata")]));
    for rule in [
        "Read(//d/ata/skill-packs/**)",
        "Edit(//d/ata/skill-packs/**)",
        "Read(//d/ata/matters/**)",
        "Read(//d/ata/agents/codex-acp/**)",
        "Read(//d/ata/agents/claude-acp/**)",
        "Read(//d/ata/agent-state/*/codex-acp/**)",
        "Read(//d/ata/agent-state/*/claude-acp/projects/*/*.jsonl)",
        "Edit(//d/ata/agent-state/*/claude-acp/settings.json)",
        "Edit(//d/ata/agent-state/*/claude-acp/hooks/**)",
        "Read(//d/ata/settings.json)",
        "Edit(//d/ata/state.sqlite*)",
        "Read(//d/ata/search.sqlite*)",
        "Edit(//d/ata/search.sqlite*)",
    ] {
        assert!(rules.iter().any(|r| r == rule), "{rule} in {rules:?}");
    }
    for kept in [
        "/run/",
        "/agent-state/**",
        "/agent-state/*/notes",
        "/agent-state/*/claude-acp/**",
        "/plans",
        "/projects/**",
        "/profile",
        "(//d/ata/**)",
        "(//d/ata/agents/**)",
    ] {
        // The relay token files are the one thing under `run` denied (R24); the run folders
        // themselves never are.
        let named = |r: &&String| r.contains(kept) && !r.ends_with("/run/*/relay-token)");
        assert!(!rules.iter().any(|r| named(&r)), "{kept}: {rules:?}");
    }
    let both = claude_deny_rules(&LaunchDeny::fixed(vec![
        PathBuf::from("/a"),
        PathBuf::from("/b"),
    ]));
    assert_eq!(both.len(), rules.len() * 2);
}

/// On Windows a path's separators become `/` and its drive `/c`; glob characters are escaped.
#[test]
fn a_rule_path_is_the_double_slash_form_with_windows_paths_forward_slashed() {
    assert_eq!(
        rule_path(r"C:\Users\A B\AppData\Roaming\ai.clauseo.desktop", true),
        "//c/Users/A B/AppData/Roaming/ai.clauseo.desktop"
    );
    assert_eq!(
        rule_path("/Users/a/Library/Application Support/x", false),
        "//Users/a/Library/Application Support/x"
    );
    assert_eq!(rule_path("/home/a*[b]?", false), r"//home/a\*\[b\]\?");
}

/// A command's path words: quoted, after `=`, `~` and `$HOME`; plain words are not.
#[test]
fn a_commands_path_tokens_are_read_through_quotes_and_assignments() {
    let got = command_paths(
        r#"cat '/d/a b/SKILL.md' | head -n 5 && sed -n 1p "$HOME/x" --file=/e ~ .. plain; echo `ls /f`"#,
        false,
    );
    let want: Vec<PathBuf> = ["/d/a b/SKILL.md", "~/x", "/e", "~", "..", "/f"]
        .iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(got, want);
    assert_eq!(
        command_paths(r"type C:\d\SKILL.md", true),
        [PathBuf::from(r"C:\d\SKILL.md")]
    );
}

/// `rawInput`'s path keys and a Codex command array are read beside the request's paths.
#[test]
fn candidates_read_raw_inputs_path_keys_and_command_arrays() {
    let input = json!({
        "path": "/p", "cwd": "/c", "paths": ["/q", 3],
        "command": ["bash", "-lc", "cat /r"],
    });
    let got = candidates(&[PathBuf::from("/l")], Some(&input), false);
    let want: Vec<PathBuf> = ["/l", "/p", "/c", "/q", "/r"]
        .iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(got, want);
}

/// A Read of a pack's source and a Bash `cat` of it are both in the data folder.
#[test]
fn a_read_or_a_cat_of_a_packs_source_is_denied() {
    let dir = layout("read");
    let root = dir.path();
    let skill = root.join("data/skill-packs/x/skills/brief/SKILL.md");
    assert!(check(root, &[&text(&skill)], None).touches_data());
    let cat = json!({"command": format!("cat '{}'", text(&skill))});
    assert!(check(root, &[], Some(&cat)).touches_data());
    let missing = root.join("data/skill-packs/y/not-made.md");
    assert!(check(root, &[&text(&missing)], None).touches_data());
}

/// `..` resolving into the data folder, from the matter or from the session's run folder.
#[test]
fn a_path_whose_dotdot_resolves_into_the_data_folder_is_denied() {
    let dir = layout("dotdot");
    let root = dir.path();
    let from_matter = root.join("matter/../data/skill-packs/x/skills/brief/SKILL.md");
    assert!(check(root, &[&text(&from_matter)], None).touches_data());
    let from_run = root.join("data/run/s1/../../skill-packs/x");
    assert!(check(root, &[&text(&from_run)], None).touches_data());
    let relative = json!({"command": "head ../data/skill-packs/x/skills/brief/SKILL.md"});
    assert!(check(root, &[], Some(&relative)).touches_data());
    let tilde = json!({"command": "cat ~/data/settings.json"});
    assert!(check(root, &[], Some(&tilde)).touches_data());
}

/// A link in the matter folder pointing into the data folder is followed.
#[test]
fn a_link_in_the_matter_folder_into_the_data_folder_is_denied() {
    let dir = layout("link");
    let root = dir.path();
    let link = root.join("matter/packs");
    let made = write::symlink(&root.join("data/skill-packs"), &link);
    // Windows without the right to make links (no Developer Mode, no admin): skipped, loudly;
    // the junction test below needs no such right.
    if cfg!(windows)
        && let Err(error) = &made
    {
        eprintln!("SKIPPED a_link_in_the_matter_folder_into_the_data_folder_is_denied: {error}");
        return;
    }
    made.unwrap();
    let through = link.join("x/skills/brief/SKILL.md");
    assert!(check(root, &[&text(&through)], None).touches_data());
    let relative = json!({"command": "cat packs/x/skills/brief/SKILL.md"});
    assert!(check(root, &[], Some(&relative)).touches_data());
}

/// A directory junction in the matter folder pointing into the data folder, which any Windows
/// user may make, is followed too.
#[cfg(windows)]
#[test]
fn a_junction_in_the_matter_folder_into_the_data_folder_is_denied() {
    let dir = layout("junction");
    let root = dir.path();
    let junction = root.join("matter").join("packs");
    let target = dunce::canonicalize(root.join("data").join("skill-packs")).unwrap();
    clauseo_sandbox::windows::test_junction::junction(&junction, &target).unwrap();
    let through = junction.join(r"x\skills\brief\SKILL.md");
    assert!(check(root, &[&text(&through)], None).touches_data());
    let relative = json!({"command": r"type packs\x\skills\brief\SKILL.md"});
    assert!(check(root, &[], Some(&relative)).touches_data());
    let search = json!({"pattern": "secret", "path": text(&junction)});
    assert!(check(root, &[], Some(&search)).touches_data());
}

/// The review's D2: nothing of the matter's own Codex state folder is the session's;
/// its login link (hard-linked to the live login), `config.toml`, databases and `.env` are
/// refused, by path, by `cat` and by `~/` (Codex's `HOME` is its state folder on macOS and
/// Linux), and so is the login home's `auth.json`, as at the base.
#[test]
fn review_the_own_state_folders_login_link_is_auto_denied() {
    let dir = layout("own_login");
    let root = dir.path();
    let data = root.join("data");
    let state = data.join("agent-state/m1/codex-acp");
    for name in ["tmp/arg0/codex-arg0abc", "sessions"] {
        write::create_dir_all(&state.join(name)).unwrap();
    }
    for name in ["auth.json", "config.toml", ".env", "state_5.sqlite"] {
        write::write_file(&state.join(name), b"x").unwrap();
    }
    write::create_dir_all(&data.join("agents/codex-acp/home")).unwrap();
    write::write_file(&data.join("agents/codex-acp/home/auth.json"), b"x").unwrap();
    let codex = |paths: &[&str], raw: Option<&serde_json::Value>| {
        let mut check = check(root, paths, raw);
        check.allowed = vec![data.join("run/s1"), data.join("agent-state/m1/notes")];
        check
            .allowed
            .extend(state_folder::fence_own(&state, AgentKind::Codex));
        check.denied = state_folder::fence_denied(&state, AgentKind::Codex);
        check.home = Some(state.clone());
        check
    };
    let base = text(&data.join("agents/codex-acp/home/auth.json"));
    let cat = json!({"command": format!("cat '{base}'")});
    assert!(codex(&[], Some(&cat)).touches_data(), "base");
    for name in [
        "auth.json",
        "config.toml",
        ".env",
        "state_5.sqlite",
        "sessions/r.jsonl",
    ] {
        let path = text(&state.join(name));
        assert!(codex(&[&path], None).touches_data(), "Read {name}");
        let cat = json!({"command": format!("cat '{path}'")});
        assert!(
            codex(&[], Some(&cat)).touches_data(),
            "head: the own matter's {name} is not auto-denied"
        );
    }
    let home = json!({"command": "cat ~/auth.json"});
    assert!(codex(&[], Some(&home)).touches_data(), "~/auth.json");
    // Round 3's H1: the state folder's `tmp` (Codex's `arg0` links, first on its `PATH`) is
    // no folder of the session's own.
    let arg0 = text(&state.join("tmp/arg0/codex-arg0abc/git"));
    assert!(codex(&[&arg0], None).touches_data(), "arg0 is not own");
}

/// The session's own run folder, the notes, the matter and Claude's plans are its own; another
/// session's run folder is not; a path elsewhere is not the data folder's.
#[test]
fn the_sessions_own_folders_and_an_unrelated_path_are_not_denied() {
    let dir = layout("own");
    let root = dir.path();
    for path in [
        root.join("data/run/s1/skill-packs/x/SKILL.md"),
        root.join("data/run/s1/work/out.docx"),
        root.join("data/agent-state/m1/notes/n.md"),
        root.join("data/agent-state/m1/claude-acp/plans/plan.md"),
        root.join("matter/sub/a.txt"),
        root.join("outside/x.txt"),
        PathBuf::from("sub/a.txt"),
    ] {
        assert!(
            !check(root, &[&text(&path)], None).touches_data(),
            "{path:?}"
        );
    }
    let shell =
        json!({"command": format!("ls {} https://example.com/a", text(&root.join("outside")))});
    assert!(!check(root, &[], Some(&shell)).touches_data());
    for other in [
        root.join("data/run/s2/work/a.txt"),
        root.join("data/agents/claude-acp/home/.credentials.json"),
    ] {
        assert!(
            check(root, &[&text(&other)], None).touches_data(),
            "{other:?}"
        );
    }
}

/// A data path in another case matches where the file system ignores case, and not elsewhere.
#[test]
fn case_is_ignored_only_where_the_file_system_ignores_it() {
    let dir = layout("case");
    let root = dir.path();
    let upper = root.join("DATA/skill-packs/x/skills/brief/SKILL.md");
    let fence = check(root, &[&text(&upper)], None);
    assert!(fence.touches_data_as(true));
    if !cfg!(any(windows, target_os = "macos")) {
        assert!(!fence.touches_data_as(false));
    }
}
