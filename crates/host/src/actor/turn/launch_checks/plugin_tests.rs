//! The launch's skills write with the built-in plugin's match (`folders::write_skills`): a
//! plugin folder swapped for a link (a symlink on Unix, a junction on Windows) is replaced by a
//! real folder and written, and nothing in the folder it led to is changed; a plugin that cannot
//! be made to hold only what Clauseo ships refuses the launch.

use std::path::{Path, PathBuf};

use clauseo_profile::{BrowserSidecars, ClauseoProfile, ProfilePaths, SkillWrite};
use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;

use super::write_skills;
use crate::write;

/// A fresh folder for one test, removed when it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("clauseo-host-plugin-{test}-{}", std::process::id()));
        write::remove_dir_all(&root).unwrap();
        write::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

/// Claude's launch writes for a profile under `root`, and its plugin folder.
fn claude_writes(root: &Path) -> (Vec<SkillWrite>, PathBuf) {
    let paths = ProfilePaths::new(
        &Locations::under_root(root),
        root.join("app").join("clauseo"),
        root.join("stetdoc").join("stetdoc"),
        BrowserSidecars {
            clauseo_mcp: root.join("app").join("clauseo-mcp"),
            agent_browser: root.join("app").join("agent-browser"),
        },
    )
    .unwrap();
    let plugin = paths.claude_plugin().to_path_buf();
    let (profile, _) = ClauseoProfile::new(paths, None).with_builtin_skills(None);
    (profile.launch_writes(Some(AgentKind::Claude)), plugin)
}

/// Makes `link` a link to the folder `target`: a symlink on Unix, a junction on Windows.
fn link_folder(target: &Path, link: &Path) {
    #[cfg(unix)]
    write::symlink(target, link).unwrap();
    #[cfg(windows)]
    clauseo_sandbox::windows::test_junction::junction(link, target).unwrap();
}

/// The plugin folder, then its parent, swapped for a link to a folder elsewhere that holds
/// files named as the plugin's are and one of its own: the launch's write leaves a real folder
/// there holding what a fresh write makes, and every file of the folder elsewhere keeps its
/// bytes.
#[cfg(any(unix, windows))]
#[test]
fn a_linked_plugin_folder_is_replaced_and_its_targets_files_survive() {
    let scratch = Scratch::new("linked");
    let (writes, plugin) = claude_writes(&scratch.0);
    write_skills(&writes).unwrap();
    let shipped = clauseo_profile::test_support::tree_text(&plugin);
    let parent = plugin.parent().unwrap().to_path_buf();
    for linked in [plugin.clone(), parent] {
        let elsewhere = scratch.0.join("elsewhere");
        write::remove_dir_all(&elsewhere).unwrap();
        let kept = [
            elsewhere.join("notes.txt"),
            elsewhere.join(".claude-plugin").join("plugin.json"),
            elsewhere
                .join("skills")
                .join("legal-drafting")
                .join("SKILL.md"),
        ];
        for file in &kept {
            write::create_dir_all(file.parent().unwrap()).unwrap();
            write::write_file(file, b"elsewhere").unwrap();
        }
        write::remove_dir_all(&linked).unwrap();
        link_folder(&elsewhere, &linked);
        write_skills(&writes).unwrap();
        let meta = std::fs::symlink_metadata(&linked).unwrap();
        assert!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "{linked:?}"
        );
        assert_eq!(clauseo_profile::test_support::tree_text(&plugin), shipped);
        for file in &kept {
            assert_eq!(std::fs::read(file).unwrap(), b"elsewhere", "{file:?}");
        }
    }
}

/// A stray file in the plugin that can be neither moved aside nor removed (its folder and the
/// plugin's parent not writable) refuses the launch with `agent.launch_failed`. Skipped where
/// the folders' modes do not stop the writes (a root account).
#[cfg(unix)]
#[test]
fn a_plugin_that_cannot_be_matched_refuses_the_launch() {
    let scratch = Scratch::new("stuck");
    let (writes, plugin) = claude_writes(&scratch.0);
    write_skills(&writes).unwrap();
    let skills = plugin.join("skills");
    write::write_file(&skills.join("stray.md"), b"x").unwrap();
    let parent = plugin.parent().unwrap().to_path_buf();
    for folder in [&skills, &parent] {
        write::set_mode(folder, 0o555).unwrap();
    }
    if write::create_dir_all(&skills.join("probe")).is_ok() {
        eprintln!("skipped: the folders' modes do not stop this account's writes");
    } else {
        let error = write_skills(&writes).unwrap_err();
        assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    }
    for folder in [&skills, &parent] {
        write::set_mode(folder, 0o755).unwrap();
    }
}
