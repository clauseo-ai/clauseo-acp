//! The Windows checks on a real volume (Windows only; on the runner the temp folder is on `D:`):
//! a directory's identity opens (`FILE_FLAG_BACKUP_SEMANTICS`), a name the system rejects is no
//! such name for the identity as for the walk, and a mapped drive, a UNC path and `\\?\UNC\` are
//! refused with nothing touched (the fake disk counts every look; a drive is asked by its letter
//! alone). The `subst` drives are tested in one test, `agent/data_fence/windows_real_tests.rs`.

use std::path::{Path, PathBuf};

use super::NetEnv;
use super::resolve::tests::FakeFs;
use super::resolve::{Disk, Fs, RealFs, Walked, no_such_name, walk};
use super::windows_names::drive_is_local;
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// A temp folder holding `matter\sub\a.txt`, its canonical path.
fn matter_dir(test: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new(test);
    let root = dunce::canonicalize(dir.path()).unwrap();
    write::create_dir_all(&root.join("matter").join("sub")).unwrap();
    write::write_file(&root.join("matter").join("sub").join("a.txt"), b"a").unwrap();
    (dir, root)
}

fn walk_on(path: &Path, fs: &dyn Fs, net: &NetEnv) -> Walked {
    let real = |_: &Path| None;
    walk(
        path,
        &Disk {
            fs,
            real: &real,
            net,
        },
    )
}

/// The cause of #766's Windows failures: a name the walk keeps because the system rejects it (a
/// shell word's `*`, a URL's `https:`, a `D:` inside a word) made the identity read fail, and the
/// fence took that failure as an unreadable identity and refused the request. The identity read
/// must answer such a name as no such name, as the walk does; and a folder's identity, the
/// drive's root's too, must open.
#[test]
fn a_folders_identity_opens_and_a_rejected_name_is_no_such_name() {
    let (_dir, root) = matter_dir("win-identity");
    let matter = root.join("matter");
    let mut ancestors = matter.ancestors();
    assert!(ancestors.all(|folder| RealFs.identity(folder).is_ok()));
    assert_ne!(
        RealFs.identity(&matter).unwrap(),
        RealFs.identity(&matter.join("sub")).unwrap()
    );
    for name in [r"sub\*.txt", r"sub\?.txt", r"https:\example.com\a"] {
        let error = RealFs.identity(&matter.join(name)).unwrap_err();
        assert!(no_such_name(&error), "{name}: {error:?}");
    }
    let missing = RealFs.identity(&matter.join("gone").join("x")).unwrap_err();
    assert!(no_such_name(&missing), "{missing:?}");
}

/// UNC, `\\?\UNC\` and a mapped drive (its device faked as the SMB redirector's or Mup's) are
/// refused from their text or their drive's device, before anything is opened.
#[test]
fn unc_paths_and_a_mapped_drive_are_refused_untouched() {
    let net = NetEnv::current();
    let fs = FakeFs::default();
    for path in [
        r"\\clauseo-test-server.invalid\share\x.docx",
        r"\\?\UNC\clauseo-test-server.invalid\share\x.docx",
        r"//clauseo-test-server.invalid/share/x",
    ] {
        assert_eq!(
            walk_on(Path::new(path), &fs, &net),
            Walked::Refused,
            "{path}"
        );
    }
    for device in [
        r"\Device\LanmanRedirector\;Z:0000000000012345\server\share",
        r"\Device\Mup\;Z:0000000000012345\server\share",
    ] {
        let query = move |letter: char| (letter == 'Z').then(|| device.to_owned());
        assert!(!drive_is_local('Z', &query), "{device}");
        let mapped = NetEnv {
            remote_drive: Box::new(move |letter| {
                let query = |asked: char| (asked == 'Z').then(|| device.to_owned());
                !drive_is_local(letter, &query)
            }),
            ..NetEnv::current()
        };
        assert_eq!(
            walk_on(Path::new(r"Z:\m\x.docx"), &fs, &mapped),
            Walked::Refused
        );
    }
    assert_eq!(fs.touched(), Vec::<PathBuf>::new());
}
