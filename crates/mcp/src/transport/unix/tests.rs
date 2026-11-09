//! The listen socket, its folder checks and the peer check, on real sockets under `/tmp`.

use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use super::super::{Identity, Peer};
use super::folder::{ancestor_refusal, folder_refusal};
use super::{EXISTS, Socket, own_identity, peer_is};

/// A fresh folder with mode 0700 under `/tmp`, by a path without links (on macOS `/tmp` is a
/// link to `/private/tmp`), so `/tmp` resolved.
fn dir(test: &str) -> PathBuf {
    let temp = dunce::canonicalize("/tmp").unwrap();
    let dir = temp.join(format!("clauseo-listen-{test}-{}", std::process::id()));
    drop(crate::write::remove_dir_all(&dir));
    crate::write::create_dir_all(&dir).unwrap();
    crate::write::set_mode(&dir, 0o700).unwrap();
    dir
}

#[test]
fn the_socket_is_fresh_owner_only_and_removed_on_drop() {
    let dir = dir("fresh");
    let path = dir.join("g.sock");
    let mut socket = Socket::make(&path).unwrap();
    let metadata = std::fs::symlink_metadata(&path).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    let listener = socket.take_listener().unwrap();
    let client = UnixStream::connect(&path).unwrap();
    let (served, _) = listener.accept().unwrap();
    assert!(peer_is(&served, Peer::SameUser), "the test's own uid");
    let own = own_identity().expect("this process's identity");
    assert_eq!(own.pid, std::process::id());
    assert!(peer_is(&client, Peer::Process(own)), "the test listened");
    let other_pid = Identity {
        pid: own.pid + 1,
        ..own
    };
    assert!(
        !peer_is(&client, Peer::Process(other_pid)),
        "another pid passed"
    );
    // A process that reused the pid: the same pid, another version.
    let reused = Identity {
        version: own.version + 1,
        ..own
    };
    assert!(
        !peer_is(&client, Peer::Process(reused)),
        "a reused pid passed"
    );
    drop((client, served));
    drop(socket);
    assert!(
        std::fs::symlink_metadata(&path).is_err(),
        "the socket stayed"
    );
    crate::write::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_path_that_exists_is_refused_and_left_alone() {
    let dir = dir("exists");
    let file = dir.join("file.sock");
    crate::write::write(&file, "kept").unwrap();
    assert_eq!(Socket::make(&file).err().as_deref(), Some(EXISTS));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "kept");
    let link = dir.join("link.sock");
    let target = dir.join("target");
    crate::write::symlink(&target, &link).unwrap();
    assert_eq!(Socket::make(&link).err().as_deref(), Some(EXISTS));
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        std::fs::symlink_metadata(&target).is_err(),
        "the link was followed"
    );
    crate::write::remove_dir_all(&dir).unwrap();
}

/// Another socket at the path by the drop (another inode): the guard leaves it.
#[test]
fn a_socket_replaced_before_the_drop_is_left_alone() {
    let dir = dir("replaced");
    let path = dir.join("g.sock");
    let socket = Socket::make(&path).unwrap();
    let ours = std::fs::symlink_metadata(&path).unwrap().ino();
    crate::write::remove_file(&path).unwrap();
    let theirs = crate::write::bind_unix_socket(&path).unwrap();
    let replaced = std::fs::symlink_metadata(&path).unwrap();
    assert!(replaced.file_type().is_socket());
    assert_ne!(replaced.ino(), ours, "the same inode came back");
    drop(socket);
    let left = std::fs::symlink_metadata(&path).expect("the other socket was removed");
    assert_eq!(left.ino(), replaced.ino());
    drop(theirs);
    crate::write::remove_dir_all(&dir).unwrap();
}

/// A folder that is open to others, a link, or reached through a link: refused, and nothing
/// is made in it.
#[test]
fn the_folder_must_be_private_and_reached_without_links() {
    let dir = dir("folder");
    let open = dir.join("open");
    crate::write::create_dir(&open).unwrap();
    crate::write::set_mode(&open, 0o755).unwrap();
    let error = Socket::make(&open.join("g.sock")).err().unwrap();
    assert!(error.contains("its mode is 755, not 700"), "{error}");
    let private = dir.join("private");
    crate::write::create_dir(&private).unwrap();
    crate::write::set_mode(&private, 0o700).unwrap();
    let link = dir.join("link");
    crate::write::symlink(&private, &link).unwrap();
    let error = Socket::make(&link.join("g.sock")).err().unwrap();
    assert!(error.contains("goes through a link"), "{error}");
    let inner = private.join("inner");
    crate::write::create_dir(&inner).unwrap();
    crate::write::set_mode(&inner, 0o700).unwrap();
    let error = Socket::make(&link.join("inner").join("g.sock"))
        .err()
        .unwrap();
    assert!(error.contains("goes through a link"), "{error}");
    assert!(!error.contains(dir.to_str().unwrap()), "a path in {error}");
    let wide = dir.join("wide");
    let under_wide = wide.join("sock");
    crate::write::create_dir_all(&under_wide).unwrap();
    crate::write::set_mode(&under_wide, 0o700).unwrap();
    crate::write::set_mode(&wide, 0o777).unwrap();
    let error = Socket::make(&under_wide.join("g.sock")).err().unwrap();
    assert!(error.contains("others may write to it"), "{error}");
    let file = dir.join("file");
    crate::write::write(&file, "").unwrap();
    let error = Socket::make(&file.join("g.sock")).err().unwrap();
    assert!(error.contains("goes through a file"), "{error}");
    for folder in [&open, &private, &inner, &under_wide] {
        assert!(
            std::fs::symlink_metadata(folder.join("g.sock")).is_err(),
            "a socket was made"
        );
    }
    drop(Socket::make(&inner.join("g.sock")).unwrap());
    crate::write::remove_dir_all(&dir).unwrap();
}

#[test]
fn only_a_private_folder_of_the_guards_own_user_passes() {
    assert_eq!(folder_refusal(0o700, 501, 501), None);
    let refused = |reason: Option<String>| reason.unwrap_or_default();
    assert!(refused(folder_refusal(0o1700, 501, 501)).contains("its mode is 1700, not 700"));
    assert!(refused(folder_refusal(0o700, 0, 501)).contains("owner is uid 0, not the guard's"));
    // On the way: root's or the guard's, and open to no other writer unless sticky.
    assert_eq!(ancestor_refusal(0o755, 0, 501), None);
    assert_eq!(ancestor_refusal(0o1777, 0, 501), None);
    assert_eq!(ancestor_refusal(0o700, 501, 501), None);
    assert!(refused(ancestor_refusal(0o755, 502, 501)).contains("owned by uid 502"));
    assert!(refused(ancestor_refusal(0o775, 501, 501)).contains("others may write"));
    assert!(refused(ancestor_refusal(0o757, 0, 501)).contains("others may write"));
}
