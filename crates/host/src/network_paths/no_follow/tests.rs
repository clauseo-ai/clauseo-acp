//! The no-follow open on Linux, on real folders: a walked path opens; a folder or the file on it
//! swapped for a symlink after the walk refuses, the link's target never reached (the held-open
//! check through it finds nothing); the per-name fallback a kernel without `openat2` takes
//! refuses the same, through the `openat2` seam answering `ENOSYS`; and the walk's own looks
//! still read a link and walk its target.

use std::path::{Path, PathBuf};

use rustix::io::Errno;

use super::sys::{self, by_name, fell_back, open_linux, reset_fallback};
use super::{Access, Kind, WalkedPath, held_open_walked, is_link_on_the_way, open_no_follow};
use crate::network_paths::resolve::{Disk, Entry, Fs as _, RealFs, Walked, walk};
use crate::network_paths::tests::plain_net;
use crate::network_paths::touchable;
use crate::write::{self, symlink};

/// A folder under the system's temp folder, its own path canonical (no link on it), removed on
/// drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = dunce::canonicalize(std::env::temp_dir())
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

/// The walked path of `path` in the matter `matter`, as the guard and the fence get it.
fn walked(path: &Path, matter: &Path) -> WalkedPath {
    let net = plain_net(false);
    touchable(path, matter, &Disk::resolved(&net)).expect("a local path the walk vouches for")
}

/// A matter `m` with `sub/Contract.docx` held by its owner file, and elsewhere a folder
/// `outside/` with the same two files, where a swapped-in link will point.
fn held_matter(scratch: &Scratch) -> (PathBuf, PathBuf) {
    let matter = scratch.0.join("m");
    let document = scratch.file("m/sub/Contract.docx");
    scratch.file("m/sub/~$ntract.docx");
    scratch.file("outside/Contract.docx");
    scratch.file("outside/~$ntract.docx");
    (matter, document)
}

#[test]
fn a_walked_path_opens_and_tells_its_kind_identity_and_entries() {
    let scratch = Scratch::new("opens");
    let (matter, document) = held_matter(&scratch);
    let file = walked(&document, &matter);
    let opened = open_no_follow(&file, Access::Look).unwrap();
    assert_eq!(opened.kind().unwrap(), Kind::File);
    assert!(open_no_follow(&file, Access::Read).is_ok());
    let folder = open_no_follow(&walked(&matter.join("sub"), &matter), Access::List).unwrap();
    assert_eq!(folder.kind().unwrap(), Kind::Dir);
    let mut names = folder.names(10).unwrap();
    names.sort();
    assert_eq!(names, ["Contract.docx", "~$ntract.docx"]);
    assert_eq!(
        folder.identity_at("Contract.docx".as_ref()).unwrap(),
        opened.identity().unwrap()
    );
    let held = held_open_walked(&matter, &file).map(|(path, _)| path);
    assert_eq!(held.unwrap().as_str(), "sub/Contract.docx");
    // A missing name is not found, never anything else.
    let missing = walked(&matter.join("sub/Missing.docx"), &matter);
    let error = open_no_follow(&missing, Access::Look).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn a_folder_swapped_for_a_symlink_after_the_walk_is_refused_and_its_target_never_reached() {
    let scratch = Scratch::new("swap-folder");
    let (matter, document) = held_matter(&scratch);
    let file = walked(&document, &matter);
    // The swap, between the walk and the touch.
    write::rename(&matter.join("sub"), &matter.join("was-sub")).unwrap();
    symlink(&scratch.0.join("outside"), &matter.join("sub")).unwrap();
    for access in [Access::Look, Access::List, Access::Read, Access::Exclusive] {
        let error = open_no_follow(&file, access).unwrap_err();
        assert!(is_link_on_the_way(&error), "{access:?}: {error}");
    }
    let folder = file.as_path().parent().unwrap().to_path_buf();
    let error = super::open_path(&folder, Access::List).unwrap_err();
    assert!(is_link_on_the_way(&error), "{error}");
    // Through the link the owner file would show the document held; the check never gets there.
    assert_eq!(held_open_walked(&matter, &file), None);
    let inside = clauseo_protocol::MatterPath::new("sub/Contract.docx").unwrap();
    assert!(
        clauseo_matters::held_open(&matter, &inside).is_some(),
        "a look by the path follows the link"
    );
}

#[test]
fn the_file_itself_swapped_for_a_symlink_is_refused() {
    let scratch = Scratch::new("swap-file");
    let (matter, document) = held_matter(&scratch);
    let file = walked(&document, &matter);
    write::remove_file(&document).unwrap();
    symlink(&scratch.0.join("outside/Contract.docx"), &document).unwrap();
    let error = open_no_follow(&file, Access::Look).unwrap_err();
    assert!(is_link_on_the_way(&error), "{error}");
    let folder = open_no_follow(&walked(&matter.join("sub"), &matter), Access::List).unwrap();
    let error = folder
        .open_at("Contract.docx".as_ref(), Access::Read)
        .unwrap_err();
    assert!(is_link_on_the_way(&error), "{error}");
    // Looked at as an entry, it is a link, never its target.
    assert_eq!(
        folder.entry_at("Contract.docx".as_ref()).unwrap(),
        Entry::Link
    );
}

#[test]
fn without_openat2_the_open_goes_name_by_name_and_still_refuses_a_link() {
    let scratch = Scratch::new("enosys");
    let (matter, document) = held_matter(&scratch);
    let no_openat2 = |_: &Path, _| Err(Errno::NOSYS);
    reset_fallback();
    let opened = open_linux(&document, Access::Look, &no_openat2).unwrap();
    assert!(fell_back(), "ENOSYS is remembered");
    assert_eq!(opened.kind().unwrap(), Kind::File);
    // Once remembered, `openat2` is not asked again.
    let never = |_: &Path, _| -> rustix::io::Result<std::os::fd::OwnedFd> {
        panic!("openat2 asked after ENOSYS")
    };
    assert!(open_linux(&matter.join("sub"), Access::List, &never).is_ok());
    write::rename(&matter.join("sub"), &matter.join("was-sub")).unwrap();
    symlink(&scratch.0.join("outside"), &matter.join("sub")).unwrap();
    let error = open_linux(&document, Access::Read, &never).unwrap_err();
    assert!(is_link_on_the_way(&error), "{error}");
    reset_fallback();
}

#[test]
fn name_by_name_a_link_anywhere_refuses_and_a_file_on_the_way_is_not_a_folder() {
    let scratch = Scratch::new("by-name");
    let document = scratch.file("real/deep/Brief.docx");
    symlink(&scratch.0.join("real"), &scratch.0.join("link")).unwrap();
    symlink(&document, &scratch.0.join("real/deep/Alias.docx")).unwrap();
    assert!(by_name(&document, Access::Look).is_ok());
    for path in [
        scratch.0.join("link/deep/Brief.docx"),
        scratch.0.join("real/deep/Alias.docx"),
        scratch.0.join("link"),
    ] {
        let error = by_name(&path, Access::Look).unwrap_err();
        assert!(is_link_on_the_way(&error), "{}: {error}", path.display());
    }
    let error = by_name(&document.join("x"), Access::Look).unwrap_err();
    assert!(!is_link_on_the_way(&error));
    assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
    // `openat2` refuses the same links.
    for path in [
        scratch.0.join("link/deep/Brief.docx"),
        scratch.0.join("real/deep/Alias.docx"),
    ] {
        let error = sys::open_path(&path, Access::Look).unwrap_err();
        assert!(is_link_on_the_way(&error), "{error}");
    }
    // A relative path or one with `..` is never opened.
    assert!(by_name(Path::new("real/deep"), Access::Look).is_err());
    assert!(by_name(&scratch.0.join("real/../real"), Access::Look).is_err());
}

#[test]
fn the_walks_looks_read_a_link_and_refuse_a_folder_that_became_one() {
    let scratch = Scratch::new("walk");
    let document = scratch.file("m/Deals/spa.docx");
    symlink(Path::new("Deals"), &scratch.0.join("m/l")).unwrap();
    let net = plain_net(false);
    assert_eq!(
        walk(&scratch.0.join("m/l/spa.docx"), &Disk::real(&net)),
        Walked::Local(document)
    );
    assert_eq!(RealFs.entry(&scratch.0.join("m/l")).unwrap(), Entry::Link);
    assert_eq!(
        RealFs.read_link(&scratch.0.join("m/l")).unwrap(),
        PathBuf::from("Deals")
    );
    // A look past a name that is a link (the walk read it as one, so a folder the walk passed
    // and an agent swapped since) refuses, never following it.
    let error = RealFs.entry(&scratch.0.join("m/l/spa.docx")).unwrap_err();
    assert!(is_link_on_the_way(&error), "{error}");
    assert!(is_link_on_the_way(
        &RealFs
            .identity(&scratch.0.join("m/l/spa.docx"))
            .unwrap_err()
    ));
    assert_eq!(
        RealFs.identity(&scratch.0.join("m/l")).unwrap(),
        RealFs.identity(&scratch.0.join("m/l")).unwrap(),
        "a link's own identity"
    );
    assert_eq!(RealFs.entry(Path::new("/")).unwrap(), Entry::Dir);
}
