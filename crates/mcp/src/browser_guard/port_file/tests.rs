use std::path::PathBuf;

use std::path::Path;

use super::{Expected, Found, Status, judge, read};

const DAEMON: &str = "daemon port";

fn file(dir: &Path, session: &str) -> PathBuf {
    Expected::daemon(dir, session, 61_100).file
}

/// A folder of its own under the system temp folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "clauseo-mcp-daemon-port-{test}-{}",
            std::process::id()
        ));
        drop(crate::write::remove_dir_all(&path));
        crate::write::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(crate::write::remove_dir_all(&self.0));
    }
}

#[test]
fn a_missing_or_empty_file_is_checked_again() {
    let scratch = Scratch::new("missing");
    let port_file = file(&scratch.0, "s-1");
    assert_eq!(port_file, scratch.0.join("s-1.port"));
    assert_eq!(judge(read(&port_file), 61_100, DAEMON), Status::Pending);
    crate::write::write(&port_file, b"").unwrap();
    assert_eq!(judge(read(&port_file), 61_100, DAEMON), Status::Pending);
}

#[test]
fn the_claimed_port_matches() {
    let scratch = Scratch::new("match");
    let port_file = file(&scratch.0, "s-1");
    crate::write::write(&port_file, b"61100").unwrap();
    assert_eq!(judge(read(&port_file), 61_100, DAEMON), Status::Matches);
    crate::write::write(&port_file, b"61100\n").unwrap();
    assert_eq!(judge(read(&port_file), 61_100, DAEMON), Status::Matches);
}

#[test]
fn another_port_is_refused() {
    let scratch = Scratch::new("mismatch");
    let port_file = file(&scratch.0, "s-1");
    // agent-browser's fallback: an ephemeral port the OS gave.
    crate::write::write(&port_file, b"52731").unwrap();
    assert_eq!(
        judge(read(&port_file), 61_100, DAEMON),
        Status::Refused(
            "agent-browser's daemon port file says 52731, not the claimed port 61100".to_owned()
        )
    );
}

#[test]
fn garbage_is_refused() {
    let scratch = Scratch::new("garbage");
    let port_file = file(&scratch.0, "s-1");
    for garbage in [
        &b"port"[..],
        b"+61100",
        b"061100x",
        b"99999",
        b"-1",
        &[0xff, 0xfe],
    ] {
        crate::write::write(&port_file, garbage).unwrap();
        assert_eq!(
            judge(read(&port_file), 61_100, DAEMON),
            Status::Refused(
                "agent-browser's daemon port file holds no port, not the claimed port 61100"
                    .to_owned()
            ),
            "{garbage:?}"
        );
    }
    // A long file is read only to its first bytes, which hold no port here.
    crate::write::write(&port_file, "6".repeat(4096)).unwrap();
    assert!(matches!(
        judge(read(&port_file), 61_100, DAEMON),
        Status::Refused(_)
    ));
}

#[test]
fn a_folder_at_the_name_is_refused() {
    let scratch = Scratch::new("folder");
    let port_file = file(&scratch.0, "s-1");
    crate::write::create_dir(&port_file).unwrap();
    assert!(matches!(read(&port_file), Found::NotFile));
    assert!(matches!(
        judge(read(&port_file), 61_100, DAEMON),
        Status::Refused(_)
    ));
}

/// A link at the name is refused, even one to a file holding the claimed port.
#[cfg(unix)]
#[test]
fn a_link_at_the_name_is_refused() {
    let scratch = Scratch::new("link");
    let target = scratch.0.join("elsewhere");
    crate::write::write(&target, b"61100").unwrap();
    let port_file = file(&scratch.0, "s-1");
    crate::write::symlink(&target, &port_file).unwrap();
    assert!(matches!(read(&port_file), Found::Link));
    assert_eq!(
        judge(read(&port_file), 61_100, DAEMON),
        Status::Refused(
            "agent-browser's daemon port file is a link, not the claimed port 61100".to_owned()
        )
    );
}

#[test]
fn the_verdict_alone_refuses_a_link_and_an_unreadable_file() {
    assert!(matches!(
        judge(Found::Link, 61_100, DAEMON),
        Status::Refused(_)
    ));
    let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    assert!(matches!(
        judge(Found::Unreadable(error), 61_100, DAEMON),
        Status::Refused(_)
    ));
}

/// The stream server's file sits beside the daemon's, as `<session>.stream`, and its refusal
/// names it.
#[test]
fn the_stream_file_is_read_the_same_way() {
    let scratch = Scratch::new("stream");
    let expected = Expected::stream(&scratch.0, "s-1", 61_100);
    assert_eq!(expected.file, scratch.0.join("s-1.stream"));
    assert_eq!(expected.check(), Status::Pending);
    crate::write::write(&expected.file, b"61100").unwrap();
    assert_eq!(expected.check(), Status::Matches);
    // agent-browser's fallback: the port the OS gave the stream server instead.
    crate::write::write(&expected.file, b"40117").unwrap();
    assert_eq!(
        expected.check(),
        Status::Refused(
            "agent-browser's stream port file says 40117, not the claimed port 61100".to_owned()
        )
    );
}
