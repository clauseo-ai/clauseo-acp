use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;

use super::{overlaps_tree, refuse_data_folder};
use crate::write;

/// A fresh data folder under the temp folder for `test`, with a sibling agent's home made.
fn data(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("clauseo-data-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root.join("data/agents/claude-acp/home/rules")).unwrap();
    write::create_dir_all(&root.join("data/work/m")).unwrap();
    write::create_dir_all(&root.join("matter")).unwrap();
    root
}

fn refused(folder: &Path, data: &Path) -> bool {
    match refuse_data_folder(folder, data) {
        Ok(()) => false,
        Err(error) => {
            assert_eq!(error.code(), ErrorCode::MatterFolderRefused);
            assert!(
                !error.to_string().contains(&*data.to_string_lossy()),
                "{error}"
            );
            true
        }
    }
}

/// The data folder, a folder holding it, its `agents` folder and any agent's home are refused;
/// a folder beside the data folder is not, nor (the named gap) one elsewhere inside it.
#[test]
fn a_matter_folder_in_the_data_folders_agents_or_holding_it_is_refused() {
    let root = data("refuse");
    let data = root.join("data");
    let home = data.join("agents/claude-acp/home");
    for folder in [
        root.clone(),
        data.clone(),
        data.join("agents"),
        data.join("agents/claude-acp"),
        home.clone(),
        home.join("rules"),
    ] {
        assert!(refused(&folder, &data), "{}", folder.display());
    }
    assert!(!refused(&root.join("matter"), &data));
    assert!(!refused(&data.join("work/m"), &data));
    // A name that only starts like `agents` is not inside it.
    assert!(!refused(&data.join("agents2"), &data));
    write::remove_dir_all(&root).unwrap();
}

/// A spelling that differs only in case is the same folder on macOS and Windows.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn a_case_folded_spelling_is_caught() {
    let root = data("case");
    let data = root.join("data");
    let shouted = root
        .join("DATA")
        .join("AGENTS")
        .join("Claude-ACP")
        .join("home");
    assert!(refused(&shouted, &data));
    assert!(refused(&root.join("Data"), &data));
    assert!(overlaps_tree(&root.join("Data"), &data));
    write::remove_dir_all(&root).unwrap();
}

/// A matter folder that is a link to the data folder, or to an agent's home, is caught.
#[cfg(unix)]
#[test]
fn a_linked_spelling_is_caught() {
    let root = data("link");
    let data = root.join("data");
    let to_data = root.join("to-data");
    write::symlink(&data, &to_data).unwrap();
    let to_home = root.join("to-home");
    write::symlink(&data.join("agents/claude-acp/home"), &to_home).unwrap();
    assert!(refused(&to_data, &data));
    assert!(refused(&to_home, &data));
    assert!(overlaps_tree(&to_home, &data));
    write::remove_dir_all(&root).unwrap();
}
