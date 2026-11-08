//! PR #789's review on NTFS, rule 8: the live watch's reads go through handles. A link's target
//! is read from the link's own handle (`FSCTL_GET_REPARSE_POINT`); a junction partway along a
//! path stops the walk, never followed; a folder's name is its long one however it was opened;
//! a target spelled in another case is inside by identity; a symbolic link to a share is judged
//! by its words, never opened (one the VM makes in developer mode; skipped without it).

use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::super::ntfs::Ntfs;
use super::super::run::Volume as _;
use super::super::scan::{Kind, Last, Missed, locate, reach, remote};
use crate::windows::test_junction::junction;
use crate::windows::test_world::World;

#[test]
fn a_junction_partway_is_never_followed_and_its_target_is_read_from_its_own_handle() {
    let w = World::new("ntfs-aside-reads-partway");
    w.dir.dir("matter/sub");
    w.dir
        .file("elsewhere/inner/.git/HEAD", "ref: refs/heads/main\n");
    let root = w.canonical("matter");
    let elsewhere = w.canonical("elsewhere");
    junction(&root.join("hop"), &elsewhere).unwrap();
    let top = Ntfs.open_root(&root).unwrap();

    let hop = Ntfs.look(&top, OsStr::new("hop")).unwrap();
    assert!(matches!(hop, Some((_, Kind::Link))), "{hop:?}");
    assert!(Ntfs.open_folder(&top, OsStr::new("hop")).is_err());
    let key = ["hop", "inner", ".git"].map(OsString::from);
    assert!(matches!(reach(&Ntfs, &root, &key), Err(Missed::Link)));
    let target = Ntfs.link_target(&top, OsStr::new("hop")).unwrap();
    let text = target.to_string_lossy().to_lowercase();
    assert!(text.starts_with(r"\??\"), "{text}");
    let tail = elsewhere.to_string_lossy().to_lowercase();
    let tail = tail.trim_start_matches(r"\\?\");
    assert!(text.ends_with(tail), "{text} names {tail}");
    assert!(locate(&Ntfs, &root, (&root, &target), Last::Must).is_none());
    assert!(
        Ntfs.link_target(&top, OsStr::new("sub")).is_err(),
        "not a link"
    );
}

#[test]
fn a_target_in_another_case_is_inside_by_identity_with_its_folders_long_names() {
    let w = World::new("ntfs-aside-reads-identity");
    w.dir.dir("matter/Sub Folder/deeper");
    let root = w.canonical("matter");
    let top = Ntfs.open_root(&root).unwrap();
    let opened = Ntfs.open_folder(&top, OsStr::new("SUB FOLDER")).unwrap();
    assert_eq!(Ntfs.folder_name(&opened).unwrap(), "Sub Folder");

    let upper = std::path::PathBuf::from(root.to_string_lossy().to_uppercase());
    let target = upper.join(r"sub folder\.\deeper");
    let key = locate(&Ntfs, &root, (&root, &target), Last::Must).unwrap();
    assert_eq!(key, ["Sub Folder", "deeper"].map(OsString::from));
    let missing = root.join("Sub Folder").join("not yet");
    let watched = locate(&Ntfs, &root, (&root, &missing), Last::MayMiss).unwrap();
    assert_eq!(watched, ["Sub Folder", "not yet"].map(OsString::from));
    assert!(locate(&Ntfs, &root, (&root, &missing), Last::Must).is_none());
    assert!(Ntfs.open_top(Path::new(r"\\server\share\")).is_err());
}

#[test]
fn a_symbolic_link_to_a_share_is_judged_by_its_words_and_never_opened() {
    let w = World::new("ntfs-aside-reads-share");
    w.dir.dir("matter");
    let Ok(link) = w
        .dir
        .symlink_dir("matter/share", Path::new(r"\\server\share\x"))
    else {
        eprintln!("no symbolic links here (developer mode off); the VM row covers it");
        return;
    };
    let root = w.canonical("matter");
    let top = Ntfs.open_root(&root).unwrap();
    let target = Ntfs.link_target(&top, OsStr::new("share")).unwrap();
    assert!(remote(target.as_os_str()), "{}", target.display());
    assert!(locate(&Ntfs, &root, (&root, &target), Last::Must).is_none());
    assert!(std::fs::symlink_metadata(link).is_ok());
}
