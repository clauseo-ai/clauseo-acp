//! Tests' helpers for the permission writes on NTFS: a canonical temp folder, grants, and the
//! entries a path holds. For this crate's Windows tests only.

use std::path::{Path, PathBuf};

use super::acl::descriptor;
use super::acl_bytes::{Acl, Descriptor, INHERITED, flags, kind, mask, sid, sid_text};
use super::acl_plan::{Grant, Inherit, Rights, Sid};
use super::admit::PathKind;
use crate::test_support::TempDir;

/// A temp folder, canonical (the runner's `TEMP` may be an 8.3 spelling).
pub(crate) struct Place {
    pub(crate) dir: TempDir,
    pub(crate) root: PathBuf,
}

impl Place {
    pub(crate) fn new(test: &str) -> Place {
        let dir = TempDir::new(test);
        let root = dunce::canonicalize(dir.path()).unwrap();
        Place { dir, root }
    }

    pub(crate) fn at(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

/// A grant on `path` for `sid`, of the kind of the matter folder.
pub(crate) fn grant(path: &Path, sid: &str, rights: Rights, inherit: Inherit) -> Grant {
    Grant {
        path: path.to_owned(),
        sid: Sid::parse(sid).unwrap(),
        rights,
        inherit,
        kind: PathKind::Matter,
    }
}

/// Every entry on `path`'s DACL and label, as (type, flags, mask, SID).
pub(crate) fn entries(path: &Path) -> Vec<(u8, u8, u32, Sid)> {
    let bytes = descriptor(path).unwrap();
    let read = Descriptor::parse(&bytes).unwrap();
    let mut out = Vec::new();
    for acl in [read.dacl, read.sacl].into_iter().flatten() {
        for ace in Acl::parse(acl).unwrap().aces() {
            if let (Some(m), Some(s)) = (mask(ace), sid(ace).and_then(sid_text)) {
                out.push((kind(ace), flags(ace), m, s));
            }
        }
    }
    out
}

/// Whether `path` has an entry for `sid`, inherited or explicit as asked.
pub(crate) fn has(path: &Path, sid: &str, inherited: bool) -> bool {
    entries(path)
        .iter()
        .any(|(_, f, _, s)| s.as_str() == sid && (f & INHERITED != 0) == inherited)
}

/// Whether `path`'s DACL is protected.
pub(crate) fn protected(path: &Path) -> bool {
    let bytes = descriptor(path).unwrap();
    Descriptor::parse(&bytes).unwrap().dacl_protected()
}
