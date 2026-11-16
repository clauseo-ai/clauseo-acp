//! `chromium_policy::SystemTree` on a real disk: a file where a policy folder should be is
//! nothing set, as Chrome skips it, and the reads keep to their caps.

use clauseo_settings::chromium_policy::{PolicyTree as _, SystemTree};
use clauseo_settings::{Files as _, SystemFiles};
use std::path::PathBuf;

/// A folder of this test's own under `CARGO_TARGET_TMPDIR`.
fn root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("settings-rdpol-{name}-{}", std::process::id()))
}

// A file where a folder on the path should be (`ENOTDIR`): nothing set, for a Linux policy
// folder and for a macOS user's managed preferences folder alike.
#[cfg(unix)]
#[test]
fn a_file_where_a_policy_folder_should_be_is_nothing_set() {
    let root = root("notdir");
    let file = root.join("managed");
    SystemFiles.write_atomic(&file, b"not a folder").unwrap();
    assert_eq!(SystemTree.json_files(&file).unwrap(), Vec::<PathBuf>::new());
    assert_eq!(
        SystemTree.json_files(&file.join("deeper")).unwrap(),
        Vec::<PathBuf>::new()
    );
    // `/Library/Managed Preferences/<user>` is a file: the user's plist is missing.
    let plist = file.join("com.google.Chrome.plist");
    assert_eq!(SystemTree.read(&plist, u64::MAX).unwrap(), None);
    assert_eq!(
        SystemTree
            .read(&root.join("absent.plist"), u64::MAX)
            .unwrap(),
        None
    );
}

#[test]
fn the_system_tree_reads_one_byte_past_the_cap_and_lists_only_json_files() {
    let root = root("tree");
    SystemFiles
        .write_atomic(&root.join("a.json"), b"0123456789")
        .unwrap();
    SystemFiles
        .write_atomic(&root.join("b.txt"), b"{}")
        .unwrap();
    // A folder named like a policy file is not listed.
    SystemFiles
        .write_atomic(&root.join("c.json").join("inner"), b"{}")
        .unwrap();
    assert_eq!(
        SystemTree.read(&root.join("a.json"), 4).unwrap(),
        Some(b"01234".to_vec())
    );
    assert_eq!(SystemTree.json_files(&root).unwrap(), [root.join("a.json")]);
}
