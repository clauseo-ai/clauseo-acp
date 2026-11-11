//! A blackline side's copy from the folder now (`folder_side`) and the cap on a version's
//! (`Capped`): a cloud-only placeholder and a file past the size cap are refused as unknown,
//! with nothing read; a file within the cap is copied whole.

use std::fs::Metadata;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use clauseo_matters::PlaceholderProbe;
use clauseo_protocol::{ErrorCode, MatterPath};

use super::{Capped, folder_side};
use crate::write;

/// A probe that calls every file cloud-only, or none.
struct Probe(bool);

impl PlaceholderProbe for Probe {
    fn is_cloud_only(&self, _path: &Path, _metadata: &Metadata) -> bool {
        self.0
    }
}

/// `<temp>/clauseo-host-unit-sides-<name>-<pid>`, emptied, holding `a.docx` with `bytes`.
fn folder_with(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "clauseo-host-unit-sides-{name}-{}",
        std::process::id()
    ));
    write::remove_dir_all(&dir).unwrap();
    write::create_dir_all(&dir).unwrap();
    write::create_new_file(&dir.join("a.docx"))
        .unwrap()
        .write_all(bytes)
        .unwrap();
    dir
}

fn code(error: &crate::error::Error) -> ErrorCode {
    error.code()
}

#[test]
fn a_cloud_only_file_in_the_folder_is_refused_as_unknown_and_not_copied() {
    let dir = folder_with("cloud", b"placeholder");
    let path = MatterPath::new("a.docx").unwrap();
    let refused = folder_side(
        &Probe(true),
        1024,
        &dir.join("a.docx"),
        &path,
        &dir.join("new.docx"),
    )
    .unwrap_err();
    assert_eq!(code(&refused), ErrorCode::CommandInvalid, "{refused:?}");
    assert!(!dir.join("new.docx").exists());
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_folder_file_past_the_size_cap_is_refused_as_unknown_and_not_copied() {
    let dir = folder_with("overcap", b"0123456789");
    let path = MatterPath::new("a.docx").unwrap();
    let refused = folder_side(
        &Probe(false),
        9,
        &dir.join("a.docx"),
        &path,
        &dir.join("new.docx"),
    )
    .unwrap_err();
    assert_eq!(code(&refused), ErrorCode::CommandInvalid, "{refused:?}");
    assert!(!dir.join("new.docx").exists());
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_folder_file_within_the_cap_is_copied_whole() {
    let dir = folder_with("withincap", b"0123456789");
    let path = MatterPath::new("a.docx").unwrap();
    folder_side(
        &Probe(false),
        10,
        &dir.join("a.docx"),
        &path,
        &dir.join("new.docx"),
    )
    .unwrap();
    assert_eq!(std::fs::read(dir.join("new.docx")).unwrap(), b"0123456789");
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_version_side_past_the_cap_fails_its_write_and_says_so() {
    let mut out = Capped::new(Vec::new(), 4);
    out.write_all(b"abcd").unwrap();
    assert!(!out.over);
    assert!(out.write_all(b"e").is_err());
    assert!(out.over);
    assert_eq!(out.inner, b"abcd");
}
