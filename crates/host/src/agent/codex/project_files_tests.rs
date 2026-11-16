//! The matter folder's project files: the two home keys that stop Codex reading `AGENTS.md` and
//! walking parents ([`super::PROJECT_FILE_OVERRIDES`]), and the shared refusal of `.codex` and
//! `.agents` ([`super::refuse_project_folders`]) at launch and before each prompt.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{ErrorCode, MatterId, Timestamp};

use super::{REFUSED_ENTRIES, home_config, prepare_launch, refuse_project_folders};
use crate::write;

/// A fresh folder under the temp folder for `test`, removed first.
fn scratch(test: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("clauseo-codex-files-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

/// The home's `config.toml` parses as TOML and turns off every project doc and every parent walk.
#[test]
fn home_config_turns_off_agents_md_and_the_parent_walk() {
    let text = home_config();
    let table: toml::Table = text.parse().unwrap();
    assert_eq!(
        table.get("project_doc_max_bytes"),
        Some(&toml::Value::Integer(0)),
        "{text}"
    );
    assert_eq!(
        table.get("project_root_markers"),
        Some(&toml::Value::Array(Vec::new())),
        "{text}"
    );
}

/// No `sandbox_mode` and no `[projects]` in the home's `config.toml`: with `sandbox_mode` there,
/// Codex would write `[projects.<git root>] trust_level = "trusted"` into it.
#[test]
fn home_config_sets_no_sandbox_mode_and_trusts_no_project() {
    let text = home_config();
    let table: toml::Table = text.parse().unwrap();
    assert!(!table.contains_key("sandbox_mode"), "{text}");
    assert!(!table.contains_key("projects"), "{text}");
    assert!(!text.contains("sandbox_mode"), "{text}");
}

/// The refused entries are `.codex`, then `.agents`, then `.git`, each with its own code.
#[test]
fn the_refused_entries_are_codex_then_agents_then_git() {
    let names: Vec<&str> = REFUSED_ENTRIES.iter().map(|entry| entry.name).collect();
    assert_eq!(names, [".codex", ".agents", ".git"]);
    let codes: Vec<ErrorCode> = REFUSED_ENTRIES.iter().map(|entry| entry.code).collect();
    assert_eq!(
        codes,
        [
            ErrorCode::MatterCodexFolder,
            ErrorCode::MatterAgentsFolder,
            ErrorCode::MatterGitFolder,
        ]
    );
}

/// Each refused entry, as a folder or a file, refuses the matter folder with its code and words
/// that name it and no path; a folder holding neither passes.
#[test]
fn each_refused_entry_of_any_kind_refuses_the_folder() {
    let root = scratch("each");
    let clean = root.join("clean");
    write::create_dir_all(&clean).unwrap();
    write::write_file(&clean.join("AGENTS.md"), b"a document\n").unwrap();
    assert!(refuse_project_folders(&clean).is_ok());
    for entry in &REFUSED_ENTRIES {
        for as_file in [false, true] {
            let folder = root.join(format!("{}-{as_file}", entry.name.trim_start_matches('.')));
            write::create_dir_all(&folder).unwrap();
            if as_file {
                write::write_file(&folder.join(entry.name), b"").unwrap();
            } else {
                write::create_dir_all(&folder.join(entry.name).join("skills")).unwrap();
            }
            let error = refuse_project_folders(&folder).unwrap_err();
            assert_eq!(error.code(), entry.code, "{}", entry.name);
            let words = error.to_string();
            assert!(words.contains("then try again"), "{words}");
            assert!(words.contains(&format!("\"{}\"", entry.name)), "{words}");
            assert!(!words.contains(&*root.to_string_lossy()), "{words}");
        }
    }
    write::remove_dir_all(&root).unwrap();
}

/// A link named `.agents` counts, wherever it points (here nowhere).
#[cfg(unix)]
#[test]
fn a_link_named_agents_counts() {
    let root = scratch("link");
    let folder = root.join("m");
    write::create_dir_all(&folder).unwrap();
    write::symlink(&root.join("nowhere"), &folder.join(".agents")).unwrap();
    let error = refuse_project_folders(&folder).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterAgentsFolder);
    write::remove_dir_all(&root).unwrap();
}

/// A link named `.git` counts too, wherever it points (here nowhere).
#[cfg(unix)]
#[test]
fn a_link_named_git_counts() {
    let root = scratch("link_git");
    let folder = root.join("m");
    write::create_dir_all(&folder).unwrap();
    write::symlink(&root.join("nowhere"), &folder.join(".git")).unwrap();
    let error = refuse_project_folders(&folder).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterGitFolder);
    write::remove_dir_all(&root).unwrap();
}

/// A `.git` junction in the matter folder counts as a link does: Windows' reparse point, which
/// `symlink_metadata` finds whatever it points at (here a folder outside the matter).
#[cfg(windows)]
#[test]
fn a_junction_named_git_counts() {
    let root = scratch("junction_git");
    let folder = root.join("m");
    let target = root.join("elsewhere");
    write::create_dir_all(&folder).unwrap();
    write::create_dir_all(&target).unwrap();
    clauseo_sandbox::windows::test_junction::junction(&folder.join(".git"), &target).unwrap();
    let error = refuse_project_folders(&folder).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterGitFolder);
    write::remove_dir_all(&root).unwrap();
}

/// A folder holding both is refused for `.codex`, the first.
#[test]
fn codex_is_named_first_when_both_are_there() {
    let root = scratch("both");
    write::create_dir_all(&root.join(".codex")).unwrap();
    write::create_dir_all(&root.join(".agents")).unwrap();
    let error = refuse_project_folders(&root).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterCodexFolder);
    write::remove_dir_all(&root).unwrap();
}

/// Only the matter folder is checked: a parent's `.agents`, `.codex`, `AGENTS.md` or `.git` file
/// is for the home's `project_root_markers = []`, and refuses nothing.
#[test]
fn a_parents_project_files_refuse_nothing() {
    let root = scratch("parent");
    write::create_dir_all(&root.join(".agents/skills/p")).unwrap();
    write::create_dir_all(&root.join(".codex/skills/c")).unwrap();
    write::write_file(&root.join("AGENTS.md"), b"x\n").unwrap();
    write::write_file(&root.join(".git"), b"gitdir: /nonexistent\n").unwrap();
    let matter = root.join("matter");
    write::create_dir_all(&matter).unwrap();
    assert!(refuse_project_folders(&matter).is_ok());
    write::remove_dir_all(&root).unwrap();
}

/// A matter folder that is gone is `matter.folder_missing`, not a pass.
#[test]
fn a_missing_folder_is_folder_missing() {
    let root = scratch("missing");
    let error = refuse_project_folders(&root.join("gone")).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterFolderMissing);
    write::remove_dir_all(&root).unwrap();
}

/// The home under `data`, as `Locations::agent_home` makes it.
fn home_in(data: &Path) -> PathBuf {
    data.join("agents").join("codex-acp").join("home")
}

/// At launch: an `.agents` refuses before the home is made; once it is gone the launch goes on.
#[test]
fn prepare_launch_refuses_an_agents_folder_before_the_home() {
    let root = scratch("prepare");
    let data = root.join("data");
    write::create_dir_all(&data).unwrap();
    let matter = root.join("matter");
    write::create_dir_all(&matter.join(".agents/skills/s")).unwrap();
    let id = MatterId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0002,
    ))
    .unwrap();
    let check = crate::agent::folder_support::FolderCheck {
        matter: Some(id),
        source: crate::agent::folder_support::CheckSource::Windows,
    };
    let error = prepare_launch(&check, &matter, &data, &home_in(&data)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterAgentsFolder);
    assert!(!data.join("agents").exists());
    write::remove_dir_all(&matter.join(".agents")).unwrap();
    prepare_launch(&check, &matter, &data, &home_in(&data)).unwrap();
    let written = std::fs::read_to_string(home_in(&data).join("config.toml")).unwrap();
    assert!(written.contains("project_root_markers = []"), "{written}");
    write::remove_dir_all(&root).unwrap();
}

/// Before each prompt and session call: the re-check refuses an `.agents` that appeared since
/// the launch.
#[test]
fn the_recheck_refuses_an_agents_folder_that_appears() {
    let root = scratch("recheck");
    let clock = crate::ManualClock::new(Timestamp::new(1_790_000_000_000).unwrap());
    let check = || {
        futures::executor::block_on(super::prepare::recheck(
            Arc::new(clock.clone()),
            None,
            Some(root.clone()),
        ))
    };
    assert!(check().is_ok());
    write::create_dir_all(&root.join(".agents")).unwrap();
    assert_eq!(check().unwrap_err().code(), ErrorCode::MatterAgentsFolder);
    write::remove_dir_all(&root).unwrap();
}
