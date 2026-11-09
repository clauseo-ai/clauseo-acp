//! The walk under Windows' rules (`NetEnv::windows`), on the fake disk so it runs in every
//! test run: `.` and `..` are dropped by their text before anything is opened, so a `..` never
//! backs up from where a junction led; a relative link target is joined to the link's folder by
//! its text. Unix paths stand in for drive paths off Windows (the walk splits both the same
//! way); the `C:\` spellings run on Windows itself.

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

use super::tests::{Fake, FakeFs};
use super::{Disk, Walked, walk};
use crate::network_paths::NetEnv;
use crate::network_paths::tests::plain_net;

fn on(path: &str, fs: &FakeFs, net: &NetEnv) -> Walked {
    let real = |_: &Path| None;
    let disk = Disk {
        fs,
        real: &real,
        net,
    };
    walk(Path::new(path), &disk)
}

fn local(path: &str) -> Walked {
    Walked::Local(path.into())
}

/// `C:\m\j\..\x` with `j` a junction opens `C:\m\x`: Win32 drops `j\..` from the text first.
/// Unix backs up from where the link led.
#[cfg(unix)]
#[test]
fn a_dot_dot_after_a_junction_is_dropped_by_its_text() {
    let fs = FakeFs::with(&[
        ("/m/j", Fake::Link("/deep/a/b".into())),
        ("/deep/a/b/x", Fake::File),
        ("/m/x", Fake::File),
    ]);
    assert_eq!(on("/m/j/../x", &fs, &plain_net(true)), local("/m/x"));
    // Nothing under the junction was looked at, nor the junction itself.
    let touched = fs.touched();
    assert!(
        !touched
            .iter()
            .any(|path| path.starts_with("/deep") || path == Path::new("/m/j")),
        "{touched:?}"
    );
    let unix = FakeFs::with(&[
        ("/m/j", Fake::Link("/deep/a/b".into())),
        ("/deep/a/b/x", Fake::File),
    ]);
    assert_eq!(
        on("/m/j/../x", &unix, &plain_net(false)),
        local("/deep/a/x")
    );
    // `.` and `..` past the root stay at the root.
    let fs = FakeFs::with(&[("/m/x", Fake::File)]);
    assert_eq!(on("/m/./../../m/x", &fs, &plain_net(true)), local("/m/x"));
}

/// A relative link target is joined to the link's folder by its text and its `..` dropped:
/// `k/../y` from a link in `/deep/a` is `/deep/a/y`, even when `k` is itself a link (which Unix
/// would follow and back up from).
#[cfg(unix)]
#[test]
fn a_relative_link_target_is_joined_by_its_text() {
    let names = [
        ("/m/l", Fake::Link("../deep/a/k/../y".into())),
        ("/deep/a/k", Fake::Link("/far/p".into())),
        ("/deep/a/y", Fake::File),
        ("/far/y", Fake::File),
    ];
    let fs = FakeFs::with(&names);
    assert_eq!(on("/m/l", &fs, &plain_net(true)), local("/deep/a/y"));
    assert!(
        !fs.touched()
            .iter()
            .any(|path| path.starts_with("/far") || path == Path::new("/deep/a/k")),
        "{:?}",
        fs.touched()
    );
    let unix = FakeFs::with(&names);
    assert_eq!(on("/m/l", &unix, &plain_net(false)), local("/far/y"));
}

/// A `..` back out of a junction to a share's spelling never reaches it: the text has no `j`
/// left to open, and a target that is a network path is refused as before.
#[cfg(unix)]
#[test]
fn a_link_to_a_share_is_still_refused_under_windows_rules() {
    let net = plain_net(true);
    let fs = FakeFs::with(&[("/m/j", Fake::Link(r"\\server\share".into()))]);
    assert_eq!(on("/m/j/x", &fs, &net), Walked::Refused);
    assert_eq!(on("/m/j/../x", &fs, &net), local("/m/x"));
    assert!(
        !fs.touched()
            .iter()
            .any(|path| path.to_string_lossy().contains("server")),
        "{:?}",
        fs.touched()
    );
    let relative = FakeFs::with(&[("/m/l", Fake::Link("../../net/host/s".into()))]);
    assert_eq!(on("/m/l/x", &relative, &net), Walked::Refused);
    assert_eq!(
        relative.touched(),
        vec![
            PathBuf::from("/m"),
            PathBuf::from("/m/l"),
            PathBuf::from("/m/l")
        ]
    );
}

/// The same on Windows itself, in drive spellings; a drive-relative target is refused.
#[cfg(windows)]
#[test]
fn drive_paths_drop_dot_dots_by_their_text_and_refuse_a_drive_relative_target() {
    let net = plain_net(true);
    let fs = FakeFs::with(&[
        (r"C:\m\j", Fake::Link(r"C:\deep\a\b".into())),
        (r"C:\m\x", Fake::File),
        (r"C:\m\rel", Fake::Link(r"sub\..\x".into())),
        (r"C:\m\rooted", Fake::Link(r"\x".into())),
        (r"C:\m\driverel", Fake::Link(r"D:x".into())),
    ]);
    assert_eq!(on(r"C:\m\j\..\x", &fs, &net), local(r"C:\m\x"));
    assert_eq!(on(r"C:\m\rel", &fs, &net), local(r"C:\m\x"));
    assert_eq!(on(r"C:\m\rooted\y", &fs, &net), Walked::Refused);
    assert_eq!(on(r"C:\m\driverel\y", &fs, &net), Walked::Refused);
    assert!(!fs.touched().iter().any(|path| path.starts_with(r"C:\deep")));
}

/// Every failure is refused on Windows itself too, in drive spellings, nothing past it looked at:
/// a loop, an unreadable link, a folder the walk may not read; a rejected name is kept.
#[cfg(windows)]
#[test]
fn drive_paths_refuse_every_failure_and_keep_a_rejected_name() {
    let net = plain_net(true);
    let fs = FakeFs::with(&[
        (r"C:\m\a", Fake::Link(r"C:\m\b".into())),
        (r"C:\m\b", Fake::Link(r"C:\m\a".into())),
        (r"C:\m\bad", Fake::BadLink),
        (r"C:\m\locked", Fake::Denied),
        (r"C:\m\docs\*.docx", Fake::BadName),
    ]);
    for path in [r"C:\m\a\x", r"C:\m\bad\x", r"C:\m\locked\x"] {
        assert_eq!(on(path, &fs, &net), Walked::Refused, "{path}");
    }
    assert_eq!(
        on(r"C:\m\docs\*.docx", &fs, &net),
        local(r"C:\m\docs\*.docx")
    );
    assert!(
        !fs.touched()
            .iter()
            .any(|path| path.starts_with(r"C:\m\bad\x") || path.starts_with(r"C:\m\locked\x"))
    );
}
