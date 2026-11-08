//! A model of the NTFS access check, enough for the reads of other matters (R18): the DACL a
//! path has once every walk has run, worked out from the explicit entries the fake holds, and
//! a check of it for a token's SIDs.
//!
//! NTFS's canonical order: a path's own explicit entries, its denies first, then each
//! ancestor's entries that reach it, nearest first, each ancestor's denies before its allows
//! (an inherited copy keeps its parent's order). The first entry to decide a bit decides it.
//! Not modelled: protected DACLs, owner rights, the integrity label, the restricted token's
//! second pass (the same SIDs here).

use std::collections::BTreeSet;
use std::path::Path;

use super::Key;
use crate::windows::acl_plan::{Inherit, Rights};
use crate::windows::test_world::{GROUP_SID, USER_SID};

/// `BUILTIN\Users`, in every pool user's restricting set.
pub(in crate::windows::apply::tests) const USERS: &str = "S-1-5-32-545";

/// `FILE_GENERIC_READ`: read data (or list a folder), attributes, extended attributes and the
/// permissions.
pub(in crate::windows::apply::tests) const READ: u32 = 0x0012_0089;

/// `FILE_WRITE_DATA` (or add a file to a folder).
pub(in crate::windows::apply::tests) const WRITE: u32 = 0x2;

/// `FILE_APPEND_DATA` (or add a folder to a folder).
pub(in crate::windows::apply::tests) const APPEND: u32 = 0x4;

/// Matter one's pool user's token: the pool user, its group, `Users`.
pub(in crate::windows::apply::tests) const POOL_TOKEN: [&str; 3] = [USER_SID, GROUP_SID, USERS];

/// What a volume's root gives `Users` and every folder below inherits: read and write, as
/// `C:\` gives, the reason for the fence.
pub(in crate::windows::apply::tests) fn volume_default(root: &Path) -> Key {
    (
        root.to_owned(),
        USERS.to_owned(),
        Rights::Modify,
        Inherit::Tree,
    )
}

/// Whether an entry with `inherit`, `depth` levels above a path (0: on it), reaches it.
fn reaches(inherit: Inherit, depth: usize, folder: bool) -> bool {
    match inherit {
        Inherit::ThisOnly => depth == 0,
        Inherit::Tree => true,
        Inherit::Below => depth > 0,
        Inherit::FilesInFolder => depth == 1 && !folder,
    }
}

/// The DACL `path` has once the walks have run, in canonical order.
fn dacl<'k>(aces: &'k BTreeSet<Key>, path: &Path) -> Vec<&'k Key> {
    let folder = path.is_dir();
    let mut out = Vec::new();
    for (depth, at) in path.ancestors().enumerate() {
        let mut here: Vec<&Key> = aces
            .iter()
            .filter(|(p, _, rights, inherit)| {
                p == at && !rights.is_label() && reaches(*inherit, depth, folder)
            })
            .collect();
        here.sort_by_key(|(_, _, rights, _)| !rights.is_deny());
        out.extend(here);
    }
    out
}

/// Whether a token holding `sids` is granted all of `want` on `path`.
pub(in crate::windows::apply::tests) fn granted(
    aces: &BTreeSet<Key>,
    path: &Path,
    sids: &[&str],
    want: u32,
) -> bool {
    let (mut allowed, mut denied) = (0u32, 0u32);
    for (_, sid, rights, _) in dacl(aces, path) {
        if !sids.contains(&sid.as_str()) {
            continue;
        }
        let bits = rights.mask() & want;
        if rights.is_deny() {
            denied |= bits & !allowed;
        } else {
            allowed |= bits & !denied;
        }
    }
    allowed & want == want
}

#[test]
fn the_model_puts_an_explicit_allow_on_a_child_before_an_inherited_deny_and_not_its_own_deny() {
    let dir = crate::test_support::TempDir::new("access-model");
    let child = dir.file("root/child.txt", "c");
    let root = child.parent().unwrap().to_owned();
    let deny = (
        root.clone(),
        USER_SID.to_owned(),
        Rights::DenyRead,
        Inherit::Tree,
    );
    let allow = |p: &Path, inherit| {
        let sid = USER_SID.to_owned();
        (p.to_owned(), sid, Rights::ReadAndExecute, inherit)
    };
    let mut aces: BTreeSet<Key> = [deny, allow(&child, Inherit::ThisOnly)].into();
    assert!(granted(&aces, &child, &POOL_TOKEN, READ));
    aces.insert(allow(&root, Inherit::ThisOnly));
    assert!(!granted(&aces, &root, &POOL_TOKEN, READ));
}
