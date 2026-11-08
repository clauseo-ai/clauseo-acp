//! The Git trees' fake machine: an in-memory volume set, a registry value, the processes'
//! users, the record and the log, with failures injected by call line and files held in use.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{Entry, FileId, Inspected, MARKER, Owner, Refused, TreeFs};
use crate::windows::admin_copy::Found;
use crate::windows::sids::Sid;

mod moves;

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

/// A path's owner, whether only administrators can change it, and when it was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Meta {
    pub(super) owner: Owner,
    pub(super) admin_only: bool,
    pub(super) created: u64,
}

/// What a path has when nothing set it: SYSTEM's, administrators only, made at time 0.
const META: Meta = Meta {
    owner: Owner::System,
    admin_only: true,
    created: 0,
};

/// The fake.
#[derive(Debug)]
pub(super) struct FakeTrees {
    /// Every path, normalized ([`key`]).
    pub(super) nodes: BTreeMap<String, Node>,
    /// The paths whose [`Meta`] isn't [`META`].
    pub(super) meta: BTreeMap<String, Meta>,
    /// The text of each marker file, by path.
    pub(super) texts: BTreeMap<String, String>,
    /// The descriptor each marker file was made with, by path.
    pub(super) marker_sddls: BTreeMap<String, String>,
    /// Files and folders a process holds open: none can be deleted, nor a folder above one, or
    /// a held folder, renamed (a held file can be, as a loaded image can).
    pub(super) in_use: BTreeSet<String>,
    /// The paths scheduled for deletion at the restart.
    pub(super) reboot: Vec<String>,
    /// `InstallPath`.
    pub(super) registry: Option<String>,
    /// Paths that are network paths, as prefixes.
    pub(super) network: Vec<String>,
    /// Whether the install folder's chain is administrators-only.
    pub(super) admin_only: bool,
    /// The `FILEVERSION` of every `git.exe`.
    pub(super) version: Option<String>,
    /// The accounts a process runs as.
    pub(super) running: BTreeSet<Sid>,
    /// Whether the process list can't be read.
    pub(super) processes_unreadable: bool,
    /// The record's subkey, when there: its values in the order first written.
    pub(super) record: Option<Vec<(Sid, String)>>,
    /// Every call, as a line.
    pub(super) calls: Vec<String>,
    /// Call lines that fail: a call fails when its line contains one of these.
    pub(super) fail: Vec<String>,
    /// The log's lines and whether each was an error.
    pub(super) logs: Vec<(bool, String)>,
    /// A file the installer replaces as the first file is copied, as an update during a build.
    pub(super) replace_on_copy: Option<String>,
    /// The next inode and creation time.
    next: u64,
}

/// `path` as the fake keys it: `\` separators, lower case.
pub(super) fn key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// Whether the key `at` is `path` or below it.
pub(super) fn under(at: &str, path: &str) -> bool {
    at == path || at.starts_with(&format!("{path}\\"))
}

impl FakeTrees {
    /// A machine with the admin folder and Git in `%ProgramFiles%\Git`, registered.
    pub(super) fn new() -> FakeTrees {
        let mut fake = FakeTrees {
            nodes: BTreeMap::new(),
            meta: BTreeMap::new(),
            texts: BTreeMap::new(),
            marker_sddls: BTreeMap::new(),
            in_use: BTreeSet::new(),
            reboot: Vec::new(),
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
            replace_on_copy: None,
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
        let inode = self.stamp();
        self.nodes.insert(path.to_lowercase(), Node::File(inode));
    }

    /// The next inode or creation time.
    fn stamp(&mut self) -> u64 {
        let now = self.next;
        self.next += 1;
        now
    }

    /// What is at `path`.
    pub(super) fn node(&self, path: &Path) -> Option<&Node> {
        self.nodes.get(&key(path))
    }

    /// The [`Meta`] of `path`.
    pub(super) fn meta_of(&self, path: &Path) -> Meta {
        self.meta.get(&key(path)).copied().unwrap_or(META)
    }

    /// Gives `path` the owner `owner`.
    pub(super) fn own(&mut self, path: &Path, owner: Owner) {
        let meta = Meta {
            owner,
            ..self.meta_of(path)
        };
        self.meta.insert(key(path), meta);
    }

    /// Makes `path` one others than administrators can change.
    pub(super) fn open_to_users(&mut self, path: &Path) {
        let meta = Meta {
            admin_only: false,
            ..self.meta_of(path)
        };
        self.meta.insert(key(path), meta);
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
        let volume = key(path)
            .bytes()
            .next()
            .map_or(0, |letter| u64::from(letter.saturating_sub(b'a')) + 1);
        match self.node(path) {
            None => Ok(None),
            Some(Node::File(inode)) => Ok(Some(FileId {
                volume,
                id: u128::from(*inode),
                size: 100,
                written: *inode,
            })),
            Some(Node::Link) => Err(Refused::Reparse),
            Some(Node::Folder(_)) => Err(Refused::Other("a folder".to_owned())),
        }
    }

    fn file_version(&mut self, _path: &Path) -> Result<Option<String>, String> {
        Ok(self.version.clone())
    }

    fn walk(&mut self, root: &Path) -> Result<Vec<Entry>, Refused> {
        self.call(format!("walk {}", key(root)))
            .map_err(Refused::Other)?;
        if self.node(root) == Some(&Node::Link) {
            return Err(Refused::Reparse);
        }
        let prefix = format!("{}\\", key(root));
        let mut entries = Vec::new();
        for (path, node) in &self.nodes {
            let Some(rel) = path.strip_prefix(&prefix) else {
                continue;
            };
            let meta = self.meta.get(path).copied().unwrap_or(META);
            let (folder, reparse, id) = match node {
                Node::Folder(_) => (true, false, 0),
                Node::File(inode) => (false, false, *inode),
                Node::Link => (false, true, 0),
            };
            entries.push(Entry {
                rel: PathBuf::from(rel),
                folder,
                reparse,
                id: u128::from(id),
                size: if folder { 0 } else { 100 },
                written: id,
                admin_only: meta.admin_only,
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

    fn inspect(&mut self, path: &Path) -> Result<Option<Inspected>, String> {
        self.call(format!("inspect {}", key(path)))?;
        let meta = self.meta_of(path);
        let kind = match self.node(path) {
            None => return Ok(None),
            Some(Node::Folder(_)) => Found::Folder,
            Some(Node::File(_)) => Found::File,
            Some(Node::Link) => Found::Link,
        };
        Ok(Some(Inspected {
            kind,
            owner: meta.owner,
            admin_only: meta.admin_only,
            created: meta.created,
        }))
    }

    fn level(&mut self, path: &Path, sddl: &str) -> Result<(), Refused> {
        self.call(format!("level {}", key(path)))
            .map_err(Refused::Other)?;
        self.parent_folder(path).map_err(Refused::Other)?;
        match self.nodes.get(&key(path)) {
            Some(Node::Link) => Err(Refused::Reparse),
            Some(Node::File(_)) => Err(Refused::Other("a file".to_owned())),
            found => {
                let created = match found {
                    None => self.stamp(),
                    Some(_) => self.meta_of(path).created,
                };
                self.nodes
                    .insert(key(path), Node::Folder(Some(sddl.to_owned())));
                let meta = Meta {
                    owner: Owner::System,
                    admin_only: true,
                    created,
                };
                self.meta.insert(key(path), meta);
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

    fn copy_tree(&mut self, from: &Path, entries: &[Entry], to: &Path) -> Result<(), Refused> {
        moves::put_tree(self, from, entries, to, false)
    }

    fn link_tree(&mut self, from: &Path, entries: &[Entry], to: &Path) -> Result<(), Refused> {
        moves::put_tree(self, from, entries, to, true)
    }

    fn read_marker(&mut self, dir: &Path) -> Result<Option<String>, String> {
        Ok(self.texts.get(&key(&dir.join(MARKER))).cloned())
    }

    fn write_marker(&mut self, dir: &Path, text: &str, sddl: &str) -> Result<(), String> {
        let path = dir.join(MARKER);
        self.call(format!("write_marker {}", key(dir)))?;
        self.parent_folder(&path)?;
        if self.node(&path).is_some() {
            return Err(format!("{} is there", key(&path)));
        }
        self.new_file(&key(&path));
        self.texts.insert(key(&path), text.to_owned());
        self.marker_sddls.insert(key(&path), sddl.to_owned());
        Ok(())
    }

    fn rename(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        self.call(format!("rename {} -> {}", key(from), key(to)))?;
        self.parent_folder(to)?;
        if self.node(to).is_some() {
            return Err(format!("{} is there", key(to)));
        }
        let (from, to) = (key(from), key(to));
        let held_folder =
            self.in_use.contains(&from) && matches!(self.nodes.get(&from), Some(Node::Folder(_)));
        if held_folder
            || self
                .in_use
                .iter()
                .any(|at| at.starts_with(&format!("{from}\\")))
        {
            return Err("ERROR_SHARING_VIOLATION: it, or a file in it, is open".to_owned());
        }
        moves::move_keys(self, &from, &to);
        Ok(())
    }

    fn remove_tree(&mut self, path: &Path) -> Result<(), String> {
        self.call(format!("remove_tree {}", key(path)))?;
        let path = key(path);
        let held: Vec<String> = self
            .in_use
            .iter()
            .filter(|at| under(at, &path))
            .cloned()
            .collect();
        let kept = |at: &str| held.iter().any(|file| under(file, at));
        let gone: Vec<String> = self
            .nodes
            .keys()
            .filter(|at| under(at, &path) && !kept(at))
            .cloned()
            .collect();
        for at in gone {
            self.nodes.remove(&at);
            self.meta.remove(&at);
            self.texts.remove(&at);
            self.marker_sddls.remove(&at);
        }
        if held.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "ERROR_ACCESS_DENIED: {} file(s) in use",
                held.len()
            ))
        }
    }

    fn delete_at_reboot(&mut self, path: &Path) -> Result<(), String> {
        self.call(format!("delete_at_reboot {}", key(path)))?;
        self.reboot.push(key(path));
        Ok(())
    }

    fn place_file(
        &mut self,
        from: &Path,
        dir: &Path,
        temp: &str,
        name: &str,
    ) -> Result<(), Refused> {
        let (temp, to) = (dir.join(temp), dir.join(name));
        self.call(format!("place_file {} -> {}", key(from), key(&to)))
            .map_err(Refused::Other)?;
        match self.node(from) {
            Some(Node::File(_)) => {}
            Some(Node::Link) => return Err(Refused::Reparse),
            _ => return Err(Refused::Other(format!("{} isn't a file", key(from)))),
        }
        self.parent_folder(&to).map_err(Refused::Other)?;
        if matches!(self.node(&to), Some(Node::Folder(_) | Node::Link))
            || self.in_use.contains(&key(&to))
        {
            return Err(Refused::Other(format!("{} can't be replaced", key(&to))));
        }
        self.new_file(&key(&temp));
        let made = self.nodes.remove(&key(&temp));
        self.nodes.insert(key(&to), made.unwrap_or(Node::File(0)));
        self.meta.remove(&key(&to));
        Ok(())
    }

    fn unique(&mut self) -> String {
        format!("{:016x}", self.stamp())
    }

    fn active(&mut self, account: &Sid) -> Result<bool, String> {
        self.call(format!("active {account}"))?;
        if self.processes_unreadable {
            return Err("the process list can't be read".to_owned());
        }
        Ok(self.running.contains(account))
    }

    fn set_record_value(&mut self, account: &Sid, text: &str) -> Result<(), String> {
        self.call(format!("set_record_value {account}"))?;
        let record = self.record.get_or_insert_with(Vec::new);
        match record.iter_mut().find(|(sid, _)| sid == account) {
            Some(value) => value.1 = text.to_owned(),
            None => record.push((account.clone(), text.to_owned())),
        }
        Ok(())
    }

    fn record_names(&mut self) -> Result<Vec<String>, String> {
        Ok(self
            .record
            .iter()
            .flatten()
            .map(|(sid, _)| sid.as_str().to_owned())
            .collect())
    }

    fn delete_record_value(&mut self, name: &str) -> Result<(), String> {
        self.call(format!("delete_record_value {name}"))?;
        if let Some(record) = self.record.as_mut() {
            record.retain(|(sid, _)| !sid.as_str().eq_ignore_ascii_case(name));
        }
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
