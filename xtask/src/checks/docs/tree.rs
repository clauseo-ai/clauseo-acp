//! The repo's files and folders as git lists them. Links and backticked paths are looked up here,
//! exactly and case-sensitively, never on disk: a disk lookup ignores case on macOS and Windows,
//! sees ignored local files, and follows `..` out of the repo, so it passes paths that are broken
//! on GitHub and on Linux CI.

use std::collections::BTreeSet;

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
    }
}
