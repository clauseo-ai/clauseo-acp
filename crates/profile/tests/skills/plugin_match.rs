//! Claude's plugin folder holds only what the profile ships (`clauseo_profile::match_plugin`):
//! what else is in it goes, files, folders, manifest entries and links; a shipped file whose
//! bytes or link count differ is written again; and what the profile wrote stays byte for byte.
//! Nothing a link leads to is changed, the plugin folder itself linked included.
//!
//! The planted skill's name, [`STRAY`], is checked absent by that name: no scratch folder's
//! name holds it, so it cannot come back through an absolute path.

use std::path::Path;

use clauseo_profile::skills::builtin::BUILTIN;
use clauseo_profile::skills::manifest::MANIFEST;
use clauseo_profile::test_support::tree_text;
use clauseo_profile::{ClauseoProfile, match_plugin, write_skills};
use clauseo_protocol::AgentKind;

use super::{Scratch, profile_paths, read, write};

/// The planted skill's name and the marker in each planted file.
const STRAY: &str = "zz-stray-skill";

/// Plants files the profile never ships in the plugin folder `plugin`: a new skill, a file in a
/// shipped skill's folder, a file beside the plugin manifest and a folder of hooks; and the new
/// skill's entry in the skills folder's manifest.
#[cfg(test)]
fn plant(plugin: &Path) {
    let skills = plugin.join("skills");
    for file in [
        skills.join(STRAY).join("SKILL.md"),
        skills.join(BUILTIN[0]).join("extra.md"),
        plugin.join(".claude-plugin").join("hooks.json"),
        plugin.join("hooks").join("hooks.json"),
    ] {
        let text = format!("---\nname: {STRAY}\ndescription: {STRAY}\n---\nx\n");
        write::write(&file, text.as_bytes()).unwrap();
    }
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&read(&skills.join(MANIFEST))).unwrap();
    manifest["skills"][STRAY] = serde_json::json!({"version": "1", "files": {}});
    let text = serde_json::to_vec(&manifest).unwrap();
    write::write(&skills.join(MANIFEST), &text).unwrap();
}

/// A new skill planted in the built-in plugin, with the rest, is gone after the launch's match,
/// before the next session; the shipped files are as written (the manifest, which named it,
/// written again); and neither Codex's index nor its keys name it. A second match finds nothing.
#[test]
fn a_planted_skill_in_the_built_in_plugin_is_removed_and_never_listed() {
    let root = Scratch::new("plugin-match-stray");
    let paths = profile_paths(&root);
    let (profile, _) = ClauseoProfile::new(paths.clone(), None).with_builtin_skills(None);
    let writes = profile.launch_writes(Some(AgentKind::Codex));
    let written = write_skills(&writes[0]).unwrap();
    let shipped = tree_text(paths.claude_plugin());
    plant(paths.claude_plugin());
    assert_eq!(match_plugin(&writes[0]).unwrap(), 5);
    assert_eq!(tree_text(paths.claude_plugin()), shipped);
    let skills = paths.claude_plugin().join("skills");
    assert!(!skills.join(STRAY).exists());
    assert!(!paths.claude_plugin().join("hooks").exists());
    assert_eq!(match_plugin(&writes[0]).unwrap(), 0);
    let keys = profile.codex_keys_with_packs(Some(AgentKind::Codex), Some(&written), None);
    let keys = serde_json::to_string(&keys).unwrap();
    assert!(!keys.contains(STRAY), "{keys}");
    assert!(!profile.skills_index().contains(STRAY));
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
    assert_eq!(match_plugin(write).unwrap(), 5);
    write_skills(write).unwrap();
    assert_eq!(match_plugin(write).unwrap(), 0);
    assert_eq!(tree_text(paths.claude_plugin()), shipped);
}

/// A shipped file with other bytes, one marked read-only with other bytes, and one hard-linked
/// to a file outside with the same bytes are each written again; the outside file keeps its
/// bytes, and the matched folder is what a fresh write makes.
#[test]
fn tampered_read_only_and_linked_shipped_files_are_written_again() {
    let root = Scratch::new("plugin-match-tampered");
    let paths = profile_paths(&root);
    let (profile, _) = ClauseoProfile::new(paths.clone(), None).with_builtin_skills(None);
    let write = &profile.launch_writes(Some(AgentKind::Claude))[0];
    write_skills(write).unwrap();
    let shipped = tree_text(paths.claude_plugin());
    let skills = paths.claude_plugin().join("skills");
    let tampered = skills.join(BUILTIN[0]).join("SKILL.md");
    write::write(&tampered, STRAY.as_bytes()).unwrap();
    let read_only = skills.join(BUILTIN[1]).join("SKILL.md");
    write::write(&read_only, STRAY.as_bytes()).unwrap();
    write::set_readonly(&read_only).unwrap();
    let linked = skills.join(BUILTIN[2]).join("SKILL.md");
    let outside = root.join("outside.md");
    write::write(&outside, &read(&linked)).unwrap();
    write::remove_file(&linked).unwrap();
    write::hard_link(&outside, &linked).unwrap();
    let outside_bytes = read(&outside);
    assert_eq!(match_plugin(write).unwrap(), 3);
    assert_eq!(tree_text(paths.claude_plugin()), shipped);
    assert_eq!(read(&outside), outside_bytes);
    write::write(&linked, STRAY.as_bytes()).unwrap();
    assert_eq!(
        read(&outside),
        outside_bytes,
        "the link to the outside file is gone"
    );
    assert_eq!(match_plugin(write).unwrap(), 1);
    assert_eq!(match_plugin(write).unwrap(), 0);
}

/// A link in the plugin folder (a symlink on Unix, a junction on Windows) is removed itself,
/// never followed: what it points at stays.
#[cfg(any(unix, windows))]
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

/// The plugin folder, or its parent, swapped for a link to a folder elsewhere (a symlink on
/// Unix, a junction on Windows): the link goes, a real folder is made in its place and written,
/// and every file of the folder elsewhere stays, a file named like a shipped one included.
#[cfg(any(unix, windows))]
#[test]
fn a_linked_plugin_folder_is_replaced_and_its_target_kept() {
    let root = Scratch::new("plugin-match-linked-folder");
    let paths = profile_paths(&root);
    let (profile, _) = ClauseoProfile::new(paths.clone(), None).with_builtin_skills(None);
    let write = &profile.launch_writes(Some(AgentKind::Claude))[0];
    write_skills(write).unwrap();
    let shipped = tree_text(paths.claude_plugin());
    let plugin = paths.claude_plugin();
    let parent = plugin.parent().unwrap();
    for linked in [plugin, parent] {
        let elsewhere = root.join("elsewhere");
        write::fresh_dir(&elsewhere).unwrap();
        let kept = [
            elsewhere.join("notes.txt"),
            elsewhere.join(".claude-plugin").join("plugin.json"),
            elsewhere.join("skills").join(MANIFEST),
        ];
        for file in &kept {
            write::write(file, STRAY.as_bytes()).unwrap();
        }
        write::remove_dir_all(linked).unwrap();
        write::symlink_dir(&elsewhere, linked).unwrap();
        match_plugin(write).unwrap();
        let meta = std::fs::symlink_metadata(linked).unwrap();
        assert!(meta.is_dir() && !meta.file_type().is_symlink());
        assert_eq!(tree_text(plugin), shipped);
        for file in &kept {
            assert_eq!(read(file), STRAY.as_bytes(), "{}", file.display());
        }
        // The write alone checks the folders too.
        write::remove_dir_all(linked).unwrap();
        write::symlink_dir(&elsewhere, linked).unwrap();
        write_skills(write).unwrap();
        assert!(
            !std::fs::symlink_metadata(linked)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        for file in &kept {
            assert_eq!(read(file), STRAY.as_bytes(), "{}", file.display());
        }
    }
}
