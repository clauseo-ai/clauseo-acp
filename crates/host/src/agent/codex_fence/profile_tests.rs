//! Clauseo's built-in plugin folder (`<data>/profile`) under Codex: readable, never writable,
//! on every system.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::tests::{access, data, fence, fence_with, listed, notes, run_root};

/// A file of the built-in plugin a built-in skill sends the model to read.
fn plugin_file() -> PathBuf {
    data()
        .join("profile")
        .join("claude-plugin")
        .join("skills")
        .join("legal-drafting")
        .join("references")
        .join("section-checklists.md")
}

/// macOS and Linux: the plugin folder is under no `write` entry and no writable root, so a
/// Codex shell, and `apply_patch` (which Codex runs in the same sandbox), reads it and never
/// writes it, in the profile as loaded and in every rebuilt turn.
#[test]
fn codex_reads_the_built_in_plugin_and_cannot_write_it() {
    for keys in [fence(), fence_with(&listed())] {
        for path in [
            data().join("profile"),
            data().join("profile").join("stetdoc-seen"),
            plugin_file(),
        ] {
            assert_eq!(access(&keys, &path), "read", "{}", path.display());
        }
    }
}

/// Windows, where Codex gets no fence: its sandbox writes only the matter folder, its writable
/// roots (`sandbox_workspace_write`: the work folder and the notes; `additionalDirectories`:
/// the run folder and the notes) and its `TEMP` and `TMP` (codex-rs
/// `windows-sandbox-rs/src/allow.rs`, `compute_allow_paths`). None of them holds the plugin.
#[test]
fn no_writable_root_codex_gets_on_windows_holds_the_built_in_plugin() {
    use crate::agent::loads::browser::Switch;
    use crate::agent::loads::profile::{Folders, codex_keys};

    let work = run_root().join("work");
    let notes = notes();
    let folders = Folders::of(&work, &notes).unwrap();
    let switch = Switch {
        on: false,
        locked: false,
    };
    let launch = codex_keys(serde_json::Map::new(), switch, folders).unwrap();
    let Some(Value::Array(writable)) = launch["sandbox_workspace_write"].get("writable_roots")
    else {
        panic!("the launch names Codex's writable roots: {launch:?}")
    };
    assert_eq!(writable, &vec![json!(work), json!(notes)]);
    let mut roots: Vec<PathBuf> = writable
        .iter()
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .collect();
    roots.extend([
        PathBuf::from(r"C:\Users\lawyer\Matters\Smith v Jones"),
        run_root(),
        notes.clone(),
    ]);
    let temp = crate::agent::temp::session_env(&run_root(), None, true);
    assert_eq!(temp.len(), 2, "TEMP and TMP: {temp:?}");
    roots.extend(temp.values().map(PathBuf::from));
    for root in &roots {
        for path in [data().join("profile"), plugin_file()] {
            assert!(
                !path.starts_with(root),
                "{} under {}",
                path.display(),
                root.display()
            );
        }
    }
}
