//! Windows only in use, built and tested everywhere: the check that agent-browser's daemon
//! listens on the port the host claimed.
//!
//! On Windows agent-browser's daemon listens on loopback TCP at a port it derives from the
//! session name (`connection.rs` `get_port_for_session`), and when that port will not bind it
//! silently takes an ephemeral one instead (`native/daemon.rs`, the `TcpListener::bind` fallback)
//! and writes the port it got to `<socket folder>\<session>.port`. The host draws the session
//! name so the derived port is one it claimed in the jail's range, and passes that port as
//! `--daemon-port`. A daemon on any other port would sit outside the ports the jail pins to the
//! session, so the guard reads the `.port` file after each reply it relays:
//!
//! - no file yet, or an empty one: the daemon has not started (it starts on the agent's first
//!   browser call) or is writing it, so the guard checks again after the next reply;
//! - the claimed port: verified, and the guard reads no more;
//! - anything else (another port, text that is no port, a link, not a regular file, unreadable):
//!   the guard kills agent-browser and exits 1.
//!
//! The file is read without following a link: a link at its name is refused, and the file read
//! must be the one `symlink_metadata` saw.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// The most of the file read: a port is at most 5 digits, with room for a line end.
const MAX_BYTES: u64 = 16;

/// The daemon port the host claimed, and the file agent-browser reports its port in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Expected {
    /// `<cwd>\run\<session>.port`.
    pub(crate) file: PathBuf,
    /// `--daemon-port`.
    pub(crate) port: u16,
}

/// `<socket folder>/<session>.port`.
pub(crate) fn file(socket_dir: &Path, session: &str) -> PathBuf {
    socket_dir.join(format!("{session}.port"))
}

/// What reading the `.port` file found.
#[derive(Debug)]
pub(crate) enum Found {
    /// No file at the name.
    Missing,
    /// A link at the name, or a file swapped for another between the look and the read.
    Link,
    /// Something at the name that is no regular file (a folder, a device).
    NotFile,
    /// The file's first bytes.
    Bytes(Vec<u8>),
    /// It could not be read.
    Unreadable(io::Error),
}

/// What the guard does after a check.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// The daemon has not written its port yet: check again after the next reply.
    Pending,
    /// The daemon is on the claimed port: stop checking.
    Verified,
    /// The daemon is elsewhere, or the file cannot be trusted: kill agent-browser. The reason.
    Refused(String),
}

/// The verdict on what [`read`] found, against the claimed `port`. Pure.
pub(crate) fn judge(found: Found, port: u16) -> Status {
    let refused = |what: String| {
        Status::Refused(format!(
            "agent-browser's daemon port file {what}, not the claimed port {port}"
        ))
    };
    match found {
        Found::Missing => Status::Pending,
        Found::Link => refused("is a link".to_owned()),
        Found::NotFile => refused("is not a regular file".to_owned()),
        Found::Unreadable(error) => refused(format!("cannot be read ({error})")),
        // agent-browser writes the file in one call (`fs::write`); empty, it is being written.
        Found::Bytes(bytes) if bytes.is_empty() => Status::Pending,
        Found::Bytes(bytes) => {
            let text = std::str::from_utf8(&bytes).unwrap_or_default().trim();
            let plain = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
            match text.parse::<u16>() {
                Ok(found) if plain && found == port => Status::Verified,
                Ok(found) if plain => refused(format!("says {found}")),
                _ => refused("holds no port".to_owned()),
            }
        }
    }
}

/// Reads `file` without following a link: `symlink_metadata` first (a link, or anything but a
/// regular file, is refused), then the open, then a check that the file opened is the one
/// looked at (on Windows the open itself takes no reparse point,
/// `FILE_FLAG_OPEN_REPARSE_POINT`). At most [`MAX_BYTES`] are read.
pub(crate) fn read(file: &Path) -> Found {
    let looked = match std::fs::symlink_metadata(file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Found::Missing,
        Err(error) => return Found::Unreadable(error),
    };
    if looked.file_type().is_symlink() {
        return Found::Link;
    }
    if !looked.is_file() {
        return Found::NotFile;
    }
    let opened = match crate::write::open_read_no_follow(file) {
        Ok(opened) => opened,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Found::Missing,
        Err(error) => return Found::Unreadable(error),
    };
    match opened.metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() || !same_file(&looked, &metadata) => {
            return Found::Link;
        }
        Ok(metadata) if !metadata.is_file() => return Found::NotFile,
        Ok(_) => {}
        Err(error) => return Found::Unreadable(error),
    }
    let mut bytes = Vec::new();
    match opened.take(MAX_BYTES).read_to_end(&mut bytes) {
        Ok(_) => Found::Bytes(bytes),
        Err(error) => Found::Unreadable(error),
    }
}

/// Whether the file opened is the one `symlink_metadata` saw. Unix: the same device and
/// inode, since the open follows a link swapped in between. Windows: the open took no reparse
/// point (`FILE_FLAG_OPEN_REPARSE_POINT`), so the handle is the name's own file, and a reparse
/// point opened is caught by `is_symlink` on the handle's metadata; nothing more to compare.
#[cfg(unix)]
fn same_file(looked: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    looked.dev() == opened.dev() && looked.ino() == opened.ino()
}

#[cfg(not(unix))]
fn same_file(_looked: &std::fs::Metadata, _opened: &std::fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests;
