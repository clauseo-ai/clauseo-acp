use crate::cancel::Cancel;
use crate::download::FetchKind;
use crate::download::part_path;
use crate::download::tests::{expected, fixture, job};
use crate::testing::{TempDir, TestRegistry, write_file};

use super::{BundleEntry, bundle_file, bundle_path, copy_from_bundle};

#[test]
fn a_bundle_hit_makes_no_request_and_a_miss_or_mismatch_fails() {
    let registry = TestRegistry::start().unwrap();
    let dir = TempDir::new("bundle").unwrap();
    let bundle = dir.path().join("bundle");
    let bytes = fixture();
    let want = expected(&bytes);
    let at = bundle.join("npm").join(format!("{}.tgz", want.hex()));
    write_file(&at, &bytes).unwrap();
    let found = bundle_file(&bundle, &want, "pkg-1.0.0.tgz", dir.path());
    assert_eq!(found.unwrap(), at);
    assert_eq!(registry.requests(), Vec::new());

    let missing = expected(b"another package");
    let err = bundle_file(&bundle, &missing, "other-2.0.0.tgz", dir.path()).unwrap_err();
    assert_eq!(err.kind, FetchKind::Failed);
    assert!(err.detail.contains("other-2.0.0.tgz"), "{}", err.detail);
    assert!(!err.detail.contains(&*dir.path().to_string_lossy()));

    let err = bundle_path(&bundle, BundleEntry::Node("../x"), &want, "x", dir.path());
    assert_eq!(err.unwrap_err().kind, FetchKind::Failed);
}

#[test]
fn the_bundle_s_node_archive_is_copied_as_it_is_hashed_and_a_mismatch_leaves_nothing() {
    let dir = TempDir::new("bundle-node").unwrap();
    let bundle = dir.path().join("bundle");
    let bytes = fixture();
    let name = "node-v24.22.0-linux-x64.tar.gz";
    let from = bundle.join("node").join(name);
    write_file(&from, &bytes).unwrap();
    let found = bundle_path(
        &bundle,
        BundleEntry::Node(name),
        &expected(&bytes),
        name,
        dir.path(),
    );
    assert_eq!(found.unwrap(), from);
    let copy = job("", &bytes, dir.path().join("downloads").join(name));
    let mut seen = Vec::new();
    let size = copy_from_bundle(
        &from,
        &copy,
        &Cancel::new(),
        &mut |r, t| seen.push((r, t)),
        dir.path(),
    )
    .unwrap();
    assert_eq!(size, bytes.len() as u64);
    assert_eq!(std::fs::read(&copy.dest).unwrap(), bytes);
    assert_eq!(seen.last(), Some(&(size, copy.max_bytes)));

    // A bundle file with other bytes: a checksum failure, and no copy or part left.
    write_file(&from, &fixture()[1..]).unwrap();
    crate::write::remove_file(&copy.dest).unwrap();
    let err =
        copy_from_bundle(&from, &copy, &Cancel::new(), &mut |_, _| {}, dir.path()).unwrap_err();
    assert_eq!(err.kind, FetchKind::Failed, "{err:?}");
    assert!(!copy.dest.exists() && !part_path(&copy.dest).exists());
    // Over the size cap: refused as it is written.
    write_file(&from, &bytes).unwrap();
    let mut small = copy.clone();
    small.max_bytes = 1000;
    let err =
        copy_from_bundle(&from, &small, &Cancel::new(), &mut |_, _| {}, dir.path()).unwrap_err();
    assert!(err.detail.contains("ran past"), "{}", err.detail);
    assert!(!part_path(&copy.dest).exists());
}
