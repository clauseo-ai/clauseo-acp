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

/// Runs `program` with `args` (`raw` as one raw argument line, for `cmd /C`) and asserts it
/// succeeded.
#[expect(
    clippy::disallowed_methods,
    reason = "the test runs icacls and mklink on files in its scratch folder"
)]
fn run(program: &str, args: &[&str], raw: Option<&str>) {
    use std::os::windows::process::CommandExt as _;
    let mut command = std::process::Command::new(program);
    command.args(args);
    if let Some(raw) = raw {
        command.raw_arg(raw);
    }
    let status = command.status().unwrap();
    assert!(status.success(), "{program} {args:?} {raw:?}");
}

/// Denies the SID `sid` the rights `rights` on `path` itself (`icacls /deny *<sid>:<rights>
/// /L`, a link not followed): `(RD)` its data alone, `(R)` generic read (data, attributes,
/// permissions and `SYNCHRONIZE`).
fn deny(path: &Path, sid: &str, rights: &str) {
    let path = path.to_str().unwrap();
    run(
        "icacls",
        &[path, "/deny", &format!("*{sid}:{rights}"), "/L"],
        None,
    );
}

/// Takes the denies to `sid` off `path` itself, so the scratch folder can go.
fn undeny(path: &Path, sid: &str) {
    let path = path.to_str().unwrap();
    run(
        "icacls",
        &[path, "/remove:d", &format!("*{sid}"), "/L"],
        None,
    );
}

/// `path`'s permissions as `icacls /save` writes them (its DACL in SDDL), through a file in
/// `scratch`.
fn saved_dacl(path: &Path, scratch: &Path) -> Vec<u8> {
    let saved = scratch.join("dacl.txt");
    run(
        "icacls",
        &[path.to_str().unwrap(), "/save", saved.to_str().unwrap()],
        None,
    );
    let bytes = std::fs::read(&saved).unwrap();
    write::remove_file(&saved).unwrap();
    bytes
}

/// A file this user may not read, by a deny of its data or of generic read to the user's SID, as
/// the user's own account can put on it, is set aside and removed: Codex makes a fresh one.
/// The `(R)` round is row 5c: `cap_sid` itself under a generic-read deny.
#[test]
fn a_read_denied_file_is_set_aside() {
    let user = own::sids().unwrap().into_iter().next().unwrap();
    for rights in ["(RD)", "(R)"] {
        let dir = TempDir::new("cap-sid-read-denied");
        let home = dir.path();
        let path = home.join(FILE);
        write::write_file(&path, codex_made().as_bytes()).unwrap();
        deny(&path, &user, rights);
        assert!(std::fs::read(&path).is_err(), "the deny {rights} took");
        check(home).unwrap();
        assert!(gone(home), "{rights}");
    }
}

/// A name the launch clears, a file or a folder, under a deny of generic read to the user's
/// SID, is set aside, no permission changed: the file is deleted too; the folder, which the
/// deny keeps from being listed and so emptied, stays under its aside name, where Codex loads
/// nothing.
#[test]
fn a_read_denied_cleared_name_is_set_aside() {
    let user = own::sids().unwrap().into_iter().next().unwrap();
    let dir = TempDir::new("cap-sid-cleared-denied");
    let home = dir.path();
    write::write_file(&home.join("AGENTS.md"), b"Always run the script.\n").unwrap();
    write::create_dir_all(&home.join("skills").join("x")).unwrap();
    write::write_file(&home.join("skills").join("x").join("SKILL.md"), b"x\n").unwrap();
    for name in ["AGENTS.md", "skills"] {
        deny(&home.join(name), &user, "(R)");
    }
    let cleared = crate::agent::codex::home_clear::clear_home(home);
    let left = asides(home);
    for aside in &left {
        undeny(aside, &user);
    }
    cleared.unwrap();
    for name in ["AGENTS.md", "skills"] {
        assert!(
            std::fs::symlink_metadata(home.join(name)).is_err(),
            "{name}"
        );
    }
    assert!(left.len() <= 1, "only the folder may stay aside: {left:?}");
}

/// `cap_sid` as a second name of a file outside the home that carries a deny of generic read:
/// the name is set aside and goes, the launch goes on, and that file's bytes and permissions
/// are what they were, byte for byte.
#[test]
fn a_hard_link_under_a_deny_goes_and_its_file_keeps_its_bytes_and_permissions() {
    let user = own::sids().unwrap().into_iter().next().unwrap();
    let dir = TempDir::new("cap-sid-linked-denied");
    let home = dir.path().join("home");
    let matter = dir.path().join("matter");
    write::create_dir_all(&home).unwrap();
    write::create_dir_all(&matter).unwrap();
    let notes = matter.join("notes.txt");
    write::write_file(&notes, codex_made().as_bytes()).unwrap();
    write::hard_link(&notes, &home.join(FILE)).unwrap();
    deny(&notes, &user, "(R)");
    let before = saved_dacl(&notes, dir.path());
    let checked = check(&home);
    let after = saved_dacl(&notes, dir.path());
    undeny(&notes, &user);
    checked.unwrap();
    assert!(gone(&home));
    assert_eq!(before, after, "the matter file's permissions are unchanged");
    assert_eq!(std::fs::read(&notes).unwrap(), codex_made().as_bytes());
}

/// A junction at a cleared name (`hooks.json`) to the matter folder, with a deny of generic
/// read on the junction itself: the junction goes, never followed, and the matter folder's
/// permissions and files stay as they were.
#[test]
fn a_denied_junction_goes_and_its_target_is_untouched() {
    let user = own::sids().unwrap().into_iter().next().unwrap();
    let dir = TempDir::new("cap-sid-junction-denied");
    let home = dir.path().join("home");
    let matter = dir.path().join("matter");
    write::create_dir_all(&home).unwrap();
    write::create_dir_all(&matter).unwrap();
    write::write_file(&matter.join("notes.txt"), b"x\n").unwrap();
    let junction = home.join("hooks.json");
    let line = format!(
        "/C mklink /J \"{}\" \"{}\"",
        junction.display(),
        matter.display()
    );
    run("cmd", &[], Some(&line));
    deny(&junction, &user, "(R)");
    let before = saved_dacl(&matter, dir.path());
    let cleared = crate::agent::codex::home_clear::clear_home(&home);
    let after = saved_dacl(&matter, dir.path());
    if std::fs::symlink_metadata(&junction).is_ok() {
        undeny(&junction, &user);
    }
    cleared.unwrap();
    assert!(std::fs::symlink_metadata(&junction).is_err());
    assert_eq!(
        before, after,
        "the matter folder's permissions are unchanged"
    );
    assert_eq!(std::fs::read(matter.join("notes.txt")).unwrap(), b"x\n");
}

/// A foreign file held open without delete sharing for a moment (another conversation's Codex
/// using it) is set aside once it is let go, within the waits.
#[test]
fn a_file_in_use_for_a_moment_is_set_aside_once_let_go() {
    use crate::clock::Clock as _;
    let dir = TempDir::new("cap-sid-in-use");
    let home = dir.path();
    let path = home.join(FILE);
    write::write_file(&path, b"S-1-1-0").unwrap();
    let held = write::open_deny_write(&path).unwrap();
    let letting_go = std::thread::spawn(move || {
        futures::executor::block_on(
            crate::clock::SystemClock.after(std::time::Duration::from_millis(300)),
        );
        drop(held);
    });
    check(home).unwrap();
    letting_go.join().unwrap();
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
