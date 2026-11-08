//! Whether a link leads out of the matter, judged without following any link: its target is
//! read with `readlinkat` from its folder's descriptor and walked from there one name at a
//! time with `fstatat`, each folder opened from the one before. A name on the way that is
//! itself a link counts as leading out, since where it goes is not judged: so `a -> b/../x`
//! with `b -> .` is out, though `b/../x` reads as `x` (the kernel goes to the matter's
//! parent). A target that ends at another link is judged as that link is, a few deep.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Component, Path, PathBuf};

use rustix::fs::{FileType, readlinkat};

use super::fd::{self, Folder};

/// The most links in a row judged: past it, a chain leads out.
const CHAIN_MAX: usize = 8;

/// Whether the link at `link` leads outside folder `root`, leads nowhere, or can't be judged
/// without following a link: a target that is absolute and not in `root`, that goes above
/// `root`, passes through a link or a missing name, or ends at nothing.
pub(super) fn leads_out(link: &Path, root: &Path) -> bool {
    match inside(link, root, 0) {
        Ok(inside) => !inside,
        Err(error) => {
            let shown = link.display();
            tracing::debug!(link = %shown, %error, "a link that can't be judged leads out");
            true
        }
    }
}

/// The folders from `root` down to where a walk is, each held open with its path.
type Held = Vec<(Folder, PathBuf)>;

/// Goes into folder `name` of the last folder in `held`; false when it is no folder now.
fn descend(held: &mut Held, name: &OsStr) -> io::Result<bool> {
    let Some((at, path)) = held.last() else {
        return Ok(false);
    };
    let path = path.join(name);
    match fd::child(at, name, &path)? {
        Some(folder) => {
            held.push((folder, path));
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Whether the link at `link` leads to an entry inside `root` that is not a link, or to a link
/// that does; `chain` links led here.
fn inside(link: &Path, root: &Path, chain: usize) -> io::Result<bool> {
    let Ok(rel) = link.strip_prefix(root) else {
        return Ok(false);
    };
    let mut names = Vec::new();
    for part in rel.components() {
        let Component::Normal(name) = part else {
            return Ok(false);
        };
        names.push(name);
    }
    let Some((name, folders)) = names.split_last() else {
        return Ok(false);
    };
    let Some((top, _)) = fd::open_dir(root)? else {
        return Ok(false);
    };
    let mut held = vec![(top, root.to_owned())];
    for folder in folders {
        if !descend(&mut held, folder)? {
            return Ok(false);
        }
    }
    let Some((at, _)) = held.last() else {
        return Ok(false);
    };
    let is_link = fd::lstat_at(&at.fd, name)?
        .is_some_and(|stat| FileType::from_raw_mode(stat.st_mode) == FileType::Symlink);
    if !is_link {
        return Ok(false);
    }
    let target = PathBuf::from(OsString::from_vec(
        readlinkat(&at.fd, *name, Vec::new())?.into_bytes(),
    ));
    let steps = if target.is_absolute() {
        let Ok(rest) = target.strip_prefix(root) else {
            return Ok(false);
        };
        held.truncate(1);
        rest
    } else {
        target.as_path()
    };
    let steps: Vec<Component<'_>> = steps.components().collect();
    if steps.is_empty() {
        // A link to `root` itself, or one with an empty target.
        return Ok(target.is_absolute());
    }
    for (at, step) in steps.iter().enumerate() {
        match step {
            Component::CurDir => {}
            Component::ParentDir => {
                if held.len() <= 1 {
                    return Ok(false);
                }
                held.pop();
            }
            Component::Normal(name) if at + 1 == steps.len() => {
                let Some((last, path)) = held.last() else {
                    return Ok(false);
                };
                let kind =
                    fd::lstat_at(&last.fd, name)?.map(|s| FileType::from_raw_mode(s.st_mode));
                return match kind {
                    Some(FileType::Symlink) if chain < CHAIN_MAX => {
                        inside(&path.join(name), root, chain + 1)
                    }
                    None | Some(FileType::Symlink) => Ok(false),
                    Some(_) => Ok(true),
                };
            }
            Component::Normal(name) => {
                if !descend(&mut held, name)? {
                    return Ok(false);
                }
            }
            Component::RootDir | Component::Prefix(_) => return Ok(false),
        }
    }
    Ok(true)
}
