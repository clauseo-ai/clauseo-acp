//! The repo's files and folders as git lists them. Links and backticked paths are looked up here,
//! exactly and case-sensitively, never on disk: a disk lookup ignores case on macOS and Windows,
//! sees ignored local files, and follows `..` out of the repo, so it passes paths that are broken
//! on GitHub and on Linux CI. The disk is read only after a lookup failed, by `on_disk`, to word
//! the message: a path that is on disk but not listed is one git ignores, and creating it is no
//! fix.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Every listed file, every folder above one, and the root as `""`. Repo-relative, `/`-separated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Tree(BTreeSet<String>);

impl Tree {
    /// The tree of `files`, the repo-relative files git lists.
    pub(super) fn new(files: &[String]) -> Tree {
        let mut entries = BTreeSet::from([String::new()]);
        for file in files {
            let mut folder = file.as_str();
            while let Some((parent, _)) = folder.rsplit_once('/') {
                entries.insert(parent.to_owned());
                folder = parent;
            }
            entries.insert(file.clone());
        }
        Tree(entries)
    }

    /// True when `path` (repo-relative, no `.` or `..` parts, no trailing `/`) is a listed file
    /// or a folder that holds one.
    pub(super) fn contains(&self, path: &str) -> bool {
        self.0.contains(path)
    }

    /// The listed entry that differs from `path` only in ASCII case, when there is one.
    pub(super) fn ignoring_case(&self, path: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|entry| *entry != path && entry.eq_ignore_ascii_case(path))
            .map(String::as_str)
    }
}

/// A path that git does not list but that is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OnDisk {
    /// A file: git does not list it, so git ignores it.
    File,
    /// A folder: git lists no file in it (it is ignored, or holds only ignored files or none).
    Folder,
}

impl OnDisk {
    /// What is wrong with `path`, for a message.
    pub(super) fn reason(self, path: &str) -> String {
        match self {
            OnDisk::File => format!("`{path}` is on disk, but git ignores it"),
            OnDisk::Folder => format!("`{path}` is a folder on disk, but git lists no file in it"),
        }
    }
}

/// What `path` (repo-relative, as `Tree::contains` takes it) is on disk under `root`, each part
/// matched with the same case by listing its folder, so a case-insensitive disk does not answer
/// for another name. `None` when it is not there or cannot be read.
pub(super) fn on_disk(root: &Path, path: &str) -> Option<OnDisk> {
    let mut at = root.to_path_buf();
    for part in path.split('/') {
        let listed = fs::read_dir(&at)
            .ok()?
            .filter_map(Result::ok)
            .any(|entry| entry.file_name() == part);
        if !listed {
            return None;
        }
        at.push(part);
    }
    let kind = if fs::metadata(&at).ok()?.is_dir() {
        OnDisk::Folder
    } else {
        OnDisk::File
    };
    Some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_their_folders_and_the_root_are_in_the_tree_and_case_counts() {
        let tree = Tree::new(&["docs/decisions/a.md".to_owned(), "AGENTS.md".to_owned()]);
        for path in [
            "",
            "AGENTS.md",
            "docs",
            "docs/decisions",
            "docs/decisions/a.md",
        ] {
            assert!(tree.contains(path), "{path} is in the tree");
        }
        for path in ["agents.md", "Docs", "docs/decisions/A.md", "docs/dec", "x"] {
            assert!(!tree.contains(path), "{path} is not in the tree");
        }
        assert_eq!(
            tree.ignoring_case("Docs/Decisions/A.md"),
            Some("docs/decisions/a.md")
        );
        assert_eq!(tree.ignoring_case("docs/decisions/a.md"), None);
        assert_eq!(tree.ignoring_case("docs/b.md"), None);
    }

    #[test]
    fn on_disk_matches_each_part_with_its_case() {
        let dir = crate::scratch::Scratch::new("docs-tree-on-disk");
        fs::create_dir_all(dir.join("docs/sub")).expect("create a folder");
        fs::write(dir.join("docs/local.md"), "# Local\n").expect("write a file");
        assert_eq!(on_disk(&dir, "docs/local.md"), Some(OnDisk::File));
        assert_eq!(on_disk(&dir, "docs/sub"), Some(OnDisk::Folder));
        assert_eq!(on_disk(&dir, "docs/LOCAL.md"), None);
        assert_eq!(on_disk(&dir, "Docs/local.md"), None);
        assert_eq!(on_disk(&dir, "docs/gone.md"), None);
    }
}
