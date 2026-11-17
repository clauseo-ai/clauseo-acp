//! Claude's plugin folder holds only what the profile ships (`clauseo_profile::match_plugin`):
//! what else is in it goes, files, folders, manifest entries and links, and what the profile
//! wrote stays byte for byte.

use std::path::Path;

use clauseo_profile::skills::builtin::BUILTIN;
use clauseo_profile::skills::manifest::MANIFEST;
use clauseo_profile::test_support::tree_text;
use clauseo_profile::{ClauseoProfile, match_plugin, write_skills};
use clauseo_protocol::AgentKind;

use super::{Scratch, profile_paths, read, write};

/// Plants files the profile never ships in the plugin folder `plugin`: a new skill, a file in a
/// shipped skill's folder, a file beside the plugin manifest and a folder of hooks; and the new
/// skill's entry in the skills folder's manifest.
#[cfg(test)]
fn plant(plugin: &Path) {
    let skills = plugin.join("skills");
    for file in [
        skills.join("planted").join("SKILL.md"),
        skills.join(BUILTIN[0]).join("extra.md"),
        plugin.join(".claude-plugin").join("hooks.json"),
        plugin.join("hooks").join("hooks.json"),
    ] {
        write::write(&file, b"---\nname: planted\ndescription: d\n---\nx\n").unwrap();
    }
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&read(&skills.join(MANIFEST))).unwrap();
    manifest["skills"]["planted"] = serde_json::json!({"version": "1", "files": {}});
    let text = serde_json::to_vec(&manifest).unwrap();
    write::write(&skills.join(MANIFEST), &text).unwrap();
}

/// A new skill file planted in the built-in plugin, with the rest, is gone after the launch's
/// match, before the next session; the shipped files are as written; and neither Codex's index
/// nor its keys name it. A second match finds nothing.
#[test]
fn a_planted_skill_in_the_built_in_plugin_is_removed_and_never_listed() {
    let root = Scratch::new("plugin-match-planted");
    let paths = profile_paths(&root);
    let (profile, _) = ClauseoProfile::new(paths.clone(), None).with_builtin_skills(None);
    let writes = profile.launch_writes(Some(AgentKind::Codex));
    let written = write_skills(&writes[0]).unwrap();
    let shipped = tree_text(paths.claude_plugin());
    plant(paths.claude_plugin());
    assert_eq!(match_plugin(&writes[0]).unwrap(), 4);
    assert_eq!(tree_text(paths.claude_plugin()), shipped);
    let skills = paths.claude_plugin().join("skills");
    assert!(!skills.join("planted").exists());
    assert!(!paths.claude_plugin().join("hooks").exists());
    assert_eq!(match_plugin(&writes[0]).unwrap(), 0);
    let keys = profile.codex_keys_with_packs(Some(AgentKind::Codex), Some(&written), None);
    let keys = serde_json::to_string(&keys).unwrap();
    assert!(!keys.contains("planted"), "{keys}");
    assert!(!profile.skills_index().contains("planted"));
    // Codex's own skills folder (none at launch) has no plugin to match.
    for write in writes.iter().skip(1) {
        assert_eq!(match_plugin(write).unwrap(), 0);
    }
}

/// Planted before the write, the match first and then the write leave the folder as a fresh
/// write makes it.
#[test]
fn a_match_before_the_write_leaves_what_a_fresh_write_makes() {
    let root = Scratch::new("plugin-match-before");
    let paths = profile_paths(&root);
    let (profile, _) = ClauseoProfile::new(paths.clone(), None).with_builtin_skills(None);
    let write = &profile.launch_writes(Some(AgentKind::Claude))[0];
    write_skills(write).unwrap();
    let shipped = tree_text(paths.claude_plugin());
    plant(paths.claude_plugin());
    assert_eq!(match_plugin(write).unwrap(), 4);
    write_skills(write).unwrap();
    assert_eq!(match_plugin(write).unwrap(), 0);
    assert_eq!(tree_text(paths.claude_plugin()), shipped);
}

/// A link in the plugin folder is removed itself, never followed: what it points at stays.
#[cfg(unix)]
#[test]
fn a_link_in_the_plugin_is_removed_and_its_target_kept() {
    let root = Scratch::new("plugin-match-link");
    let paths = profile_paths(&root);
    let (profile, _) = ClauseoProfile::new(paths.clone(), None).with_builtin_skills(None);
    let write = &profile.launch_writes(Some(AgentKind::Claude))[0];
    write_skills(write).unwrap();
    let elsewhere = root.join("elsewhere");
    write::write(&elsewhere.join("SKILL.md"), b"kept").unwrap();
    let link = paths.claude_plugin().join("skills").join("linked");
    write::symlink_dir(&elsewhere, &link).unwrap();
    assert_eq!(match_plugin(write).unwrap(), 1);
    assert!(std::fs::symlink_metadata(&link).is_err());
    assert_eq!(read(&elsewhere.join("SKILL.md")), b"kept");
}
