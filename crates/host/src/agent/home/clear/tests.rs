use std::path::{Path, PathBuf};

use super::{ASIDE, LOADED, SETTINGS, clear_home, clear_settings};
use crate::write;

/// A fresh data folder under the temp folder, named for `test`, holding an empty home; both.
fn fresh(test: &str) -> (PathBuf, PathBuf) {
    let data = std::env::temp_dir().join(format!(
        "clauseo-claude-clear-{test}-{}",
        std::process::id()
    ));
    write::remove_dir_all(&data).unwrap();
    // As `Locations::agent_home` lays it out.
    let home = data.join("agents").join("claude-acp").join("home");
    write::create_dir_all(&home).unwrap();
    (data, home)
}

/// The names in `home`, sorted.
fn names(home: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Makes `link` a link to the folder `target`: a symlink on Unix, a junction on Windows.
fn link_folder(target: &Path, link: &Path) {
    #[cfg(unix)]
    write::symlink(target, link).unwrap();
    #[cfg(windows)]
    clauseo_sandbox::windows::test_junction::junction(link, target).unwrap();
}

/// Plants each of `planted` in `home`: a file for a name with a `.`, else a folder holding a
/// skill.
fn plant(home: &Path, planted: &[&str]) {
    for name in planted {
        let path = home.join(name);
        if name.contains('.') {
            write::write_file(&path, b"planted").unwrap();
        } else {
            write::create_dir_all(&path.join("x")).unwrap();
            write::write_file(&path.join("x").join("SKILL.md"), b"planted").unwrap();
        }
    }
}

/// What Claude Code keeps in its home, each left as it was: its global config holding state
/// alone, its sign-in, the settings Clauseo writes, its plans and its projects.
const KEPT: [(&str, &str); 5] = [
    (
        ".claude.json",
        r#"{"userID": "u", "oauthAccount": {"accountUuid": "a"}}"#,
    ),
    (".credentials.json", "credentials"),
    ("settings.json", "settings"),
    ("plans/note.md", "plan"),
    ("projects/p/tool-results/x.txt", "result"),
];

/// Writes each of [`KEPT`] in `home`.
fn keep(home: &Path) {
    for (kept, bytes) in KEPT {
        let path = kept
            .split('/')
            .fold(home.to_owned(), |at, part| at.join(part));
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, bytes.as_bytes()).unwrap();
    }
}

/// Each loaded name and each settings file goes, a folder with what it holds or a file, and
/// nothing is left aside; Claude Code's own state stays byte for byte; `skills`, `commands` and
/// `agents` do not exist afterwards, not even empty. A second clear finds nothing.
#[test]
fn a_launch_removes_what_claude_code_loads_from_its_home() {
    let (data, home) = fresh("loaded");
    plant(&home, &LOADED);
    plant(&home, &SETTINGS);
    keep(&home);
    clear_home(&data, &home).unwrap();
    assert_eq!(
        names(&home),
        [
            ".claude.json",
            ".credentials.json",
            "plans",
            "projects",
            "settings.json"
        ]
    );
    for (kept, bytes) in KEPT {
        let path = kept.split('/').fold(home.clone(), |at, part| at.join(part));
        assert_eq!(std::fs::read(&path).unwrap(), bytes.as_bytes());
    }
    for watched in ["skills", "commands", "agents"] {
        assert!(
            std::fs::symlink_metadata(home.join(watched)).is_err(),
            "{watched}"
        );
    }
    clear_home(&data, &home).unwrap();
    assert!(!names(&home).iter().any(|name| name.starts_with(ASIDE)));
    write::remove_dir_all(&data).unwrap();
}

/// A launch removes the configuration from Claude Code's global config and keeps its state.
#[test]
fn a_launch_strips_the_configuration_from_the_global_config() {
    let (data, home) = fresh("global");
    let config = br#"{"env": {"ANTHROPIC_BASE_URL": "https://example.invalid"}, "userID": "u"}"#;
    write::write_file(&home.join(".claude.json"), config).unwrap();
    clear_home(&data, &home).unwrap();
    let kept: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.join(".claude.json")).unwrap()).unwrap();
    assert_eq!(kept, serde_json::json!({"userID": "u"}));
    write::remove_dir_all(&data).unwrap();
}

/// Before a sign-in, status check or logout, the settings files and the global config's
/// configuration go; the folders only a session loads stay for the next launch.
#[test]
fn a_sign_in_run_removes_the_settings_files_alone() {
    let (data, home) = fresh("sign-in");
    plant(&home, &LOADED);
    plant(&home, &SETTINGS);
    keep(&home);
    clear_settings(&home).unwrap();
    let mut want: Vec<String> = LOADED.iter().map(|name| (*name).to_owned()).collect();
    want.extend(
        [
            ".claude.json",
            ".credentials.json",
            "plans",
            "projects",
            "settings.json",
        ]
        .map(String::from),
    );
    want.sort();
    assert_eq!(names(&home), want);
    write::remove_dir_all(&data).unwrap();
}

/// A loaded name that is a link to a folder elsewhere (a symlink on Unix, a junction on
/// Windows) goes itself; the folder it led to keeps every file.
#[cfg(any(unix, windows))]
#[test]
fn a_linked_loaded_name_goes_and_its_target_stays() {
    let (data, home) = fresh("linked");
    let elsewhere = data.join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    write::write_file(&elsewhere.join("SKILL.md"), b"kept").unwrap();
    link_folder(&elsewhere, &home.join("skills"));
    clear_home(&data, &home).unwrap();
    assert!(std::fs::symlink_metadata(home.join("skills")).is_err());
    assert_eq!(std::fs::read(elsewhere.join("SKILL.md")).unwrap(), b"kept");
    write::remove_dir_all(&data).unwrap();
}

/// A home swapped for a link after the launch made it refuses the launch, and a sign-in run's
/// clear too; the folder it leads to keeps every name the clear would remove.
#[cfg(any(unix, windows))]
#[test]
fn a_home_swapped_for_a_link_refuses_the_launch_and_its_target_stays() {
    let (data, home) = fresh("swapped");
    let elsewhere = data.join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    plant(&elsewhere, &[".config.json", "CLAUDE.md", "skills"]);
    write::remove_dir_all(&home).unwrap();
    link_folder(&elsewhere, &home);
    let error = clear_home(&data, &home).unwrap_err();
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    assert!(clear_settings(&home).is_err());
    assert_eq!(names(&elsewhere), [".config.json", "CLAUDE.md", "skills"]);
    write::remove_link(&home).unwrap();
    write::remove_dir_all(&data).unwrap();
}

/// A loaded name that cannot be renamed (the home not writable) refuses the launch with
/// Clauseo's words for a home it could not prepare. Skipped where the folder's mode does not
/// stop the writes (a root account).
#[cfg(unix)]
#[test]
fn a_loaded_name_that_cannot_be_moved_refuses_the_launch() {
    let (data, home) = fresh("stuck");
    write::create_dir_all(&home.join("skills")).unwrap();
    write::set_mode(&home, 0o500).unwrap();
    if write::create_dir_all(&home.join("probe")).is_ok() {
        eprintln!("skipped: the folder's mode does not stop this account's writes");
    } else {
        let error = clear_home(&data, &home).unwrap_err();
        assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    }
    write::set_mode(&home, 0o700).unwrap();
    write::remove_dir_all(&data).unwrap();
}

/// A loaded folder holding a file another program keeps open without letting it be deleted
/// cannot be renamed: the launch is refused with Clauseo's words for a home it could not
/// prepare.
#[cfg(windows)]
#[test]
fn a_loaded_name_that_cannot_be_moved_refuses_the_launch() {
    let (data, home) = fresh("stuck");
    let held = home.join("skills").join("x").join("SKILL.md");
    write::create_dir_all(held.parent().unwrap()).unwrap();
    write::write_file(&held, b"held").unwrap();
    let open = write::open_deny_write(&held).unwrap();
    let error = clear_home(&data, &home).unwrap_err();
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    drop(open);
    write::remove_dir_all(&data).unwrap();
}
