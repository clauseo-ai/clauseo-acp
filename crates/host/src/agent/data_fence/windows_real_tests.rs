//! The fence on a real Windows volume (Windows only; on the runner the temp folder is on `D:`),
//! both ways. Allowed: an ordinary local matter, the session's own run folder, shell words with a
//! wildcard, a URL or a drive inside a script, a `\\?\D:\…` spelling, an 8.3 spelling and a local
//! `subst` drive, each judged by where it leads. Refused: the data folder by each of those
//! spellings, and a UNC path, `\\?\UNC\…`, a mapped drive and a `subst` to a share, with nothing
//! touched (the fake disk counts every look).

use std::path::{Path, PathBuf};

use serde_json::json;

use super::resolve::Disk;
use super::tests::{check, layout, text};
use crate::network_paths::resolve::tests::FakeFs;
use crate::network_paths::resolve::{Fs, RealFs, Walked, walk};
use crate::network_paths::{NetEnv, TestDrive, drive_is_local, short_path};

/// `layout`'s folder, its canonical path.
fn root_of(dir: &crate::pipeline::tests::support::TempDir) -> PathBuf {
    dunce::canonicalize(dir.path()).unwrap()
}

fn denied(root: &Path, path: &str) -> bool {
    check(root, &[path], None).touches_data()
}

fn command_denied(root: &Path, command: &str) -> bool {
    check(root, &[], Some(&json!({ "command": command }))).touches_data()
}

/// The paths of an ordinary local matter, the session's own folders and an unrelated folder
/// pass, and so do shell words the system rejects as names (a wildcard, a URL, a drive inside a
/// script word): #766's Windows regression, where such a word's identity read failed and
/// refused the request. The data folder is still refused.
#[test]
fn an_ordinary_local_matter_passes_and_the_data_folder_is_refused() {
    let dir = layout("win-ordinary");
    let root = root_of(&dir);
    let at = |place: &str| text(&root.join(place));
    for path in [
        at(r"matter\sub\a.txt"),
        at(r"matter\sub\new.docx"),
        at(r"outside\x.txt"),
        at(r"data\run\s1\work\out.txt"),
        at(r"data\run\s1\work"),
        r"sub\a.txt".to_owned(),
        r"sub\*.txt".to_owned(),
    ] {
        assert!(!denied(&root, &path), "{path}");
    }
    let slashed = |place: &str| at(place).replace('\\', "/");
    for command in [
        format!(r"type {}\*.txt", at(r"matter\sub")),
        format!("ls {} https://example.com/a", at("outside")),
        format!("find {} -name '*.docx'", at("outside")),
        format!("bash -c 'grep -r x {}'", slashed("matter")),
        format!("bash -c 'cat {}'", slashed("data/run/s1/work/out.txt")),
        r"cat sub/*.txt".to_owned(),
    ] {
        assert!(!command_denied(&root, &command), "{command}");
    }
    for path in [
        at(r"data\skill-packs\x\skills\brief\SKILL.md"),
        at(r"data\run\s2\work\a.txt"),
        at(r"data\state.sqlite"),
    ] {
        assert!(denied(&root, &path), "{path}");
    }
    assert!(command_denied(
        &root,
        &format!(r"type {}", at(r"data\state.sqlite"))
    ));
}

/// A `\\?\D:\…` spelling of a local folder is walked as the folder: the matter passes, the data
/// folder is refused.
#[test]
fn a_long_path_spelling_is_judged_as_the_folder() {
    let dir = layout("win-verbatim");
    let root = root_of(&dir);
    let verbatim = |place: &str| format!(r"\\?\{}", root.join(place).display());
    assert!(!denied(&root, &verbatim(r"matter\sub\a.txt")));
    assert!(!denied(&root, &verbatim(r"data\run\s1\work\out.txt")));
    assert!(denied(
        &root,
        &verbatim(r"data\skill-packs\x\skills\brief\SKILL.md")
    ));
}

/// An 8.3 spelling of a local folder is walked to its long form: the matter passes, the data
/// folder is refused. Skipped where the volume makes no short names.
#[test]
fn a_short_name_spelling_is_judged_as_the_folder() {
    let dir = layout("win-short-name-of-a-long-folder");
    let root = root_of(&dir);
    let Some(short) = short_path(&root).filter(|short| *short != root) else {
        eprintln!("skipped: the volume gave no short name for the test folder");
        return;
    };
    let at = |place: &str| text(&short.join(place));
    assert!(!denied(&root, &at(r"matter\sub\a.txt")));
    assert!(denied(
        &root,
        &at(r"data\skill-packs\x\skills\brief\SKILL.md")
    ));
    assert!(command_denied(
        &root,
        &format!("type {}", at(r"data\state.sqlite"))
    ));
}

/// The `subst` drives, in one test so that no two test processes define drive letters
/// at once. A local one is local (its device `\??\D:\…` followed to its drive) and judged by
/// where it points: a path through it walks to the folder (the same identity by either
/// spelling), the matter through it passes, the data folder through it is refused, as a path and
/// as a search root. One to a share (`\??\UNC\…`) is remote, asked by its letter alone, and a
/// path on it is refused with nothing on it looked at.
#[test]
fn subst_drives_are_judged_by_where_they_point() {
    let dir = layout("win-subst");
    let root = root_of(&dir);
    let Some(local) = TestDrive::define(&format!(r"\??\{}", root.display())) else {
        eprintln!("skipped: no free drive letter, or it could not be defined");
        return;
    };
    let net = NetEnv::current();
    let at = |place: &str| format!(r"{}:\{place}", local.letter);
    let through = PathBuf::from(at(r"matter\sub\a.txt"));
    assert!(!net.is_network(&through));
    assert_eq!(
        walk(&through, &Disk::real(&net)),
        Walked::Local(root.join(r"matter\sub\a.txt"))
    );
    assert_eq!(
        RealFs.identity(&through).unwrap(),
        RealFs.identity(&root.join(r"matter\sub\a.txt")).unwrap()
    );
    assert!(!denied(&root, &at(r"matter\sub\a.txt")));
    assert!(denied(
        &root,
        &at(r"data\skill-packs\x\skills\brief\SKILL.md")
    ));
    let search = json!({"pattern": "secret", "path": at("data")});
    assert!(check(&root, &[], Some(&search)).touches_data());
    let Some(share) = TestDrive::define(r"\??\UNC\clauseo-test-server.invalid\share") else {
        eprintln!("the subst-to-a-share case is skipped: no second free drive letter");
        return;
    };
    let on_share = format!(r"{}:\matter\x.docx", share.letter);
    assert!(NetEnv::current().is_network(Path::new(&on_share)));
    let fs = FakeFs::default();
    let real = |_: &Path| None;
    let disk = Disk {
        fs: &fs,
        real: &real,
        net: &net,
    };
    assert_eq!(walk(Path::new(&on_share), &disk), Walked::Refused);
    assert!(check(&root, &[on_share.as_str()], None).touches_data_on(true, &disk));
    let letter = format!("{}:", share.letter);
    assert!(
        !fs.touched()
            .iter()
            .any(|path| path.to_string_lossy().starts_with(&letter)),
        "{:?}",
        fs.touched()
    );
}

/// A UNC path, `\\?\UNC\…` and a mapped drive (its device faked as the SMB redirector's) are
/// refused, and nothing on the network is looked at.
#[test]
fn network_paths_are_refused_untouched() {
    let dir = layout("win-network");
    let root = root_of(&dir);
    let server = "clauseo-test-server.invalid";
    let paths = [
        format!(r"\\{server}\share\x.docx"),
        format!(r"\\?\UNC\{server}\share\x.docx"),
        r"Z:\matter\x.docx".to_owned(),
    ];
    // Z: answers as a drive mapped to a share (the SMB redirector's device); every other letter
    // as this machine's own does.
    let mapped = |letter: char| {
        let device = |asked: char| {
            if asked == 'Z' {
                Some(r"\Device\LanmanRedirector\;Z:0000000000012345\server\share".to_owned())
            } else {
                let root = format!(r"{asked}:\");
                (!NetEnv::current().is_network(Path::new(&root)))
                    .then(|| r"\Device\HarddiskVolume1".to_owned())
            }
        };
        !drive_is_local(letter, &device)
    };
    let net = NetEnv {
        remote_drive: Box::new(mapped),
        ..NetEnv::current()
    };
    for path in &paths {
        let fs = FakeFs::default();
        let real = |_: &Path| None;
        let disk = Disk {
            fs: &fs,
            real: &real,
            net: &net,
        };
        assert!(
            check(&root, &[path.as_str()], None).touches_data_on(true, &disk),
            "{path}"
        );
        let on_network: Vec<PathBuf> = fs
            .touched()
            .into_iter()
            .filter(|touched| {
                let text = touched.to_string_lossy().to_ascii_lowercase();
                text.contains(server) || text.starts_with(r"\\") || text.starts_with("z:")
            })
            .collect();
        assert_eq!(on_network, Vec::<PathBuf>::new(), "{path}");
    }
}
