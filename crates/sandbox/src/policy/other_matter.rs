//! Another matter's folder (R18): [`PolicyBuilder::hidden_matter`], hidden like a hidden path,
//! and [`PolicyBuilder::other_matter_read`], a read-only grant of a folder or a file in one.
//!
//! The grant is applied after the hidden rules, never writes, and keeps every one of
//! [`REFUSED_IN_OTHER_MATTER`] refused at any depth.

use std::fs;
use std::path::{Path, PathBuf};

use super::build::{existing, refuse};
use super::{Policy, PolicyBuilder, same, under};
use crate::Error;

/// The names that stay refused in a read of another matter, in any ASCII letter case: its git
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
        under(path, &self.path)
            && path
                .components()
                .skip(self.path.components().count())
                .any(|c| refused_name(c.as_os_str()))
    }
}

/// Whether `name` is one of [`REFUSED_IN_OTHER_MATTER`], in any ASCII letter case.
pub(crate) fn refused_name(name: &std::ffi::OsStr) -> bool {
    REFUSED_IN_OTHER_MATTER
        .iter()
        .any(|refused| name.eq_ignore_ascii_case(refused))
}

impl Policy {
    /// The read-only grants of other matters' folders, each passing the rules of
    /// [`PolicyBuilder::other_matter_read`].
    pub fn other_matter_reads(&self) -> &[OtherMatterRead] {
        &self.other_matter_reads
    }

    /// The other matter whose hidden folder holds `path`, if any (the innermost).
    pub fn hidden_matter_of(&self, path: &Path) -> Option<&str> {
        matter_of(path, &self.hidden_matters).map(|m| m.matter.as_str())
    }
}

impl PolicyBuilder {
    /// Another registered matter's folder, `matter` its id as the host names it: a hidden path
    /// ([`PolicyBuilder::hidden`], and [`Policy::hidden`] lists it) that
    /// [`PolicyBuilder::other_matter_read`] may read in, and whose refused paths classify as
    /// [`PathClass::OtherMatter`](crate::PathClass::OtherMatter). It need not exist.
    pub fn hidden_matter(mut self, path: &Path, matter: &str) -> PolicyBuilder {
        self.hidden_matters
            .push((path.to_owned(), matter.to_owned()));
        self
    }

    /// A read-only grant of a folder in another matter, the folder and everything under it, or
    /// of one file in it (`is_file`) (R18), applied after the hidden paths: it reads again what
    /// they hide, and writes nothing. `build` refuses it unless it passes
    /// [`Policy::check_allowed_folder`]'s cross-matter branch and is a file exactly when
    /// `is_file`. A grant of a folder outside every matter is [`PolicyBuilder::allowed_folder`].
    pub fn other_matter_read(mut self, path: &Path, is_file: bool) -> PolicyBuilder {
        self.other_matter_reads.push((path.to_owned(), is_file));
        self
    }
}

/// The innermost of `matters` that is or holds `path`.
pub(super) fn matter_of<'m>(path: &Path, matters: &'m [HiddenMatter]) -> Option<&'m HiddenMatter> {
    matters
        .iter()
        .filter(|m| under(path, &m.path))
        .max_by_key(|m| m.path.components().count())
}

/// Each of `reads` by [`existing`] and [`check_read`], a file exactly when it says so.
pub(super) fn all_reads(
    reads: &[(PathBuf, bool)],
    matter: &Path,
    home: Option<&Path>,
    hidden: &[PathBuf],
    matters: &[HiddenMatter],
) -> Result<Vec<OtherMatterRead>, Error> {
    let mut out = Vec::with_capacity(reads.len());
    for (p, is_file) in reads {
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
        .map(|c| c.as_os_str())
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
    if let Some(h) = hidden
        .iter()
        .find(|h| under(h, path) && !same(h, &other.path))
    {
        return refuse(format!(
            "{what} may not hold a hidden path: {shown} and {}",
            h.display()
        ));
    }
    Ok(other)
}
