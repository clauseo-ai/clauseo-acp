//! Windows only: an 8.3 short name in a path lands in the same fenced folder as its long name
//! (`GetFinalPathNameByHandleW` through `dunce::canonicalize`). Skipped on a volume with short
//! names off.

use std::path::Path;

use serde_json::json;

use super::decide::{Denial, Fence, decide};
use super::real::Ground;
use super::{FenceFile, FencedRoot, RootKind};
use crate::network_paths::resolve::Disk;
use crate::network_paths::{NetEnv, short_path};
use crate::pipeline::tests::support::TempDir;
use crate::write;

#[test]
fn a_short_name_lands_in_the_fenced_folder() {
    let dir = TempDir::new("fence-short");
    let fenced = dir
        .path()
        .join("Clauseo Matters Long Name")
        .join("Matter B Long Name");
    let own = dir.path().join("Matter A");
    write::create_dir_all(&fenced).unwrap();
    write::create_dir_all(&own).unwrap();
    write::write_file(&fenced.join("lease.pdf"), b"lease").unwrap();
    let Some(short) = short_path(&fenced) else {
        eprintln!("skipped: no short name could be read");
        return;
    };
    let long_names = |path: &Path| path.to_string_lossy().contains("Long Name");
    if long_names(&short) {
        eprintln!("skipped: this volume makes no 8.3 names");
        return;
    }
    let net = NetEnv::current();
    let root = FencedRoot::at(
        RootKind::Matter,
        &[fenced.as_path()],
        (vec!["**".into()], vec![]),
        &net,
    );
    let file = FenceFile {
        version: 1,
        roots: root.into_iter().collect(),
    };
    let fence = Fence::on_disk(&file, true);
    let disk = Disk::real(&net);
    let ground = Ground {
        cwd: &own,
        home: None,
        shares: std::slice::from_ref(&own),
        disk: &disk,
    };
    let lease = short.join("lease.pdf");
    let read = json!({"file_path": lease});
    let hit = Some(Denial::Fenced(RootKind::Matter));
    assert_eq!(
        decide(&fence, "Read", &read, &ground),
        hit,
        "{}",
        lease.display()
    );
    let new = json!({"file_path": short.join("Drafts").join("new.docx")});
    assert_eq!(decide(&fence, "Write", &new, &ground), hit);
    let upper = json!({"file_path": lease.to_string_lossy().to_uppercase()});
    assert_eq!(decide(&fence, "Read", &upper, &ground), hit);
    let grep = json!({"pattern": "x", "path": short});
    assert_eq!(
        decide(&fence, "Grep", &grep, &ground),
        Some(Denial::Holds(RootKind::Matter))
    );
    let above = json!({"pattern": "x", "path": short.parent().unwrap()});
    assert_eq!(
        decide(&fence, "Grep", &above, &ground),
        Some(Denial::Holds(RootKind::Matter))
    );
    let own_file = json!({"file_path": own.join("brief.docx")});
    assert_eq!(decide(&fence, "Read", &own_file, &ground), None);
}
