//! Windows: [`check`] on a real home, and [`own::sids`] on this process's token.

use std::path::{Path, PathBuf};

use super::{FILE, MAX_FILE, check, own};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// `cap_sid` text as Codex writes it, with SIDs of the shape Codex makes.
fn codex_made() -> String {
    serde_json::json!({
        "workspace": "S-1-5-21-3922318142-1240498823-2788036105-3021407425",
        "readonly": "S-1-5-21-12-34-56-78",
        "workspace_by_cwd": {"c:/matter": "S-1-5-21-1-2-3-4"},
    })
    .to_string()
}

/// What is in `home` under the name `cap_sid` is set aside under: none left means removed.
fn asides(home: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(home).unwrap();
    let paths = entries.map(|entry| entry.unwrap().path());
    paths
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name.starts_with(".clauseo-aside-")
        })
        .collect()
}

/// Whether `home` has no `cap_sid` and nothing set aside: the entry was set aside and removed.
fn gone(home: &Path) -> bool {
    std::fs::symlink_metadata(home.join(FILE)).is_err() && asides(home).is_empty()
}

/// The token gives the user's SID, then its groups': more than the one entry the type names,
/// every one a SID, Everyone among the groups.
#[test]
fn own_sids_give_the_user_and_every_group() {
    let sids = own::sids().unwrap();
    assert!(sids.len() > 2, "{} SIDs", sids.len());
    assert!(sids.iter().all(|sid| sid.starts_with("S-1-")));
    assert!(sids.first().unwrap().starts_with("S-1-5-"));
    assert!(sids.iter().skip(1).any(|sid| sid == "S-1-1-0"));
}

/// A file Codex made stays, byte for byte; nothing there is fine.
#[test]
fn a_codex_made_file_stays() {
    let dir = TempDir::new("cap-sid-kept");
    let home = dir.path();
    check(home).unwrap();
    write::write_file(&home.join(FILE), codex_made().as_bytes()).unwrap();
    check(home).unwrap();
    assert_eq!(
        std::fs::read(home.join(FILE)).unwrap(),
        codex_made().as_bytes()
    );
    assert_eq!(asides(home), Vec::<PathBuf>::new());
}

/// A file holding the user's own SID, or one of its groups' (read past the first entry), is
/// set aside and removed, and the launch goes on.
#[test]
fn a_sid_the_user_holds_is_set_aside() {
    let sids = own::sids().unwrap();
    for sid in [sids.first().unwrap(), sids.last().unwrap()] {
        let dir = TempDir::new("cap-sid-held");
        let home = dir.path();
        write::write_file(&home.join(FILE), sid.as_bytes()).unwrap();
        check(home).unwrap();
        assert!(gone(home), "{sid}");
    }
}

/// A second name of a matter file, with text Codex could have made, is set aside and removed;
/// the matter file keeps its bytes.
#[test]
fn a_hard_link_is_set_aside_and_its_file_kept() {
    let dir = TempDir::new("cap-sid-hard-link");
    let home = dir.path().join("home");
    let matter = dir.path().join("matter");
    write::create_dir_all(&home).unwrap();
    write::create_dir_all(&matter).unwrap();
    let notes = matter.join("notes.txt");
    write::write_file(&notes, codex_made().as_bytes()).unwrap();
    write::hard_link(&notes, &home.join(FILE)).unwrap();
    check(&home).unwrap();
    assert!(gone(&home));
    assert_eq!(std::fs::read(&notes).unwrap(), codex_made().as_bytes());
}

/// A folder at the name, with what it holds, is set aside and removed.
#[test]
fn a_folder_is_set_aside() {
    let dir = TempDir::new("cap-sid-folder");
    let home = dir.path();
    write::create_dir_all(&home.join(FILE).join("inner")).unwrap();
    write::write_file(&home.join(FILE).join("inner").join("x"), b"x").unwrap();
    check(home).unwrap();
    assert!(gone(home));
}

/// Denies the SID `sid` reading `path`'s data (`icacls /deny *<sid>:(RD)`): its attributes stay
/// readable, so it can still be renamed and removed.
#[expect(
    clippy::disallowed_methods,
    reason = "the test runs icacls to deny the user reading a file in its scratch folder"
)]
fn deny_read_data(path: &Path, sid: &str) {
    let status = std::process::Command::new("icacls")
        .arg(path)
        .args(["/deny", &format!("*{sid}:(RD)")])
        .status()
        .unwrap();
    assert!(status.success(), "icacls");
}

/// A file this user may not read (a deny of read-data to the user's SID) is set aside and
/// removed.
#[test]
fn a_read_denied_file_is_set_aside() {
    let dir = TempDir::new("cap-sid-read-denied");
    let home = dir.path();
    let path = home.join(FILE);
    write::write_file(&path, codex_made().as_bytes()).unwrap();
    let user = own::sids().unwrap().into_iter().next().unwrap();
    deny_read_data(&path, &user);
    assert!(std::fs::read(&path).is_err(), "the deny took");
    check(home).unwrap();
    assert!(gone(home));
}

/// Text that is not UTF-8, or longer than [`MAX_FILE`] (blank, which Codex would write over), is
/// set aside and removed.
#[test]
fn unreadable_text_is_set_aside() {
    let long = vec![b' '; usize::try_from(MAX_FILE).unwrap() + 1];
    for bytes in [b"S-1-5-21-1-2-3-\xff".to_vec(), long] {
        let dir = TempDir::new("cap-sid-text");
        let home = dir.path();
        write::write_file(&home.join(FILE), &bytes).unwrap();
        check(home).unwrap();
        assert!(gone(home));
    }
}

/// A foreign file that can't be renamed (held open without delete sharing) refuses the launch,
/// and stays where it is.
#[test]
fn a_foreign_file_that_cannot_be_moved_refuses_the_launch() {
    let dir = TempDir::new("cap-sid-held-open");
    let home = dir.path();
    let path = home.join(FILE);
    write::write_file(&path, b"S-1-1-0").unwrap();
    let held = write::open_deny_write(&path).unwrap();
    assert!(check(home).is_err());
    drop(held);
    assert!(std::fs::symlink_metadata(&path).is_ok());
}
