//! The card's view of the walk: whether a path is a file, why a refused one was refused (a link to
//! a share is `Network`, a link that can't be read `Unresolved`), and a name Windows reads as
//! another place refused before anything is touched.

use std::path::{Path, PathBuf};

use super::{Disk, Resolved, resolve};
use crate::network_paths::NetEnv;
#[cfg(unix)]
use crate::network_paths::resolve::tests::Fake;
use crate::network_paths::resolve::tests::FakeFs;
use crate::network_paths::tests::plain_net;

fn as_written(_: &Path) -> Option<PathBuf> {
    None
}

fn on(path: &str, fs: &FakeFs, net: &NetEnv) -> Resolved {
    let disk = Disk {
        fs,
        real: &as_written,
        net,
    };
    resolve(Path::new(path), &disk)
}

#[cfg(unix)]
fn local(path: &str, is_file: bool) -> Resolved {
    Resolved::Local {
        path: path.into(),
        is_file,
    }
}

// Unix fixture paths aren't absolute on Windows, where the walk refuses them; Windows' walk
// rules are tested in network_paths' Windows tests.
#[cfg(unix)]
#[test]
fn a_file_a_folder_and_a_missing_tail() {
    let fs = FakeFs::with(&[("/srv/a/x.docx", Fake::File)]);
    let net = plain_net(false);
    assert_eq!(on("/srv/a/x.docx", &fs, &net), local("/srv/a/x.docx", true));
    assert_eq!(on("/srv/a", &fs, &net), local("/srv/a", false));
    assert_eq!(
        on("/srv/a/new/y.docx", &fs, &net),
        local("/srv/a/new/y.docx", true)
    );
}

// Unix fixture paths aren't absolute on Windows, where the walk refuses them; Windows' walk
// rules are tested in network_paths' Windows tests.
#[cfg(unix)]
#[test]
fn a_refusal_says_why() {
    let fs = FakeFs::with(&[
        ("/srv/share", Fake::Link(r"\\server\share".into())),
        ("/srv/up", Fake::Link("../../net/host".into())),
        ("/srv/bad", Fake::BadLink),
        ("/srv/locked", Fake::Denied),
    ]);
    let net = plain_net(false);
    for path in ["/srv/share/x", "/srv/up/x", "/net/host/x"] {
        assert_eq!(on(path, &fs, &net), Resolved::Network, "{path}");
    }
    for path in ["/srv/bad/x", "/srv/locked/x", "relative/x"] {
        assert_eq!(on(path, &fs, &net), Resolved::Unresolved, "{path}");
    }
    let touched = fs.touched();
    assert!(
        touched
            .iter()
            .all(|p| !p.starts_with("/srv/share/x") && !p.starts_with("/net")),
        "{touched:?}"
    );
}

#[test]
fn a_windows_name_for_another_place_is_refused_untouched() {
    let fs = FakeFs::default();
    let net = plain_net(true);
    for path in [
        r"C:\Users\rohan.\Documents",
        r"C:\Users\rohan \Documents",
        r"D:\Matters\Other.",
        r"C:\Users\rohan\AppData\Roaming\Microsoft\Credentials.\x",
        r"C:\Users\rohan\dir::$INDEX_ALLOCATION\x",
        r"C:\Users\rohan\notes.docx:hidden",
        r"C:\x\CON",
        r"C:\x\nul.txt",
        r"C:\x\COM1",
        r"C:\x\lpt9.log",
    ] {
        assert_eq!(on(path, &fs, &net), Resolved::Unresolved, "{path}");
    }
    assert_eq!(fs.touched(), Vec::<PathBuf>::new());
}
