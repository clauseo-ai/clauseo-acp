//! The no-follow open on Windows, on real folders (the Windows CI runs these): a junction or a
//! file symlink swapped in after the walk refuses; a local `subst` drive opens from the volume
//! it names; a folder the user may pass through but not list stays usable below it; an ordinary
//! path on the runner's temp drive (`D:\`) opens.
#![expect(
    unsafe_code,
    reason = "FFI to DefineDosDeviceW for the subst drive test and its drop guard"
)]

use std::io;
use std::path::{Path, PathBuf};

use clauseo_sandbox::windows::test_dacl::{auto_inherit_with, protect_with};
use clauseo_sandbox::windows::test_junction::junction;
use windows::Win32::Storage::FileSystem::{
    DDD_REMOVE_DEFINITION, DEFINE_DOS_DEVICE_FLAGS, DefineDosDeviceW,
};
use windows::core::{HSTRING, PCWSTR};

use super::{Access, Kind, is_link_on_the_way, open_path};
use crate::network_paths::resolve::Entry;
use crate::network_paths::windows::device_of;
use crate::write;

/// `ERROR_PRIVILEGE_NOT_HELD`: no right to make a symlink.
const PRIVILEGE_NOT_HELD: i32 = 1314;

/// A folder of its own, its path canonical (no link on it), removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        Self::under(&std::env::temp_dir(), name)
    }

    fn under(base: &Path, name: &str) -> Self {
        let root = dunce::canonicalize(base)
            .unwrap()
            .join(format!("clauseo-no-follow-{name}-{}", std::process::id()));
        drop(write::remove_dir_all(&root));
        write::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn file(&self, relative: &str) -> PathBuf {
        let path = self.0.join(relative);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, b"x").unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.0));
    }
}

/// A matter `m` with `sub\Contract.docx`, and a folder `outside` with the same name where a
/// swapped-in link points.
fn matter(scratch: &Scratch) -> PathBuf {
    scratch.file(r"outside\Contract.docx");
    scratch.file(r"m\sub\Contract.docx")
}

#[test]
fn a_junction_swapped_in_for_a_folder_after_the_walk_is_refused() {
    let scratch = Scratch::new("junction");
    let document = matter(&scratch);
    assert_eq!(
        open_path(&document, Access::Read).unwrap().kind().unwrap(),
        Kind::File
    );
    let sub = scratch.0.join(r"m\sub");
    write::rename(&sub, &scratch.0.join(r"m\was-sub")).unwrap();
    junction(&sub, &scratch.0.join("outside")).unwrap();
    for access in [Access::Look, Access::Read, Access::Exclusive] {
        let error = open_path(&document, access).unwrap_err();
        assert!(is_link_on_the_way(&error), "{access:?}: {error}");
    }
    let error = open_path(&sub, Access::List).unwrap_err();
    assert!(is_link_on_the_way(&error), "{error}");
    let parent = open_path(&scratch.0.join("m"), Access::List).unwrap();
    assert_eq!(parent.entry_at("sub".as_ref()).unwrap(), Entry::Link);
    let error = parent.open_at("sub".as_ref(), Access::Folder).unwrap_err();
    assert!(is_link_on_the_way(&error), "{error}");
}

#[test]
fn a_file_symlink_swapped_in_after_the_walk_is_refused() {
    let scratch = Scratch::new("file-link");
    let document = matter(&scratch);
    write::remove_file(&document).unwrap();
    let made = write::symlink_file(&scratch.0.join(r"outside\Contract.docx"), &document);
    if let Err(error) = &made
        && error.raw_os_error() == Some(PRIVILEGE_NOT_HELD)
    {
        eprintln!("SKIPPED a_file_symlink_swapped_in_after_the_walk_is_refused: {error}");
        return;
    }
    made.unwrap();
    for access in [Access::Look, Access::Read, Access::Exclusive] {
        let error = open_path(&document, access).unwrap_err();
        assert!(is_link_on_the_way(&error), "{access:?}: {error}");
    }
    let folder = open_path(&scratch.0.join(r"m\sub"), Access::List).unwrap();
    assert_eq!(
        folder.entry_at("Contract.docx".as_ref()).unwrap(),
        Entry::Link
    );
    let error = folder
        .open_at("Contract.docx".as_ref(), Access::Read)
        .unwrap_err();
    assert!(is_link_on_the_way(&error), "{error}");
}

/// A `subst` drive on a free letter, removed on drop.
struct Subst(char);

impl Subst {
    fn to(target: &Path) -> Self {
        let letter = ('G'..='Z')
            .rev()
            .find(|letter| device_of(*letter).is_none())
            .expect("a free drive letter");
        let name = HSTRING::from(format!("{letter}:"));
        let target = HSTRING::from(target.as_os_str());
        // SAFETY: both strings are NUL-terminated and outlive the call, which keeps no pointer.
        unsafe { DefineDosDeviceW(DEFINE_DOS_DEVICE_FLAGS(0), &name, &target) }.unwrap();
        Self(letter)
    }
}

impl Drop for Subst {
    fn drop(&mut self) {
        let name = HSTRING::from(format!("{}:", self.0));
        // SAFETY: `name` is NUL-terminated and outlives the call; a null target removes the
        // letter's latest definition, the one `Subst::to` made.
        drop(unsafe { DefineDosDeviceW(DDD_REMOVE_DEFINITION, &name, PCWSTR::null()) });
    }
}

#[test]
fn a_path_on_a_local_subst_drive_opens_from_the_volume_it_names() {
    let scratch = Scratch::new("subst");
    let document = scratch.file(r"deals\Contract.docx");
    let drive = Subst::to(&scratch.0);
    let through = PathBuf::from(format!(r"{}:\deals\Contract.docx", drive.0));
    let opened = open_path(&through, Access::Read).unwrap();
    assert_eq!(opened.kind().unwrap(), Kind::File);
    assert_eq!(
        opened.identity().unwrap(),
        open_path(&document, Access::Look)
            .unwrap()
            .identity()
            .unwrap()
    );
    let folder = open_path(&PathBuf::from(format!(r"{}:\deals", drive.0)), Access::List).unwrap();
    assert_eq!(folder.names(10).unwrap(), ["Contract.docx"]);
}

/// A folder's own access list put back to its parent's inherited one on drop.
struct Inherit(PathBuf);

impl Drop for Inherit {
    fn drop(&mut self) {
        drop(auto_inherit_with(&self.0, "D:"));
    }
}

/// A folder whose access list grants the user only to pass through it and read its attributes
/// (traverse, read attributes, synchronize), and everything below it: a path through it opens,
/// its entries are told by opening them as themselves, and only its own listing is refused.
#[test]
fn a_folder_the_user_may_pass_through_but_not_list_stays_usable() {
    let scratch = Scratch::new("traverse");
    let document = scratch.file(r"locked\inner\Contract.docx");
    let locked = scratch.0.join("locked");
    protect_with(&locked, "D:P(A;;0x1000A0;;;WD)(A;OICIIO;FA;;;WD)").unwrap();
    let _restore = Inherit(locked.clone());
    match open_path(&locked, Access::List) {
        Ok(_) => {
            eprintln!(
                "SKIPPED a_folder_the_user_may_pass_through_but_not_list_stays_usable: the \
                 folder could be listed (a privilege that bypasses its access list)"
            );
            return;
        }
        Err(error) => assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}"),
    }
    for access in [Access::Look, Access::Read, Access::Exclusive] {
        let opened = open_path(&document, access).unwrap();
        assert_eq!(opened.kind().unwrap(), Kind::File, "{access:?}");
    }
    let inner = open_path(&locked.join("inner"), Access::List).unwrap();
    assert_eq!(inner.names(10).unwrap(), ["Contract.docx"]);
    let through = open_path(&locked, Access::Folder).unwrap();
    assert_eq!(through.entry_at("inner".as_ref()).unwrap(), Entry::Dir);
    assert!(through.open_at("inner".as_ref(), Access::List).is_ok());
}

/// An ordinary path on the runner's temp drive (`RUNNER_TEMP`, on `D:\`) opens, and its folder
/// lists: the open is not refused for a drive other than the system's.
#[test]
fn an_ordinary_path_on_the_runners_temp_drive_opens() {
    let Some(base) = std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .filter(|base| base.starts_with(r"D:\"))
    else {
        eprintln!("SKIPPED an_ordinary_path_on_the_runners_temp_drive_opens: no RUNNER_TEMP on D:");
        return;
    };
    let scratch = Scratch::under(&base, "d-drive");
    let document = scratch.file(r"m\Contract.docx");
    assert!(document.starts_with(r"D:\"), "{}", document.display());
    assert_eq!(
        open_path(&document, Access::Read).unwrap().kind().unwrap(),
        Kind::File
    );
    let folder = open_path(&scratch.0.join("m"), Access::List).unwrap();
    assert_eq!(folder.names(10).unwrap(), ["Contract.docx"]);
    assert_eq!(
        folder.entry_at("Contract.docx".as_ref()).unwrap(),
        Entry::File
    );
}
