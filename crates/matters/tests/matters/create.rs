//! Creating and importing a matter: `matter.json`, the git dir's layout, cancel, the marker and
//! the import summary.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_matters::{
    DEFAULT_MARKER, Error, Imported, Limits, Marker, Matter, NewMatter, TimelineEntry,
};
use clauseo_protocol::{CheckpointKind, ErrorCode};

use crate::fixture::{Fixture, at, cid, matter_id, meta, tools};

/// The names directly inside `dir`, sorted.
fn names(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(ToString::to_string).collect()
}

#[test]
fn create_writes_matter_json_and_root_checkpoint() {
    let fx = Fixture::new("create-root");
    let matter = Matter::create(fx.settings(), fx.new_matter()).unwrap();

    let bytes = fx.read(".clauseo/matter.json");
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["matter_id"], matter_id(1).to_string());
    assert_eq!(json["created_at"], at(0).as_millis());
    assert_eq!(
        matter.timeline(),
        [TimelineEntry {
            id: cid(1),
            kind: CheckpointKind::Import,
            thread: None,
            turn: None,
        }]
    );
    assert_eq!(matter.last_checkpoint(), Some(cid(1)));
    assert_eq!(matter.id(), matter_id(1));
    assert_eq!(matter.folder(), fx.folder);
    assert!(fx.git_dir().is_dir());
    assert_eq!(names(&fx.folder), set(&[".clauseo"]));
    assert_eq!(names(&fx.folder.join(".clauseo")), set(&["matter.json"]));

    // The folder exists now: creating there again (as another matter) is refused.
    let again = NewMatter {
        id: matter_id(2),
        ..fx.new_matter()
    };
    let err = Matter::create(fx.settings(), again).unwrap_err();
    assert!(matches!(err, Error::FolderExists { .. }), "{err:?}");
    assert_eq!(err.code(), ErrorCode::CommandInvalid);
    assert!(!fx.root().join(matter_id(2).to_string()).exists());
}

#[test]
fn import_cancel_leaves_nothing() {
    let fx = Fixture::new("import-cancel");
    for name in ["a.txt", "b.txt", "c/d.txt"] {
        fx.write(name, name.repeat(100));
    }
    let cancel = AtomicBool::new(false);
    let mut calls = 0_u32;
    let imported = Matter::import(
        fx.settings(),
        fx.new_matter(),
        &[],
        &mut |progress| {
            calls += 1;
            if progress.files >= 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    )
    .unwrap();

    assert!(matches!(imported, Imported::Cancelled), "{imported:?}");
    assert_eq!(calls, 1, "the import went on after the cancel flag");
    assert!(!fx.home().exists());
    assert!(!fx.exists(".clauseo"));
    assert_eq!(names(&fx.folder), set(&["a.txt", "b.txt", "c"]));
    for name in ["a.txt", "b.txt", "c/d.txt"] {
        assert_eq!(fx.read(name), name.repeat(100).as_bytes());
    }
}

#[test]
fn marker_none_leaves_no_git() {
    assert_eq!(DEFAULT_MARKER, Marker::None);
    let fx = Fixture::new("marker");
    let created = Fixture::new("marker-created");
    Matter::create(created.settings(), created.new_matter()).unwrap();
    assert!(!created.exists(".git"));

    fx.write("a.txt", "a");
    let mut matter = fx.import();
    assert!(!fx.exists(".git"));
    matter.before_turn(&meta(2)).unwrap();
    assert!(!fx.exists(".git"));

    // The empty-repository marker is refused by both, leaving nothing behind.
    let refused = Fixture::new("marker-refused");
    let new = NewMatter {
        marker: Marker::EmptyRepository,
        ..refused.new_matter()
    };
    let err = Matter::create(refused.settings(), new.clone()).unwrap_err();
    assert!(matches!(err, Error::MarkerNotBuilt), "{err:?}");
    assert_eq!(err.code(), ErrorCode::CommandInvalid);
    assert!(!refused.folder.exists());
    assert!(!refused.root().exists());

    refused.write("a.txt", "a");
    let cancel = AtomicBool::new(false);
    let err = Matter::import(refused.settings(), new, &[], &mut |_| {}, &cancel).unwrap_err();
    assert!(matches!(err, Error::MarkerNotBuilt), "{err:?}");
    assert_eq!(err.code(), ErrorCode::CommandInvalid);
    assert_eq!(names(&refused.folder), set(&["a.txt"]));
    assert!(!refused.home().exists());
}

#[test]
fn import_summary_counts_files_bytes_cloud_only() {
    let mut fx = Fixture::new("import-summary");
    fx.limits = Limits {
        file_cap_bytes: 1000,
        import_warn_bytes: 10_000,
    };
    fx.write("a.txt", [1; 10]);
    fx.write("sub/b.docx", [2; 200]);
    fx.write("sub/deeper/c.pdf", [3; 1000]);
    fx.write("cloud.docx", [4; 300]);
    fx.write("cloud2.docx", [4; 30]);
    fx.write("big.bin", [5; 1001]);
    fx.write("~$b.docx", [6; 50]);
    fx.write(".DS_Store", [6; 50]);
    fx.probe.mark(fx.file("cloud.docx"));
    fx.probe.mark(fx.file("cloud2.docx"));

    let (_matter, summary) = fx.import_with_summary();
    assert_eq!(summary.files, 3);
    assert_eq!(summary.bytes.get(), 1210);
    assert_eq!(summary.cloud_only, 2);
}

#[test]
fn history_git_holds_only_head_config_index_objects_refs() {
    let fx = Fixture::new("git-layout");
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    matter.after_turn(&meta(3), &tools(&["a.txt"])).unwrap();

    let git = fx.git_dir();
    assert_eq!(names(&fx.home()), set(&["history.git"]));
    assert_eq!(
        names(&git),
        set(&["HEAD", "config", "index", "objects", "refs"])
    );
    assert_eq!(
        std::fs::read_to_string(git.join("HEAD")).unwrap(),
        "ref: refs/heads/clauseo\n"
    );
    assert_eq!(names(&git.join("refs")), set(&["clauseo"]));
    assert_eq!(names(&git.join("refs/clauseo")), set(&["timeline"]));
    assert!(git.join("refs/clauseo/timeline").is_file());
    // Loose objects only: two-hex-digit folders, no packs or info.
    for name in names(&git.join("objects")) {
        assert!(
            name.len() == 2 && name.chars().all(|c| c.is_ascii_hexdigit()),
            "objects/{name}"
        );
    }
}

/// Windows: whether `path` has `FILE_ATTRIBUTE_HIDDEN`.
#[cfg(windows)]
fn hidden(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    std::fs::symlink_metadata(path).unwrap().file_attributes() & 0x2 != 0
}

#[cfg(windows)]
#[test]
fn clauseo_hidden_after_create_and_import() {
    let fx = Fixture::new("hidden-create");
    drop(Matter::create(fx.settings(), fx.new_matter()).unwrap());
    assert!(hidden(&fx.file(".clauseo")));

    let imported = fx.dir.path().join("imported");
    crate::support::write(imported.join("a.txt"), "a");
    let new = NewMatter {
        id: matter_id(2),
        folder: imported.clone(),
        ..fx.new_matter()
    };
    let cancel = AtomicBool::new(false);
    drop(Matter::import(fx.settings(), new, &[], &mut |_| {}, &cancel).unwrap());
    assert!(hidden(&imported.join(".clauseo")));
}

/// A zip or a sync can drop the attribute: opening the matter sets it again.
#[cfg(windows)]
#[test]
fn clauseo_hidden_again_on_open() {
    let fx = Fixture::new("hidden-open");
    fx.write("a.txt", "a");
    drop(fx.import());
    let marker = fx.read(".clauseo/matter.json");
    crate::support::remove_dir_all(fx.file(".clauseo"));
    fx.write(".clauseo/matter.json", &marker);
    assert!(!hidden(&fx.file(".clauseo")));

    drop(fx.open(&fx.folder).unwrap());
    assert!(hidden(&fx.file(".clauseo")));
}
