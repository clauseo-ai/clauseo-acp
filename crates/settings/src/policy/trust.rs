//! The trust check on a system `policy.json`: the file is read only when an administrator owns
//! it and no one else can change it, so a user-level process cannot plant a machine-wide firm
//! policy (code that runs as another user, security keys turned down).
//!
//! - **macOS and Linux:** the file and its folder are owned by root (uid 0), have no group or
//!   other write bit, and are not symlinks. On Linux the folder's own parent (`/etc`) has no
//!   group or other write bit either. On macOS it is not checked: admin-group users can write
//!   `/Library/Application Support`, and a folder they make there is theirs, so it fails the
//!   owner check. That also means an admin user who makes `/Library/Application Support/Clauseo`
//!   before IT does (no password is asked) gets the firm's file ignored: on macOS, deploy policy
//!   through device management (managed preferences), or create the folder as root first.
//! - **Windows:** the file and `ProgramData\Clauseo` (`ProgramData` itself is the OS's) are
//!   owned by Administrators, SYSTEM or `TrustedInstaller`, and neither is a reparse point. The
//!   file's DACL grants no write-type right to any other SID, and the file has exactly one name,
//!   so no one can hard-link an admin-owned file of their choosing in as `policy.json`. The
//!   folder's rule is narrower: it grants no other SID a right that removes, renames or
//!   re-secures an existing file (`FILE_DELETE_CHILD`, `DELETE`, `WRITE_DAC`, `WRITE_OWNER`,
//!   `GENERIC_WRITE`, `GENERIC_ALL`). A user can add a file there only where none exists, and
//!   such a file is user-owned, so it is ignored. IT still creates the folder itself, owned by
//!   Administrators with a protected ACL: a folder a user made first is theirs, and ignored.
//!
//! The folder is judged before anything in it is opened, and the check and the read go through the
//! same open file. On Unix the folder is opened with `O_DIRECTORY | O_NOFOLLOW` and `fstat`ed: no
//! folder is absent, and a wrong owner or mode ignores the policy. A folder that will not open is
//! judged by `lstat` on its path: a link, something not a folder, or a wrong owner or mode is
//! ignored; a folder that looks trusted, or that cannot be looked at, is unreadable, since the
//! failure may be the reading process's own sandbox (`sandbox-exec`, Landlock), and ignoring it
//! would let a process switch the policy off for itself. Only in a trusted folder is the file
//! opened, with `openat(O_NOFOLLOW)`: no file is absent; a link, a non-regular file or a wrong
//! owner or mode is ignored; a file that cannot be opened is unreadable, since only root put it
//! there (the security keys lock). The bytes are read from that descriptor. On Windows the folder
//! is opened and judged by handle first. A trusted folder still lets users add files, so a refused
//! or odd open of the file (access denied, a directory, a reparse point) ignores it; but a file
//! someone holds open without sharing is retried three times, 100 ms apart, then unreadable (any
//! user can read it, so ignoring would let anyone switch the policy off); an admin-made file Users
//! cannot read is therefore ignored too, and must keep the Users read right the folder's ACL gives
//! it. Its owner, DACL and link count come from that one handle before it is read. So nothing can
//! swap the file between the check and the read. On Unix a second name elsewhere does not matter:
//! only root can add a name inside the folder, and the content is the file's.
//!
//! Left open, by design:
//!
//! - **Windows:** a process started with a restricted token or in an `AppContainer`, lacking
//!   `BUILTIN\Users`, cannot open the folder or read its owner, so it can make the policy read
//!   as ignored for itself. That needs deliberate tooling, unlike holding the file open.
//! - **Linux:** a user can run Clauseo in a user namespace (`unshare -r`, no root needed), where
//!   root's files show another owner, and with a mount namespace present a different `/etc`, so
//!   the policy reads as ignored or absent for that process. A firm that needs the policy
//!   enforced on Linux restricts unprivileged user namespaces: the
//!   `kernel.unprivileged_userns_clone=0` sysctl, or `AppArmor`'s userns restriction, which
//!   Ubuntu 24.04 turns on by default. No `uid_map` check: it would lock settings for every
//!   user under flatpak, snap or a container, and would not stop a tmpfs over `/etc`.
//! - **Everywhere:** the policy file binds the Clauseo builds the firm deploys. Clauseo is open
//!   source, so a user who can run a build of their own is not bound by it; firms enforce which
//!   builds run with application allowlisting (MDM, WDAC, Santa).
//!
//! A file that fails is ignored: it reads as absent, so a planted file locks nothing and changes
//! nothing, and the host log says once per process which source was ignored and why, with no
//! path (0047). No file at all is absent, as before.

use super::{PolicyRead, PolicySource, file};
use crate::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this process has logged an ignored policy file: the warning comes once per process,
/// though policy is read again on a settings change.
static WARNED: AtomicBool = AtomicBool::new(false);

/// A system `policy.json`, read only when the trust check passes (the module doc).
#[derive(Debug, Clone)]
pub struct TrustedFilePolicy {
    path: PathBuf,
    name: String,
    rule: Rule,
    warned: &'static AtomicBool,
}

/// Which trust check applies, by OS.
#[derive(Debug, Clone, Copy)]
enum Rule {
    /// Owned by `owner` (0 in production), no group or other write, no symlink; with `parent`,
    /// the folder's parent has no group or other write either.
    #[cfg(unix)]
    Unix { owner: u32, parent: bool },
    /// Owned by an administrator, the ACL rules of the module doc, no reparse point, one name.
    #[cfg(windows)]
    Windows,
}

/// What the trust check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Trust {
    /// There is no file.
    Missing,
    /// The file may be read: its bytes, read through the handle the check used.
    Trusted(Vec<u8>),
    /// The file is ignored, for this reason.
    Untrusted(Distrust),
}

/// Why a policy file is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Distrust {
    /// The file or its folder is owned by someone other than an administrator.
    Owner,
    /// Someone other than an administrator can change the file or its folder.
    Writable,
    /// The file or its folder is a symlink or a reparse point.
    Link,
    /// The folder (or, on Windows, the file in a trusted folder, where users may add files)
    /// cannot be opened, or is not a folder or a plain file: something planted there.
    Unusable,
    /// The file has more than one name (Windows).
    #[cfg(windows)]
    HardLink,
}

impl Distrust {
    /// The reason kind, as the host log says it.
    pub(crate) fn text(self) -> &'static str {
        match self {
            Distrust::Owner => "owner is not an administrator",
            Distrust::Writable => "writable by non-administrators",
            Distrust::Link => "is a link or reparse point",
            Distrust::Unusable => "cannot be opened, or is not a folder or plain file",
            #[cfg(windows)]
            Distrust::HardLink => "has more than one name",
        }
    }
}

impl TrustedFilePolicy {
    /// The `policy.json` at `path`, trusted when it and its folder are owned by the user id
    /// `owner` (root, 0, in production) and neither is group- or other-writable or a symlink.
    #[cfg(unix)]
    pub fn owned_by(path: PathBuf, owner: u32) -> TrustedFilePolicy {
        TrustedFilePolicy::new(
            path,
            Rule::Unix {
                owner,
                parent: false,
            },
        )
    }

    /// The same policy, also requiring that the folder's parent have no group or other write
    /// bit (Linux's `/etc`).
    #[cfg(unix)]
    #[must_use]
    pub fn with_parent_check(mut self) -> TrustedFilePolicy {
        let Rule::Unix { parent, .. } = &mut self.rule;
        *parent = true;
        self
    }

    /// The `policy.json` at `path`, trusted when it and its folder pass the Windows rules of the
    /// module doc.
    #[cfg(windows)]
    pub fn admin_owned(path: PathBuf) -> TrustedFilePolicy {
        TrustedFilePolicy::new(path, Rule::Windows)
    }

    /// The same policy, logging an ignored file only while `flag` is unset, and setting it. By
    /// default every source shares one flag, so the warning comes once per process; a test
    /// passes its own.
    #[must_use]
    pub fn warn_once_with(mut self, flag: &'static AtomicBool) -> TrustedFilePolicy {
        self.warned = flag;
        self
    }

    fn new(path: PathBuf, rule: Rule) -> TrustedFilePolicy {
        TrustedFilePolicy {
            name: path.display().to_string(),
            path,
            rule,
            warned: &WARNED,
        }
    }

    /// The source, as the host log names it: never its path.
    fn label(&self) -> &'static str {
        match self.rule {
            #[cfg(unix)]
            Rule::Unix { .. } => "the system policy file",
            #[cfg(windows)]
            Rule::Windows => "the ProgramData policy file",
        }
    }

    /// The trust check on the file, with its bytes when it passes.
    fn check(&self) -> io::Result<Trust> {
        match self.rule {
            #[cfg(unix)]
            Rule::Unix { owner, parent } => unix(&self.path, owner, parent),
            #[cfg(windows)]
            Rule::Windows => super::windows::trust(&self.path),
        }
    }
}

impl PolicySource for TrustedFilePolicy {
    fn name(&self) -> &str {
        &self.name
    }

    fn read(&self) -> PolicyRead {
        match self.check() {
            Ok(Trust::Missing) => PolicyRead::Absent,
            Ok(Trust::Trusted(bytes)) => match file::parse(&self.path, &bytes) {
                Ok(values) => PolicyRead::Read(values),
                Err(error) => PolicyRead::Unreadable(error),
            },
            Ok(Trust::Untrusted(why)) => {
                if !self.warned.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        source = self.label(),
                        reason = why.text(),
                        "a policy file is ignored: it is not trusted"
                    );
                }
                PolicyRead::Absent
            }
            Err(source) => PolicyRead::Unreadable(Error::Io {
                path: self.path.clone(),
                source,
            }),
        }
    }
}

/// The macOS and Linux check on `file` (the module doc), with `owner` as the expected user id,
/// reading the file through the descriptor it checked.
#[cfg(unix)]
fn unix(file: &Path, owner: u32, parent: bool) -> io::Result<Trust> {
    use nix::errno::Errno;
    use nix::fcntl::{OFlag, open, openat};
    use nix::sys::stat::Mode;
    use std::fs::{self, File};
    use std::io::Read as _;
    use std::os::unix::fs::MetadataExt as _;

    /// Why `meta` is not trusted, if it is not.
    fn distrust(meta: &fs::Metadata, owner: u32) -> Option<Distrust> {
        if meta.file_type().is_symlink() {
            Some(Distrust::Link)
        } else if meta.uid() != owner {
            Some(Distrust::Owner)
        } else if meta.mode() & 0o022 != 0 {
            Some(Distrust::Writable)
        } else {
            None
        }
    }

    // Nothing is followed, and a FIFO planted before the check cannot block the open.
    let flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK;
    let folder_path = parent_of(file)?;
    let name = file.file_name().ok_or_else(no_name)?;
    // The folder is judged before anything in it is opened.
    let folder = match open(folder_path, flags | OFlag::O_DIRECTORY, Mode::empty()) {
        Ok(fd) => File::from(fd),
        Err(Errno::ENOENT) => return Ok(Trust::Missing),
        Err(error) => return unopened_folder(folder_path, owner, error.into()),
    };
    if let Some(why) = distrust(&folder.metadata()?, owner) {
        return Ok(Trust::Untrusted(why));
    }
    if parent && fs::symlink_metadata(parent_of(folder_path)?)?.mode() & 0o022 != 0 {
        return Ok(Trust::Untrusted(Distrust::Writable));
    }
    // The folder is trusted, so only root put what is in it there: a file that cannot be
    // opened is unreadable (the security keys lock), not ignored.
    let mut policy = match openat(&folder, name, flags, Mode::empty()) {
        Ok(fd) => File::from(fd),
        Err(Errno::ENOENT) => return Ok(Trust::Missing),
        Err(Errno::ELOOP) => return Ok(Trust::Untrusted(Distrust::Link)),
        Err(error) => return Err(error.into()),
    };
    let meta = policy.metadata()?;
    if let Some(why) = distrust(&meta, owner) {
        return Ok(Trust::Untrusted(why));
    }
    if !meta.is_file() {
        return Ok(Trust::Untrusted(Distrust::Unusable));
    }
    let mut bytes = Vec::new();
    policy.read_to_end(&mut bytes)?;
    Ok(Trust::Trusted(bytes))
}

/// What a folder that would not open (with `error`) means, judged by its path. What `lstat`
/// proves was planted (a link, not a folder, a wrong owner or mode) is ignored. A folder that
/// looks trusted, or that cannot even be looked at, is unreadable: the failure may be the
/// reading process's own restriction (a sandbox profile, a Landlock ruleset), and ignoring it
/// would let a process switch the policy off for itself.
#[cfg(unix)]
fn unopened_folder(path: &Path, owner: u32, error: io::Error) -> io::Result<Trust> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(gone) if gone.kind() == io::ErrorKind::NotFound => return Ok(Trust::Missing),
        Err(_) => return Err(error),
    };
    if meta.file_type().is_symlink() {
        Ok(Trust::Untrusted(Distrust::Link))
    } else if !meta.is_dir() {
        Ok(Trust::Untrusted(Distrust::Unusable))
    } else if meta.uid() != owner {
        Ok(Trust::Untrusted(Distrust::Owner))
    } else if meta.mode() & 0o022 != 0 {
        Ok(Trust::Untrusted(Distrust::Writable))
    } else {
        Err(error)
    }
}

/// The folder `path` sits in.
pub(crate) fn parent_of(path: &Path) -> io::Result<&Path> {
    path.parent().ok_or_else(no_name)
}

/// The error for a policy path that names no file in a folder.
fn no_name() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "a policy file path names no file in a folder",
    )
}
