//! The Git trees' fake machine: an in-memory volume set, a registry value, the processes'
//! users, the record and the log, with failures injected by call line.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{Entry, FileId, Refused, TreeFs};
use crate::windows::admin_copy::Found;
use crate::windows::sids::Sid;

/// `%ProgramFiles%` in the fake.
pub(super) const PROGRAM_FILES: &str = r"C:\Program Files";
/// The admin folder in the fake.
pub(super) const ADMIN: &str = r"C:\Program Files\Clauseo\Sandbox";
/// The default Git install.
pub(super) const GIT_ROOT: &str = r"C:\Program Files\Git";
/// The files of the fake's Git, from its root (folders are made for them).
pub(super) const GIT_FILES: [&str; 6] = [
    r"usr\bin\msys-2.0.dll",
    r"usr\bin\bash.exe",
    r"usr\bin\sh.exe",
    r"cmd\git.exe",
    r"mingw64\bin\git.exe",
    r"etc\fstab",
];

/// What is at a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Node {
    /// A folder, with the DACL `level` gave it (`None`: inherited).
    Folder(Option<String>),
    /// A regular file: its inode, which its hard links share.
    File(u64),
    /// A reparse point.
    Link,
}

/// The fake.
#[derive(Debug)]
pub(super) struct FakeTrees {
    /// Every path, normalized ([`key`]).
    pub(super) nodes: BTreeMap<String, Node>,
    /// `InstallPath`.
    pub(super) registry: Option<String>,
    /// Paths that are network paths, as prefixes.
    pub(super) network: Vec<String>,
    /// Whether the install folder is administrators-only.
    pub(super) admin_only: bool,
    /// The `FILEVERSION` of every `git.exe`.
    pub(super) version: Option<String>,
    /// The accounts a process runs as.
    pub(super) running: BTreeSet<Sid>,
    /// Whether the process list can't be read.
    pub(super) processes_unreadable: bool,
    /// The record's subkey, when there.
    pub(super) record: Option<Vec<(Sid, String)>>,
    /// Every call, as a line.
    pub(super) calls: Vec<String>,
    /// Call lines that fail: a call fails when its line contains one of these.
    pub(super) fail: Vec<String>,
    /// The log's lines and whether each was an error.
    pub(super) logs: Vec<(bool, String)>,
    /// A file the installer replaces as the first link is made, as an update during a build.
    pub(super) replace_on_link: Option<String>,
    /// The next inode.
    next: u64,
}

/// `path` as the fake keys it: `\` separators, lower case.
pub(super) fn key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

impl FakeTrees {
    /// A machine with the admin folder and Git in `%ProgramFiles%\Git`, registered.
    pub(super) fn new() -> FakeTrees {
        let mut fake = FakeTrees {
            nodes: BTreeMap::new(),
            registry: Some(GIT_ROOT.to_owned()),
            network: Vec::new(),
            admin_only: true,
            version: Some("2.47.1.1".to_owned()),
            running: BTreeSet::new(),
            processes_unreadable: false,
            record: None,
            calls: Vec::new(),
            fail: Vec::new(),
            logs: Vec::new(),
            replace_on_link: None,
            next: 1,
        };
        fake.folders(ADMIN);
        fake.install(GIT_ROOT);
        fake.folders(r"C:\Users\lawyer");
        fake
    }

    /// Makes `path` and every folder above it.
    pub(super) fn folders(&mut self, path: &str) {
        let mut at = String::new();
        for part in path.split('\\') {
            if !at.is_empty() {
                at.push('\\');
            }
            at.push_str(part);
            self.nodes
                .entry(at.to_lowercase())
                .or_insert(Node::Folder(None));
        }
    }

    /// Puts a Git at `root`: [`GIT_FILES`], each a new file.
    pub(super) fn install(&mut self, root: &str) {
        for file in GIT_FILES {
            let path = format!(r"{root}\{file}");
            let parent = path.rsplit_once('\\').map_or("", |(dir, _)| dir).to_owned();
            self.folders(&parent);
            self.new_file(&path);
        }
    }

    /// Puts a new file at `path` (another inode), as an installer replacing it does.
    pub(super) fn new_file(&mut self, path: &str) {
        let inode = self.next;
        self.next += 1;
        self.nodes.insert(path.to_lowercase(), Node::File(inode));
    }

    /// What is at `path`.
    pub(super) fn node(&self, path: &Path) -> Option<&Node> {
        self.nodes.get(&key(path))
    }

    /// The paths below `dir`, from it, lower case.
    pub(super) fn below(&self, dir: &Path) -> Vec<String> {
        let prefix = format!("{}\\", key(dir));
        self.nodes
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix).map(str::to_owned))
            .collect()
    }

    /// The calls whose line starts with `op`.
    pub(super) fn count(&self, op: &str) -> usize {
        self.calls
            .iter()
            .filter(|line| line.starts_with(&format!("{op} ")))
            .count()
    }

    /// Records the call `line`; an injected failure is its error.
    fn call(&mut self, line: impl Into<String>) -> Result<(), String> {
        let line = line.into();
        let failed = self
            .fail
            .iter()
            .any(|pattern| line.contains(pattern.as_str()));
        self.calls.push(line.clone());
        if failed {
            return Err(format!("injected: {line}"));
        }
        Ok(())
    }

    /// The parent of `path` must be a folder.
    fn parent_folder(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().map(key).unwrap_or_default();
        match self.nodes.get(&parent) {
            Some(Node::Folder(_)) => Ok(()),
            _ => Err(format!("no folder above {}", key(path))),
        }
    }

    /// The volume of `path`: its drive letter's place in the alphabet.
    fn volume_of(path: &Path) -> u64 {
        key(path)
            .bytes()
            .next()
            .map_or(0, |letter| u64::from(letter.saturating_sub(b'a')) + 1)
    }
}

impl TreeFs for FakeTrees {
    fn admin_dir(&mut self) -> Result<PathBuf, String> {
        self.call("admin_dir -".to_owned())?;
        Ok(PathBuf::from(ADMIN))
    }

    fn registry_install(&mut self) -> Result<Option<String>, String> {
        self.call("registry_install -".to_owned())?;
        Ok(self.registry.clone())
    }

    fn program_files(&mut self) -> Result<PathBuf, String> {
        Ok(PathBuf::from(PROGRAM_FILES))
    }

    fn profiles_dir(&mut self) -> Result<PathBuf, String> {
        Ok(PathBuf::from(r"C:\Users"))
    }

    fn is_network(&mut self, path: &Path) -> Result<bool, String> {
        let path = key(path);
        Ok(path.starts_with(r"\\")
            || self
                .network
                .iter()
                .any(|prefix| path.starts_with(&prefix.to_lowercase())))
    }

    fn plain_chain(&mut self, dir: &Path) -> Result<(), Refused> {
        let mut at = String::new();
        for part in key(dir).split('\\') {
            if !at.is_empty() {
                at.push('\\');
            }
            at.push_str(part);
            match self.nodes.get(&at) {
                Some(Node::Folder(_)) => {}
                Some(Node::Link) => return Err(Refused::Reparse),
                _ => return Err(Refused::Other(format!("{at} isn't a folder"))),
            }
        }
        Ok(())
    }

    fn admin_only(&mut self, _dir: &Path) -> Result<bool, String> {
        Ok(self.admin_only)
    }

    fn file_id(&mut self, path: &Path) -> Result<Option<FileId>, Refused> {
        self.call(format!("file_id {}", key(path)))
            .map_err(Refused::Other)?;
        match self.node(path) {
            None => Ok(None),
            Some(Node::File(inode)) => Ok(Some(FileId {
                volume: Self::volume_of(path),
                id: u128::from(*inode),
                size: 100,
                written: 7,
            })),
            Some(Node::Link) => Err(Refused::Reparse),
            Some(Node::Folder(_)) => Err(Refused::Other("a folder".to_owned())),
        }
    }

    fn file_version(&mut self, _path: &Path) -> Result<Option<String>, String> {
        Ok(self.version.clone())
    }

    fn volume(&mut self, path: &Path) -> Result<u64, String> {
        Ok(Self::volume_of(path))
    }

    fn walk(&mut self, root: &Path) -> Result<Vec<Entry>, Refused> {
        self.call(format!("walk {}", key(root)))
            .map_err(Refused::Other)?;
        let prefix = format!("{}\\", key(root));
        let mut entries = Vec::new();
        for (path, node) in &self.nodes {
            let Some(rel) = path.strip_prefix(&prefix) else {
                continue;
            };
            let (folder, id) = match node {
                Node::Folder(_) => (true, 0),
                Node::File(inode) => (false, u128::from(*inode)),
                Node::Link => return Err(Refused::Reparse),
            };
            entries.push(Entry {
                rel: PathBuf::from(rel),
                folder,
                id,
            });
        }
        Ok(entries)
    }

    fn found(&mut self, path: &Path) -> Result<Found, String> {
        Ok(match self.node(path) {
            None => Found::Missing,
            Some(Node::Folder(_)) => Found::Folder,
            Some(Node::File(_)) => Found::File,
            Some(Node::Link) => Found::Link,
        })
    }

    fn level(&mut self, path: &Path, sddl: &str) -> Result<(), Refused> {
        self.call(format!("level {}", key(path)))
            .map_err(Refused::Other)?;
        self.parent_folder(path).map_err(Refused::Other)?;
        match self.nodes.get(&key(path)) {
            Some(Node::Link) => Err(Refused::Reparse),
            Some(Node::File(_)) => Err(Refused::Other("a file".to_owned())),
            _ => {
                self.nodes
                    .insert(key(path), Node::Folder(Some(sddl.to_owned())));
                Ok(())
            }
        }
    }

    fn entries(&mut self, dir: &Path) -> Result<Vec<String>, String> {
        Ok(self
            .below(dir)
            .into_iter()
            .filter(|rel| !rel.contains('\\'))
            .collect())
    }

    fn make_dir(&mut self, path: &Path) -> Result<(), String> {
        self.call(format!("make_dir {}", key(path)))?;
        self.parent_folder(path)?;
        if self.node(path).is_some() {
            return Err(format!("{} is there", key(path)));
        }
        self.nodes.insert(key(path), Node::Folder(None));
        Ok(())
    }

    fn hard_link(&mut self, existing: &Path, link: &Path) -> Result<(), String> {
        self.call(format!("hard_link {} -> {}", key(existing), key(link)))?;
        if let Some(replaced) = self.replace_on_link.take() {
            self.new_file(&replaced);
        }
        self.parent_folder(link)?;
        let Some(Node::File(inode)) = self.node(existing).cloned() else {
            return Err(format!("{} isn't a file", key(existing)));
        };
        if Self::volume_of(existing) != Self::volume_of(link) {
            return Err("ERROR_NOT_SAME_DEVICE".to_owned());
        }
        if self.node(link).is_some() {
            return Err(format!("{} is there", key(link)));
        }
        self.nodes.insert(key(link), Node::File(inode));
        Ok(())
    }

    fn copy_file(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        self.call(format!("copy_file {} -> {}", key(from), key(to)))?;
        self.parent_folder(to)?;
        if !matches!(self.node(from), Some(Node::File(_))) || self.node(to).is_some() {
            return Err("can't copy".to_owned());
        }
        self.new_file(&key(to));
        Ok(())
    }

    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        self.call(format!("rename {} -> {}", key(from), key(to)))?;
        if self.node(to).is_some() {
            return Err(format!("{} is there", key(to)));
        }
        let (from, to) = (key(from), key(to));
        let moved: Vec<String> = self
            .nodes
            .keys()
            .filter(|path| **path == from || path.starts_with(&format!("{from}\\")))
            .cloned()
            .collect();
        for path in moved {
            if let Some(node) = self.nodes.remove(&path) {
                let rest = path.get(from.len()..).unwrap_or_default();
                self.nodes.insert(format!("{to}{rest}"), node);
            }
        }
        Ok(())
    }

    fn remove_tree(&mut self, path: &Path) -> Result<(), String> {
        self.call(format!("remove_tree {}", key(path)))?;
        let path = key(path);
        self.nodes
            .retain(|at, _| *at != path && !at.starts_with(&format!("{path}\\")));
        Ok(())
    }

    fn active(&mut self, account: &Sid) -> Result<bool, String> {
        self.call(format!("active {account}"))?;
        if self.processes_unreadable {
            return Err("the process list can't be read".to_owned());
        }
        Ok(self.running.contains(account))
    }

    fn write_record(&mut self, values: &[(Sid, String)]) -> Result<(), String> {
        self.call("write_record -".to_owned())?;
        self.record = Some(values.to_vec());
        Ok(())
    }

    fn delete_record(&mut self) -> Result<(), String> {
        self.call("delete_record -".to_owned())?;
        self.record = None;
        Ok(())
    }

    fn log(&mut self, error: bool, line: &str) {
        self.logs.push((error, line.to_owned()));
    }
}
