use std::path::{Component, Path, PathBuf};

use clauseo_settings::Locations;
use serde_json::{Map, Value, json};

use super::{EXTENDS, PROFILE, SET_ASIDE_GLOB, Session, keys};

/// What of `<data>` is left readable to a Codex shell on purpose, with why. The guard test checks
/// that each `Locations` path is denied or here.
const ALLOWED: [(&str, &str); 2] = [
    (
        "agents/codex-acp",
        "Codex's own install and home: the sandbox re-runs the Codex binary, and a read-only \
         carve-back under a deny stays hidden on Linux",
    ),
    (
        "profile",
        "Clauseo's built-in plugin, the same for every matter: Codex's skills index names files \
         in profile/claude-plugin/skills",
    ),
];

const ROOT: &str = "/home/lawyer";

pub(super) fn locations() -> Locations {
    Locations::under_root(Path::new(ROOT))
}

pub(super) fn data() -> PathBuf {
    locations().data_dir().to_path_buf()
}

pub(super) fn run_root() -> PathBuf {
    data().join("run").join("s1")
}

pub(super) fn notes() -> PathBuf {
    data().join("agent-state").join("m1").join("notes")
}

/// The matter's Codex state folder, the session's `CODEX_HOME` (R17 PR-C).
pub(super) fn state() -> PathBuf {
    data().join("agent-state").join("m1").join("codex-acp")
}

fn matter() -> PathBuf {
    PathBuf::from("/home/lawyer/Matters/Smith v Jones")
}

/// The `agent-state` denies `state_denies` reads at a launch after the clean: another matter,
/// this matter's Claude folder, what Codex kept in its state folder, and the names denied even
/// when absent. Not `tmp`, not `notes`.
fn denies() -> Vec<PathBuf> {
    let agent_state = data().join("agent-state");
    let mut denies = vec![
        agent_state.join("m2"),
        agent_state.join("m1").join("claude-acp"),
    ];
    denies.extend(
        [
            "sessions",
            "archived_sessions",
            "history.jsonl",
            "state_5.sqlite",
            "state_5.sqlite-wal",
            "log",
            "auth.json",
            ".env",
            "config.toml",
        ]
        .map(|name| state().join(name)),
    );
    denies
}

pub(super) fn fence() -> Map<String, Value> {
    let (run_root, notes, state, state_denies) = (run_root(), notes(), state(), denies());
    let session = Session {
        run_root: &run_root,
        notes: &notes,
        state: &state,
        state_denies: &state_denies,
    };
    keys(&locations(), session, false)
}

/// The profile's rules as codex-rs compiles them: each scoped subpath joined to its base, each
/// top-level key as it is, with its mode; glob rules apart.
fn compiled(keys: &Map<String, Value>) -> (Vec<(PathBuf, String)>, Vec<String>) {
    let mut exact = Vec::new();
    let mut globs = Vec::new();
    let filesystem = keys["permissions"][PROFILE]["filesystem"]
        .as_object()
        .expect("a filesystem table");
    for (key, value) in filesystem {
        match value {
            Value::String(mode) => exact.push((PathBuf::from(key), mode.clone())),
            Value::Object(scoped) => {
                for (subpath, mode) in scoped {
                    let mode = mode.as_str().expect("a mode").to_owned();
                    if subpath.contains(['*', '?', '[', ']']) {
                        assert_eq!(mode, "deny", "a glob rule is a deny");
                        globs.push(format!("{key}/{subpath}"));
                    } else {
                        exact.push((Path::new(key).join(subpath), mode));
                    }
                }
            }
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_)) => {
                panic!("not a mode or a table: {other}")
            }
        }
    }
    (exact, globs)
}

/// A glob matched as bubblewrap applies it: against `path` and its ancestors (a match masks the
/// folder whole), component by component, `*` within a component, and a last `**` matching one
/// component or more.
fn glob_matches(glob: &str, path: &Path) -> bool {
    let parts: Vec<&str> = glob.split('/').filter(|part| !part.is_empty()).collect();
    let fits = |pattern: &str, name: &str| wildcard(pattern, name);
    path.ancestors().any(|ancestor| {
        let names: Vec<String> = ancestor
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => name.to_str().map(str::to_owned),
                Component::Prefix(_)
                | Component::RootDir
                | Component::CurDir
                | Component::ParentDir => None,
            })
            .collect();
        match parts.split_last() {
            Some((&"**", head)) => {
                names.len() > head.len()
                    && head
                        .iter()
                        .zip(&names)
                        .all(|(pattern, name)| fits(pattern, name))
            }
            _ => {
                names.len() == parts.len()
                    && parts
                        .iter()
                        .zip(&names)
                        .all(|(pattern, name)| fits(pattern, name))
            }
        }
    })
}

/// Whether `name` fits `pattern`, whose `*`s each stand for any run of characters.
fn wildcard(pattern: &str, name: &str) -> bool {
    let mut pieces = pattern.split('*');
    let first = pieces.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let pieces: Vec<&str> = pieces.collect();
    let Some((last, middle)) = pieces.split_last() else {
        return rest.is_empty();
    };
    for piece in middle {
        match rest.find(piece) {
            Some(at) => rest = rest.get(at + piece.len()..).unwrap_or_default(),
            None => return false,
        }
    }
    rest.ends_with(last)
}

fn rank(mode: &str) -> u8 {
    match mode {
        "read" => 0,
        "write" => 1,
        _ => 2,
    }
}

/// How a turn's policy is made (codex-rs `core/src/session/session.rs:491-507`).
#[derive(Clone, Copy)]
enum Turn {
    /// The profile as loaded: all its entries, no extra roots.
    Profile,
    /// Rebuilt from codex-acp's `workspaceWrite` policy: its writable roots (the matter folder
    /// and the session's `additionalDirectories`, `profile_loads::codex_directories`) and the
    /// profile's deny entries only (`protocol/src/permissions.rs:720-728`).
    Rebuilt,
}

/// The access a Codex shell gets to `path` in `turn`, the way codex-rs decides it
/// (`protocol/src/permissions.rs:1043-1048`): `/` readable and the matter folder writable
/// (`:workspace`), with the rules `turn` keeps; the deepest matching rule wins, and `deny` wins
/// a tie. A deny glob that matches denies.
fn access_in(keys: &Map<String, Value>, turn: Turn, path: &Path) -> String {
    let (exact, globs) = compiled(keys);
    if globs.iter().any(|glob| glob_matches(glob, path)) {
        return "deny".to_owned();
    }
    let mut rules = vec![
        (PathBuf::from("/"), "read".to_owned()),
        (matter(), "write".to_owned()),
    ];
    match turn {
        Turn::Profile => rules.extend(exact),
        Turn::Rebuilt => {
            rules.extend(exact.into_iter().filter(|(_, mode)| mode == "deny"));
            rules.extend([run_root(), notes()].map(|root| (root, "write".to_owned())));
        }
    }
    rules
        .into_iter()
        .filter(|(root, _)| path.starts_with(root))
        .max_by_key(|(root, mode)| (root.components().count(), rank(mode)))
        .map_or_else(|| "deny".to_owned(), |(_, mode)| mode)
}

/// The access in both kinds of turn, which must agree.
pub(super) fn access(keys: &Map<String, Value>, path: &Path) -> String {
    let profile = access_in(keys, Turn::Profile, path);
    assert_eq!(
        profile,
        access_in(keys, Turn::Rebuilt, path),
        "{}",
        path.display()
    );
    profile
}

fn readable(keys: &Map<String, Value>, path: &Path) -> bool {
    access(keys, path) != "deny"
}

#[test]
fn the_fence_selects_a_profile_extending_workspace_scoped_under_the_data_folder() {
    let keys = fence();
    assert_eq!(keys["default_permissions"], json!(PROFILE));
    assert_eq!(keys["permissions"][PROFILE]["extends"], json!(EXTENDS));
    assert_eq!(EXTENDS, ":workspace");
    let filesystem = keys["permissions"][PROFILE]["filesystem"]
        .as_object()
        .unwrap();
    let base = data().to_str().unwrap().to_owned();
    let scoped = filesystem[&base].as_object().expect("rules under <data>");
    for subpath in [
        "logs",
        "settings.json",
        "state.sqlite",
        "search.sqlite",
        "search.sqlite-wal",
        "skill-packs",
        "agents/claude-acp",
        "agents/codex-acp/home",
        "run",
        "agent-state/m2",
        "agent-state/m1/claude-acp",
        "agent-state/m1/codex-acp/auth.json",
        SET_ASIDE_GLOB,
    ] {
        assert_eq!(scoped.get(subpath), Some(&json!("deny")), "{subpath}");
    }
    // `agent-state` is denied entry by entry, so the own state folder's `tmp` stays readable.
    assert_eq!(scoped.get("agent-state"), None);
    for (subpath, value) in scoped {
        let own = ["run/s1", "agent-state/m1/notes"].contains(&subpath.as_str());
        assert_eq!(
            value,
            &json!(if own { "write" } else { "deny" }),
            "{subpath}"
        );
    }
}

/// Under the profile Codex reads no `writable_roots` from `CODEX_CONFIG` (stage B, live): the
/// profile itself names the session's run folder and notes `write`, exactly.
#[test]
fn the_profile_names_the_own_run_folder_and_notes_write() {
    let keys = fence();
    let scoped = &keys["permissions"][PROFILE]["filesystem"][data().to_str().unwrap()];
    assert_eq!(scoped["run/s1"], json!("write"));
    assert_eq!(scoped["agent-state/m1/notes"], json!("write"));
    for path in [
        run_root().join("work").join("a.md"),
        notes().join("brief.md"),
    ] {
        assert_eq!(
            access_in(&keys, Turn::Profile, &path),
            "write",
            "{}",
            path.display()
        );
    }
}

/// A session made after the launch is denied, in the profile and in every turn rebuilt from
/// codex-acp's policy, while the own run folder and notes stay writable. A matter made after the
/// launch is readable, never writable, for the session (round 4's ruling, the named residual).
#[test]
fn sessions_and_matters_made_after_the_launch_are_denied() {
    let keys = fence();
    let data = data();
    for path in [
        data.join("run").join("s-later").join("work").join("a.txt"),
        data.join("run").join("s-later").join("skill-packs"),
    ] {
        assert_eq!(access(&keys, &path), "deny", "{}", path.display());
    }
    for path in [
        data.join("agent-state")
            .join("m-later")
            .join("notes")
            .join("brief.md"),
        data.join("agent-state").join("m1").join("other"),
    ] {
        assert_eq!(access(&keys, &path), "read", "{}", path.display());
    }
    for path in [
        run_root().join("work").join("a.md"),
        run_root().join("skill-packs").join("x").join("SKILL.md"),
        notes().join("matter-brief.md"),
    ] {
        assert_eq!(access(&keys, &path), "write", "{}", path.display());
    }
}

#[test]
fn search_the_store_and_other_matters_and_sessions_are_denied() {
    let keys = fence();
    let data = data();
    for path in [
        data.join("search.sqlite"),
        data.join("search.sqlite-wal"),
        data.join("state.sqlite"),
        data.join("state.sqlite-shm"),
        data.join("agent-state")
            .join("m2")
            .join("notes")
            .join("brief.md"),
        data.join("run")
            .join("s2")
            .join("skill-packs")
            .join("x")
            .join("SKILL.md"),
        data.join("run").join("s2").join("browser"),
        data.join("run.old-0").join("s9").join("work").join("a.txt"),
        data.join("skill-packs").join("pack").join("SKILL.md"),
        data.join("logs").join("host.log"),
        data.join("agents")
            .join("claude-acp")
            .join("home")
            .join(".credentials.json"),
        state().join("auth.json"),
        state().join("state_5.sqlite-wal"),
        state().join("history.jsonl"),
        state().join("sessions").join("2026").join("t.jsonl"),
        state().join("config.toml"),
        state().join(".env"),
        // The login home whole: its login and the transcripts from before R17 PR-C.
        locations().agent_home("codex-acp").join("auth.json"),
        locations()
            .agent_home("codex-acp")
            .join("sessions")
            .join("old.jsonl"),
        // Another matter's state folder (R17 PR-C), made before or after the launch.
        data.join("agent-state")
            .join("m2")
            .join("codex-acp")
            .join("sessions")
            .join("r.jsonl"),
        data.join("agent-state")
            .join("m2")
            .join("claude-acp")
            .join("projects"),
    ] {
        assert!(!readable(&keys, &path), "{} is readable", path.display());
    }
}

#[test]
fn the_own_folders_state_folder_and_the_built_in_skills_stay_readable() {
    let keys = fence();
    for path in [
        run_root().join("work").join("notes.md"),
        run_root().join("skill-packs").join("x").join("SKILL.md"),
        notes().join("matter-brief.md"),
        data()
            .join("agents")
            .join("codex-acp")
            .join("install")
            .join("codex"),
        data()
            .join("profile")
            .join("claude-plugin")
            .join("skills")
            .join("s")
            .join("SKILL.md"),
        matter().join("brief.txt"),
    ] {
        assert!(readable(&keys, &path), "{} is denied", path.display());
    }
    assert_eq!(access(&keys, &matter().join("a.txt")), "write");
}

/// The review's D1: nothing Codex or its shell loads at the next launch from the state folder
/// (its `CODEX_HOME` and `HOME`) is writable by the shell: `.env`, which Codex exports at
/// start, git's `.config/git/config`, `agents` role files, `AGENTS.md`, `config.toml`. The login
/// link, `.env` and `config.toml` are denied outright, present or not.
#[test]
fn review_codex_home_dotenv_and_git_xdg_config_are_not_writable() {
    let keys = fence();
    for name in [
        ".env",
        ".config/git/config",
        "agents/role.toml",
        "AGENTS.md",
        "config.toml",
        "hooks.json",
        "auth.json",
        ".gitconfig",
    ] {
        assert_ne!(access(&keys, &state().join(name)), "write", "{name}");
    }
    for name in ["auth.json", ".env", "config.toml"] {
        assert_eq!(access(&keys, &state().join(name)), "deny", "{name}");
    }
}

/// Every path `Locations` defines under the data folder is denied, or allowed for a reason
/// (`ALLOWED`). A new accessor goes here; a rename follows, since the list is derived.
#[test]
fn every_locations_path_is_denied_or_allowed_with_a_reason() {
    let locations = locations();
    let data = data();
    let keys = fence();
    let defined = [
        locations.log_dir(),
        locations.updates_dir(),
        locations.update_marker(),
        locations.settings_file().to_path_buf(),
        locations.agent_home("claude-acp"),
        locations.agent_home("codex-acp"),
    ];
    for path in defined {
        let relative = path.strip_prefix(&data).unwrap_or(&path);
        let allowed = ALLOWED
            .iter()
            .find(|(prefix, _)| relative.starts_with(prefix));
        match allowed {
            Some((prefix, reason)) => assert!(reason.len() > 20, "{prefix}"),
            None => assert!(!readable(&keys, &path), "{} is readable", path.display()),
        }
    }
    for (prefix, reason) in ALLOWED {
        assert!(
            Path::new(prefix)
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        );
        assert!(reason.len() > 20, "{prefix} needs a reason");
    }
}

#[test]
fn windows_gets_no_fence() {
    let (run_root, notes, state) = (run_root(), notes(), state());
    let session = Session {
        run_root: &run_root,
        notes: &notes,
        state: &state,
        state_denies: &[],
    };
    assert!(keys(&locations(), session, true).is_empty());
}

#[test]
fn a_data_folder_with_a_glob_character_is_named_literally_without_the_set_aside_glob() {
    let locations = Locations::under_root(Path::new("/srv/Clauseo[1]"));
    let data = locations.data_dir().to_path_buf();
    let (run_root, notes, state) = (
        data.join("run").join("s1"),
        data.join("agent-state").join("m1").join("notes"),
        data.join("agent-state").join("m1").join("codex-acp"),
    );
    let session = Session {
        run_root: &run_root,
        notes: &notes,
        state: &state,
        state_denies: &[],
    };
    let keys = keys(&locations, session, false);
    let scoped = keys["permissions"][PROFILE]["filesystem"][data.to_str().unwrap()]
        .as_object()
        .expect("rules scoped under the literal data folder");
    assert_eq!(scoped.get("search.sqlite"), Some(&json!("deny")));
    assert_eq!(scoped.get(SET_ASIDE_GLOB), None);
}

#[test]
fn the_fence_reaches_codex_config_whole_and_the_hook_cannot_set_it() {
    use crate::agent::loads::browser::Switch;
    use crate::agent::loads::profile::{Folders, codex_keys};

    let Value::Object(hook) = json!({
        "developer_instructions": "Work.",
        "default_permissions": ":danger-no-sandbox",
        "permissions": {"open": {"filesystem": {":root": "write"}}},
    }) else {
        panic!("the hook is an object")
    };
    let work = run_root().join("work");
    let notes = notes();
    let folders = Folders::of(&work, &notes).unwrap();
    let switch = Switch {
        on: false,
        locked: false,
    };
    let mut launch = codex_keys(hook, switch, folders).unwrap();
    assert_eq!(launch.get("default_permissions"), None);
    assert_eq!(launch.get("permissions"), None);
    let fence = fence();
    launch.extend(fence.clone());
    let config =
        crate::agent::codex::config_on(&std::collections::BTreeMap::new(), launch, None, false);
    assert_eq!(config["default_permissions"], fence["default_permissions"]);
    assert_eq!(config["permissions"], fence["permissions"]);
    assert_eq!(config.get("sandbox_mode"), None);
}
