//! Another matter's folder (R18): [`PolicyBuilder::hidden_matter`], hidden like a hidden path,
//! and [`PolicyBuilder::other_matter_read`], a read-only grant of a folder or a file in one.
//!
//! The grant is applied after the hidden rules, never writes, and keeps every one of
//! [`REFUSED_IN_OTHER_MATTER`] refused at any depth.

use std::fs;
use std::path::{Path, PathBuf};

use super::PolicyBuilder;
use super::ascii_fold;
use super::build::{all_beneath, beneath_existing, existing, refuse};
use super::{same, under};
use crate::Error;

/// The names that stay refused in a read of another matter, in any letter case (by Unicode case
/// folding: [`ascii_fold`](super::ascii_fold), so `.clauſeo` with a long s too): its git
/// history, its Clauseo data folder, and its agents' config and instructions. The policy refuses
/// a grant with one anywhere in its path; the macOS profile denies reads and writes of each at
/// any depth of a granted tree; Windows tier B leaves each out of the grant (decision 0049).
pub const REFUSED_IN_OTHER_MATTER: [&str; 5] = [".git", ".clauseo", ".claude", ".codex", ".agents"];

/// Another matter's folder, hidden: [`PolicyBuilder::hidden_matter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HiddenMatter {
    pub(super) path: PathBuf,
    pub(super) matter: String,
}

/// A read-only grant of another matter's folder (a tree) or a file in it, canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherMatterRead {
    path: PathBuf,
    is_file: bool,
    root: PathBuf,
    matter: String,
}

impl OtherMatterRead {
    /// The granted folder or file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the grant is one file rather than a tree.
    pub fn is_file(&self) -> bool {
        self.is_file
    }

    /// The other matter's folder, the [`PolicyBuilder::hidden_matter`] root that is or holds
    /// [`OtherMatterRead::path`].
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The other matter, as the host named it to [`PolicyBuilder::hidden_matter`].
    pub fn matter(&self) -> &str {
        &self.matter
    }

    /// Whether `path` is under the grant with one of its names below the grant one of
    /// [`REFUSED_IN_OTHER_MATTER`], at any depth: refused although the grant holds it.
    pub fn refuses(&self, path: &Path) -> bool {
        in_refused_name(path, &self.path)
    }
}

/// Whether `name` is one of [`REFUSED_IN_OTHER_MATTER`] in any letter case, each letter folded
/// as [`ascii_fold::folds_to`] does, on every OS (refusing more is safe).
pub(crate) fn refused_name(name: &std::ffi::OsStr) -> bool {
    REFUSED_IN_OTHER_MATTER
        .iter()
        .any(|refused| ascii_fold::folds_to(name, refused))
}

/// Whether `path` is under `root` with one of its names below `root` one of
/// [`REFUSED_IN_OTHER_MATTER`].
pub(super) fn in_refused_name(path: &Path, root: &Path) -> bool {
    under(path, root)
        && path
            .components()
            .skip(root.components().count())
            .any(|c| refused_name(c.as_os_str()))
}

/// The innermost of `matters` that is or holds `path`.
pub(super) fn matter_of<'m>(path: &Path, matters: &'m [HiddenMatter]) -> Option<&'m HiddenMatter> {
    matters
        .iter()
        .filter(|m| under(path, &m.path))
        .max_by_key(|m| m.path.components().count())
}

/// The hidden paths, by [`all_beneath`], with each of `matters`' folders (by
/// [`beneath_existing`]) after them; and the matters, which [`all_reads`] needs.
pub(super) fn hidden_matters(
    matters: &[(PathBuf, String)],
    hidden: &[PathBuf],
) -> Result<(Vec<PathBuf>, Vec<HiddenMatter>), Error> {
    let mut hidden = all_beneath(hidden, "hidden path")?;
    let mut out = Vec::with_capacity(matters.len());
    for (path, matter) in matters {
        let path = beneath_existing(path, "matter's hidden folder")?;
        hidden.push(path.clone());
        let matter = matter.clone();
        out.push(HiddenMatter { path, matter });
    }
    Ok((hidden, out))
}

/// Each of `b`'s reads of other matters by [`existing`] and [`check_read`], a file exactly
/// when it says so.
pub(super) fn all_reads(
    b: &PolicyBuilder,
    matter: &Path,
    home: Option<&Path>,
    hidden: &[PathBuf],
    matters: &[HiddenMatter],
) -> Result<Vec<OtherMatterRead>, Error> {
    let mut out = Vec::with_capacity(b.other_matter_reads.len());
    for (p, is_file) in &b.other_matter_reads {
        let path = existing(p, "read of another matter")?;
        let other = check_read(&path, matter, home, hidden, matters)?;
        if fs::metadata(&path).is_ok_and(|meta| meta.is_file()) != *is_file {
            let want = if *is_file { "a file" } else { "a folder" };
            return refuse(format!(
                "a read of another matter must be {want}, as granted: {}",
                path.display()
            ));
        }
        out.push(OtherMatterRead {
            path,
            is_file: *is_file,
            root: other.path.clone(),
            matter: other.matter.clone(),
        });
    }
    Ok(out)
}

/// The cross-matter rules, on a canonical `path`; the other matter when they pass.
pub(super) fn check_read<'m>(
    path: &Path,
    matter: &Path,
    home: Option<&Path>,
    hidden: &[PathBuf],
    matters: &'m [HiddenMatter],
) -> Result<&'m HiddenMatter, Error> {
    let shown = path.display();
    let what = "a read of another matter";
    if let Some(name) = path
        .components()
        .map(std::path::Component::as_os_str)
        .find(|name| refused_name(name))
    {
        return refuse(format!(
            "{what} may not be or sit in a {}: {shown}",
            name.to_string_lossy()
        ));
    }
    if home.is_some_and(|home| under(home, path)) {
        return refuse(format!(
            "{what} may not equal or hold the home folder: {shown}"
        ));
    }
    if under(path, matter) || under(matter, path) {
        return refuse(format!(
            "{what} may not equal, hold or sit inside the matter: {shown} and {}",
            matter.display()
        ));
    }
    let Some(other) = matter_of(path, matters) else {
        return refuse(format!(
            "{what} must be in another matter's folder: {shown}"
        ));
    };
    if let Some(h) = hidden
        .iter()
        .find(|h| under(&other.path, h) && !same(&other.path, h))
    {
        return refuse(format!(
            "{what} may not be in a matter inside another hidden path: {shown} and {}",
            h.display()
        ));
    }
    // A hidden path in the matter other than its root, held by the grant or holding it, stays
    // hidden: no grant reaches into it or around it.
    if let Some(h) = hidden
        .iter()
        .find(|h| !same(h, &other.path) && (under(h, path) || under(path, h)))
    {
        return refuse(format!(
            "{what} may not hold or sit in a hidden path: {shown} and {}",
            h.display()
        ));
    }
    Ok(other)
}
