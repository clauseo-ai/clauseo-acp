//! What Codex may write, on every OS: the conversation's own work folder, the matter's notes and
//! the launch's run folder, never a folder above a work or notes folder. On Windows Codex 0.162.1's
//! sandbox gives each writable root its own SID, kept in its home, with an allow entry on the
//! folder it never takes back (`cap.rs`, `spawn_prep.rs` at `rust-v0.162.1`): only a command
//! given that root, or one above it, carries the SID. So a root of `agent-state\<matter>\work`,
//! the matter's state, `agent-state` or the data folder would let every later Codex command
//! of any conversation write every conversation's work folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Map;

use super::{folders, scratch};
use crate::agent::loads::browser::Switch;
use crate::agent::loads::profile::{Folders, codex_keys};
use crate::write;

/// The writable roots of the `CODEX_CONFIG` Codex gets, on Windows and elsewhere, and the
/// `additionalDirectories` its session call sends, name only the session's own folders.
#[test]
fn codex_is_granted_its_own_folders_and_none_above_them() {
    let data = scratch("codex-roots");
    let run = folders(&data, 1);
    let switch = Switch {
        on: false,
        locked: false,
    };
    let keys = codex_keys(
        Map::new(),
        switch,
        Folders::of(&run.work, &run.notes).unwrap(),
    )
    .unwrap();
    let mut granted: Vec<PathBuf> = Vec::new();
    for windows in [true, false] {
        let config = crate::agent::codex::config_on(&BTreeMap::new(), keys.clone(), None, windows);
        let roots: Vec<PathBuf> = config["sandbox_workspace_write"]["writable_roots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|root| PathBuf::from(root.as_str().unwrap()))
            .collect();
        assert_eq!(
            roots,
            [run.work.clone(), run.notes.clone()],
            "windows: {windows}"
        );
        let sandbox = config.get("windows").and_then(|value| value.get("sandbox"));
        assert_eq!(sandbox.is_some(), windows, "{config:?}");
        granted.extend(roots);
    }
    let directories = crate::actor::turn::profile_loads::codex_directories(&run);
    assert_eq!(
        directories,
        [run.root.clone(), run.work.clone(), run.notes.clone()]
    );
    granted.extend(directories);
    // Every folder above a work or notes folder, up to the data folder itself.
    let above = |folder: &Path| -> Vec<PathBuf> {
        let parents = folder.ancestors().skip(1);
        parents
            .take_while(|parent| parent.starts_with(&data))
            .map(Path::to_path_buf)
            .collect()
    };
    let above: Vec<PathBuf> = above(&run.work)
        .into_iter()
        .chain(above(&run.notes))
        .collect();
    assert!(above.contains(&data.join("agent-state")), "{above:?}");
    for root in &granted {
        assert!(
            !above.contains(root),
            "a grant above a work or notes folder: {root:?}"
        );
        assert!(root.starts_with(&data) && root != &data, "{root:?}");
    }
    write::remove_dir_all(&data).unwrap();
}
