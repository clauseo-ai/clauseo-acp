//! The matter folder's root, from which the guard's scan of the matter's held files opens every
//! folder (`pre_write_guard/names.rs`). A local matter: its canonical path, each folder opened
//! from its volume with no link followed ([`open_path`]). A matter on a share or a mapped drive
//! on Windows, which canonicalises to a UNC path [`open_path`] refuses as not local: its root as
//! the lawyer stored it, admitted by the share-matter exception's strict rule
//! ([`share_matter_root`]) and opened as that one text (the lawyer configured it; a handle that
//! is a link itself is refused), then every
//! name below it opened from that handle with no link followed and the same reparse-point
//! refusal. A path an agent names elsewhere on a share stays refused; so does a share root the
//! rule does not admit, never canonicalised (that would reach the server).

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::{Access, NoFollow, log_refusal, open_path};
use crate::network_paths::{NetEnv, under_share_matter};

/// Opens a share matter's root as stored, to list (a seam: CI mounts no share).
pub(crate) type OpenShareRoot<'a> = dyn Fn(&Path) -> io::Result<NoFollow> + 'a;

/// Where the held-file scan opens the matter's folders from.
pub(crate) struct MatterRoot<'a> {
    form: Form,
    open_share: &'a OpenShareRoot<'a>,
}

/// A [`MatterRoot`]'s kind.
enum Form {
    /// A local matter's canonical path (as given when it can't be canonicalised).
    Local(PathBuf),
    /// A share matter's root as stored, and that root opened.
    Share { stored: PathBuf, root: NoFollow },
    /// Nothing to open: a share root the rule does not admit, or one that would not open.
    Closed,
}

impl MatterRoot<'static> {
    /// The root of the matter `folder` (as the guard got it, the matter's stored root) under
    /// `net`'s rules.
    pub(crate) fn new(folder: &Path, net: &NetEnv) -> Self {
        MatterRoot::with(folder, net, &open_share_root)
    }
}

impl<'a> MatterRoot<'a> {
    /// [`Self::new`], a share matter's root opened by `open_share`.
    pub(crate) fn with(folder: &Path, net: &NetEnv, open_share: &'a OpenShareRoot<'a>) -> Self {
        let form = if share_matter_root(folder, net) {
            match open_root(open_share, folder) {
                Ok(root) => Form::Share {
                    stored: folder.to_path_buf(),
                    root,
                },
                Err(error) => {
                    log_refusal(&error);
                    Form::Closed
                }
            }
        } else if net.windows && net.is_network(folder) {
            tracing::debug!(
                kind = "share_root_not_admitted",
                "a matter root on the network the share rule does not admit; not scanned"
            );
            Form::Closed
        } else {
            Form::Local(dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf()))
        };
        Self { form, open_share }
    }

    /// The folder `relative` (plain names) under the root, opened for `access` with no link
    /// followed below the root; the root itself, for a share matter, opened again as stored.
    pub(crate) fn open(&self, relative: &Path, access: Access) -> io::Result<NoFollow> {
        match &self.form {
            Form::Local(root) => open_path(&root.join(relative), access),
            Form::Share { stored, root } => {
                let names = plain_names(relative)?;
                let Some((last, folders)) = names.split_last() else {
                    return open_root(self.open_share, stored);
                };
                let mut at: Option<NoFollow> = None;
                for name in folders {
                    let next = at.as_ref().unwrap_or(root).open_at(name, Access::Folder)?;
                    at = Some(next);
                }
                at.as_ref().unwrap_or(root).open_at(last, access)
            }
            Form::Closed => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "a matter root that is not opened",
            )),
        }
    }
}

/// The share root `stored` opened by `open_share` (following links, as DFS needs), refused when
/// the handle it gives is a link itself (a name-surrogate reparse tag): the system opened the link
/// rather than a folder, and nothing below it is listed.
fn open_root(open_share: &OpenShareRoot<'_>, stored: &Path) -> io::Result<NoFollow> {
    let root = open_share(stored)?;
    if root.is_link()? {
        return Err(super::link_on_the_way());
    }
    Ok(root)
}

/// `relative`'s names, each plain; an error for any other part.
fn plain_names(relative: &Path) -> io::Result<Vec<&OsStr>> {
    relative
        .components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a path of plain names",
            )),
        })
        .collect()
}

/// Whether `folder` is a share matter's root by the share-matter exception's rule, with
/// Windows' rules only (elsewhere a share's mount opens as any local folder): a network location
/// that is at itself by [`under_share_matter`] (no `.` or `..`, ordinal names), in one of the
/// two forms the system opens as written, `\\server\share\…` or `X:\…` (never `\\?\`, `\\.\`, `/`),
/// with no empty name, none the system would trim (a trailing dot or space) and no character a
/// file name can't hold.
pub(crate) fn share_matter_root(folder: &Path, net: &NetEnv) -> bool {
    if !net.windows || !under_share_matter(folder, folder, net) {
        return false;
    }
    let Some(text) = folder.to_str() else {
        return false;
    };
    let plain = |name: &&str| {
        !name.is_empty()
            && *name != "."
            && *name != ".."
            && !name.ends_with(['.', ' '])
            && !name.contains(['/', ':', '?', '*', '<', '>', '"', '|'])
    };
    let trimmed = text.strip_suffix('\\').unwrap_or(text);
    let names: Vec<&str> = trimmed.split('\\').collect();
    match names.as_slice() {
        ["", "", server, share, rest @ ..] => {
            plain(server) && plain(share) && rest.iter().all(plain)
        }
        [drive, rest @ ..] => {
            let bytes = drive.as_bytes();
            bytes.len() == 2
                && bytes.first().is_some_and(u8::is_ascii_alphabetic)
                && bytes.get(1) == Some(&b':')
                && text.as_bytes().get(2) == Some(&b'\\')
                && rest.iter().all(plain)
        }
        [] => false,
    }
}

/// The system's share-root open: `CreateFileW` on Windows (`super::windows_share`).
#[cfg(windows)]
fn open_share_root(path: &Path) -> io::Result<NoFollow> {
    super::windows_share::open(path).map(NoFollow)
}

/// No share root is opened off Windows ([`share_matter_root`] needs Windows' rules).
#[cfg(not(windows))]
fn open_share_root(_path: &Path) -> io::Result<NoFollow> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no share matter root off Windows",
    ))
}
