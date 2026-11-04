//! The check that agent-browser's listeners sit on the ports the host claimed, built and
//! tested on every OS.
//!
//! agent-browser writes the port each listener got to a file in the socket folder, and when the
//! port it was asked for will not bind it silently takes another and writes that one:
//!
//! - the stream server (every OS): `<socket folder>/<session>.stream`, decimal, written after the
//!   server starts with port fallback allowed (`native/daemon.rs`
//!   `StreamServer::start_without_client`, `stream/mod.rs`); the guard expects
//!   `AGENT_BROWSER_STREAM_PORT`;
//! - the daemon (Windows): `<cwd>\run\<session>.port`, decimal, from a port it derives from the
//!   session name, with an ephemeral fallback (`native/daemon.rs`, the `TcpListener::bind`
//!   fallback); the guard expects `--daemon-port`.
//!
//! A listener on any other port sits outside the ports the jail pins to the session. The guard
//! reads every file after each line it relays, for the session's whole life: the daemon can end
//! (idle timeout, `agent_browser_close`) and start again on another port. For each file:
//!
//! - no file yet, or an empty one: not started, or being written; checked again next time;
//! - the claimed port: fine for now; checked again next time;
//! - anything else (another port, text that is no port, a link, not a regular file,
//!   unreadable): the guard ends agent-browser's tree and exits 1.
//!
//! The file is read without following a link: a link at its name is refused, and the file read
//! must be the one `symlink_metadata` saw. The socket folder is writable by the agent (`<cwd>`
//! is, and on Unix the jail allows the socket folder to the whole session), so this check
//! catches agent-browser's silent fallback; it is not a boundary against the agent, which can
//! write the claimed port into the file itself.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// The most of the file read: a port is at most 5 digits, with room for a line end.
const MAX_BYTES: u64 = 16;

/// One listener's claimed port, and the file agent-browser reports the port it got in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Expected {
    /// The listener, for the refusal: `"daemon port"` or `"stream port"`.
    pub(crate) what: &'static str,
    /// `<socket folder>/<session>.<extension>`.
    pub(crate) file: PathBuf,
    /// The claimed port.
    pub(crate) port: u16,
}

impl Expected {
    /// The daemon's: `<socket folder>/<session>.port`, against `--daemon-port`.
    pub(crate) fn daemon(socket_dir: &Path, session: &str, port: u16) -> Self {
        Self {
            what: "daemon port",
            file: socket_dir.join(format!("{session}.port")),
            port,
        }
    }

    /// The stream server's: `<socket folder>/<session>.stream`, against
    /// `AGENT_BROWSER_STREAM_PORT`.
    pub(crate) fn stream(socket_dir: &Path, session: &str, port: u16) -> Self {
        Self {
            what: "stream port",
            file: socket_dir.join(format!("{session}.stream")),
            port,
        }
    }

    /// Reads the file and judges it.
    pub(crate) fn check(&self) -> Status {
        judge(read(&self.file), self.port, self.what)
    }
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
    /// No port written yet: check again after the next line.
    Pending,
    /// The claimed port, for now: check again after the next line.
    Matches,
    /// Another port, or a file that cannot be trusted: end agent-browser. The reason.
    Refused(String),
}

/// The verdict on what [`read`] found, against the claimed `port` of the listener `what`.
/// Pure.
pub(crate) fn judge(found: Found, port: u16, what: &str) -> Status {
    let refused = |problem: String| {
        Status::Refused(format!(
            "agent-browser's {what} file {problem}, not the claimed port {port}"
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
                Ok(found) if plain && found == port => Status::Matches,
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
