//! The tests' fakes: a file system in memory, a sink, a session, and the helpers.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use super::super::{Fs, Kind, Meta, Stamp};

/// The fake volume: every entry by path, with its kind and inode.
#[derive(Default)]
struct Disk {
    entries: BTreeMap<PathBuf, (Kind, u64)>,
    next: u64,
    /// Paths on a network volume.
    network: Vec<PathBuf>,
    /// Paths on a local volume of their own, with its device.
    volumes: Vec<(PathBuf, u64)>,
    /// Renames into the set-aside folder fail with this OS error.
    fail_moves: Option<i32>,
    /// Each link's target, absolute.
    targets: BTreeMap<PathBuf, PathBuf>,
    /// Renames in place and link removals fail too.
    fail_in_place: bool,
    /// Listing this folder panics.
    panic_on: Option<PathBuf>,
    /// The last random number given.
    random: u64,
    /// Each folder's change count, its stamp: an entry made, renamed or removed in it moves it.
    stamps: BTreeMap<PathBuf, i64>,
    /// How many times `dated` read an entry.
    dated: u64,
    /// The canary's writes, by path.
    touched: Vec<PathBuf>,
    /// The next this many canary writes fail.
    fail_touches: u32,
    /// How many folders were listed.
    lists: u64,
    /// Listing this folder waits until the flag is set.
    block: Option<(PathBuf, Arc<AtomicBool>)>,
}

impl Disk {
    /// Moves the stamp of the folder holding `path`.
    fn changed(&mut self, path: &Path) {
        if let Some(dir) = path.parent() {
            *self.stamps.entry(dir.to_owned()).or_default() += 1;
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct FakeFs(Arc<Mutex<Disk>>);

pub(super) const MATTER: &str = "/m";
pub(super) const SET_ASIDE: &str = "/data/sandbox/git-set-aside";

impl FakeFs {
    pub(super) fn new() -> FakeFs {
        let fs = FakeFs::default();
        fs.dir(MATTER);
        fs
    }

    fn add(&self, path: &str, kind: Kind) {
        let mut disk = self.0.lock().unwrap();
        disk.next += 1;
        let id = disk.next;
        disk.entries.insert(PathBuf::from(path), (kind, id));
        disk.changed(Path::new(path));
    }

    pub(super) fn dir(&self, path: &str) {
        self.add(path, Kind::Dir);
    }

    pub(super) fn file(&self, path: &str) {
        self.add(path, Kind::File);
    }

    /// A symbolic link at `path` to `target`.
    pub(super) fn link(&self, path: &str, target: &str) {
        self.add(path, Kind::Link);
        let mut disk = self.0.lock().unwrap();
        disk.targets
            .insert(PathBuf::from(path), PathBuf::from(target));
    }

    /// A git repository's `.git` folder at `path`.
    pub(super) fn repo(&self, path: &str) {
        self.dir(path);
        self.file(&format!("{path}/HEAD"));
        self.dir(&format!("{path}/hooks"));
    }

    /// A bare repository's layout in folder `path`, which exists.
    pub(super) fn bare(&self, path: &str) {
        self.dir(&format!("{path}/objects"));
        self.dir(&format!("{path}/refs"));
        self.file(&format!("{path}/HEAD"));
    }

    /// How many entries `dated` has read.
    pub(super) fn dated_reads(&self) -> u64 {
        self.0.lock().unwrap().dated
    }

    /// How many folders have been listed.
    pub(super) fn listings(&self) -> u64 {
        self.0.lock().unwrap().lists
    }

    /// The canary's writes.
    /// Fails the next `n` canary writes.
    pub(super) fn fail_touches(&self, n: u32) {
        self.0.lock().unwrap().fail_touches = n;
    }

    pub(super) fn touched(&self) -> Vec<PathBuf> {
        self.0.lock().unwrap().touched.clone()
    }

    /// Removes the entry at `path` and everything below it.
    pub(super) fn remove(&self, path: &str) {
        let mut disk = self.0.lock().unwrap();
        disk.entries.retain(|p, _| !p.starts_with(path));
        disk.changed(Path::new(path));
    }

    pub(super) fn exists(&self, path: &str) -> bool {
        self.0.lock().unwrap().entries.contains_key(Path::new(path))
    }

    /// Marks `path` and below a network volume.
    pub(super) fn network(&self, path: &str) {
        self.0.lock().unwrap().network.push(PathBuf::from(path));
    }

    /// Marks `path` and below a local volume of its own, device `dev`.
    pub(super) fn volume(&self, path: &str, dev: u64) {
        let mut disk = self.0.lock().unwrap();
        disk.volumes.push((PathBuf::from(path), dev));
    }

    /// Ejects the volume mounted at `path`: what is there is on the volume above it again.
    pub(super) fn eject(&self, path: &str) {
        let mut disk = self.0.lock().unwrap();
        disk.network.retain(|at| at != Path::new(path));
        disk.volumes.retain(|(at, _)| at != Path::new(path));
    }

    /// Fails every move into the set-aside folder with OS error `code`.
    pub(super) fn fail_moves(&self, code: i32) {
        self.0.lock().unwrap().fail_moves = Some(code);
    }

    /// Fails every rename in place and every link removal too.
    pub(super) fn fail_in_place(&self) {
        self.0.lock().unwrap().fail_in_place = true;
    }

    /// Makes a listing of `dir` panic.
    pub(super) fn panic_on(&self, dir: &str) {
        self.0.lock().unwrap().panic_on = Some(PathBuf::from(dir));
    }

    /// Makes a listing of `dir` wait until the flag returned is set.
    pub(super) fn block_listing(&self, dir: &str) -> Arc<AtomicBool> {
        let open = Arc::new(AtomicBool::new(false));
        self.0.lock().unwrap().block = Some((PathBuf::from(dir), Arc::clone(&open)));
        open
    }

    /// The paths in folder `dir` whose name starts with `prefix`.
    pub(super) fn named(&self, dir: &str, prefix: &str) -> Vec<PathBuf> {
        let disk = self.0.lock().unwrap();
        disk.entries
            .keys()
            .filter(|p| p.parent() == Some(Path::new(dir)))
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
            .cloned()
            .collect()
    }

    /// The paths under the set-aside folder.
    pub(super) fn set_aside(&self) -> Vec<PathBuf> {
        let disk = self.0.lock().unwrap();
        disk.entries
            .keys()
            .filter_map(|p| p.strip_prefix(SET_ASIDE).ok().map(Path::to_owned))
            .filter(|p| !p.as_os_str().is_empty())
            .collect()
    }

    /// The records written, parsed.
    pub(super) fn records(&self) -> Vec<serde_json::Value> {
        let disk = self.0.lock().unwrap();
        disk.entries
            .keys()
            .filter(|p| p.ends_with("record.json"))
            .map(|p| {
                let text = RECORDS.lock().unwrap()[p].clone();
                serde_json::from_slice(&text).unwrap()
            })
            .collect()
    }
}

/// [`Fs::leads_out`] of `fs` as `os::Real` judges it, with `chain` links before this one.
fn leads_out_from(fs: &FakeFs, link: &Path, root: &Path, chain: usize) -> bool {
    let disk = fs.0.lock().unwrap();
    let kind = |path: &Path| disk.entries.get(path).map(|&(kind, _)| kind);
    // The link's folders, from `root` down: each a folder, none a link.
    let Some(folder) = link.parent().filter(|dir| dir.starts_with(root)) else {
        return true;
    };
    let folders = folder
        .ancestors()
        .take_while(|dir| dir.starts_with(root))
        .all(|dir| kind(dir) == Some(Kind::Dir));
    let Some(target) = disk.targets.get(link).filter(|_| folders) else {
        return true;
    };
    let (mut at, steps) = if target.is_absolute() {
        let Ok(rest) = target.strip_prefix(root) else {
            return true;
        };
        (root.to_owned(), rest)
    } else {
        (folder.to_owned(), target.as_path())
    };
    // Its target, one name at a time: a link on the way leads out; one at the end is
    // judged in turn.
    let steps: Vec<Component<'_>> = steps.components().collect();
    let mut next = None;
    for (i, step) in steps.iter().enumerate() {
        match step {
            Component::CurDir => {}
            Component::ParentDir if at == root => return true,
            Component::ParentDir => {
                at.pop();
            }
            Component::Normal(name) => {
                at.push(name);
                let last = i + 1 == steps.len();
                match kind(&at) {
                    Some(Kind::Dir) => {}
                    Some(Kind::File | Kind::Other) if last => {}
                    Some(Kind::Link) if last && chain < 8 => {
                        next = Some(at.clone());
                        break;
                    }
                    _ => return true,
                }
            }
            Component::RootDir | Component::Prefix(_) => return true,
        }
    }
    let nowhere = steps.is_empty() && !target.is_absolute();
    drop(disk);
    match next {
        Some(next) => leads_out_from(fs, &next, root, chain + 1),
        None => nowhere,
    }
}

/// The records' bytes, by path (one test binary; paths include a unique session).
static RECORDS: Mutex<BTreeMap<PathBuf, Vec<u8>>> = Mutex::new(BTreeMap::new());

impl Fs for FakeFs {
    type Dir = PathBuf;

    fn meta(&self, path: &Path) -> Option<Meta> {
        let disk = self.0.lock().unwrap();
        // A network volume is a volume of its own, as a real mount is.
        let mounted = disk.volumes.iter().find(|(at, _)| path.starts_with(at));
        let volume = if disk.network.iter().any(|n| path.starts_with(n)) {
            2
        } else {
            mounted.map_or(1, |(_, dev)| *dev)
        };
        disk.entries.get(path).map(|&(kind, ino)| Meta {
            kind,
            id: (volume, ino),
        })
    }

    fn dated(&self, path: &Path) -> Option<(Meta, Stamp)> {
        let meta = self.meta(path)?;
        let mut disk = self.0.lock().unwrap();
        disk.dated += 1;
        let changes = disk.stamps.get(path).copied().unwrap_or_default();
        Some((meta, [changes, 0, changes, 0]))
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let mut disk = self.0.lock().unwrap();
        disk.touched.retain(|at| at != path);
        Ok(())
    }

    fn touch(&self, path: &Path) -> io::Result<()> {
        let mut disk = self.0.lock().unwrap();
        if disk.fail_touches > 0 {
            disk.fail_touches -= 1;
            return Err(io::ErrorKind::NotFound.into());
        }
        disk.touched.push(path.to_owned());
        Ok(())
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<(OsString, Kind)>> {
        let mut disk = self.0.lock().unwrap();
        disk.lists += 1;
        if let Some((_, open)) = disk.block.clone().filter(|(at, _)| at == dir) {
            drop(disk);
            while !open.load(Ordering::SeqCst) {
                async_io::block_on(async_io::Timer::after(Duration::from_millis(10)));
            }
            disk = self.0.lock().unwrap();
        }
        if disk.panic_on.as_deref() == Some(dir) {
            drop(disk);
            panic!("the fake's listing of {} panics", dir.display());
        }
        Ok(disk
            .entries
            .iter()
            .filter(|(p, _)| p.parent() == Some(dir))
            .map(|(p, &(kind, _))| (p.file_name().unwrap().to_owned(), kind))
            .collect())
    }

    fn open_dir(&self, dir: &Path) -> io::Result<Option<(PathBuf, Meta)>> {
        if !self.local(dir) {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let meta = self.meta(dir).filter(|meta| meta.kind == Kind::Dir);
        Ok(meta.map(|meta| (dir.to_owned(), meta)))
    }

    fn open_child(
        &self,
        _dir: &PathBuf,
        _name: &OsStr,
        path: &Path,
    ) -> io::Result<Option<(PathBuf, Meta)>> {
        self.open_dir(path)
    }

    fn entries(&self, dir: &PathBuf) -> io::Result<Vec<(OsString, Kind)>> {
        self.list(dir)
    }

    fn leads_out(&self, link: &Path, root: &Path) -> bool {
        leads_out_from(self, link, root, 0)
    }

    fn local(&self, dir: &Path) -> bool {
        let disk = self.0.lock().unwrap();
        !disk.network.iter().any(|n| dir.starts_with(n))
    }

    fn make_dirs(&self, dir: &Path) -> io::Result<()> {
        let mut disk = self.0.lock().unwrap();
        for at in dir.ancestors() {
            disk.next += 1;
            let id = disk.next;
            disk.entries.entry(at.to_owned()).or_insert((Kind::Dir, id));
        }
        Ok(())
    }

    fn make_dir(&self, dir: &Path) -> io::Result<()> {
        let mut disk = self.0.lock().unwrap();
        if disk.entries.contains_key(dir) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        disk.next += 1;
        let id = disk.next;
        disk.entries.insert(dir.to_owned(), (Kind::Dir, id));
        disk.changed(dir);
        Ok(())
    }

    fn rename_new(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut disk = self.0.lock().unwrap();
        if let Some(code) = disk.fail_moves
            && to.starts_with(SET_ASIDE)
        {
            return Err(io::Error::from_raw_os_error(code));
        }
        if disk.fail_in_place && to.to_string_lossy().contains("set-aside-by-clauseo") {
            return Err(io::Error::from_raw_os_error(1));
        }
        if !disk.entries.contains_key(from) {
            return Err(io::ErrorKind::NotFound.into());
        }
        if disk.entries.contains_key(to) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        let moved: Vec<PathBuf> = disk
            .entries
            .keys()
            .filter(|p| p.starts_with(from))
            .cloned()
            .collect();
        for p in moved {
            let entry = disk.entries.remove(&p).unwrap();
            let rest = p.strip_prefix(from).unwrap();
            let new = if rest.as_os_str().is_empty() {
                to.to_owned()
            } else {
                to.join(rest)
            };
            disk.entries.insert(new, entry);
        }
        disk.changed(from);
        disk.changed(to);
        Ok(())
    }

    fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.make_dir(path)?;
        let mut disk = self.0.lock().unwrap();
        disk.entries.get_mut(path).unwrap().0 = Kind::File;
        RECORDS
            .lock()
            .unwrap()
            .insert(path.to_owned(), bytes.to_owned());
        Ok(())
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        let disk = self.0.lock().unwrap();
        disk.targets
            .get(path)
            .cloned()
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn remove_link(&self, path: &Path) -> io::Result<()> {
        let mut disk = self.0.lock().unwrap();
        if disk.fail_in_place {
            return Err(io::Error::from_raw_os_error(1));
        }
        match disk.entries.get(path) {
            Some((Kind::Link, _)) => {
                disk.entries.remove(path);
                disk.targets.remove(path);
                disk.changed(path);
                Ok(())
            }
            Some(_) => Err(io::Error::other("not a link")),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    fn random(&self) -> u64 {
        let mut disk = self.0.lock().unwrap();
        disk.random = disk
            .random
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695);
        disk.random
    }

    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(489_600)
    }
}

mod kit;

pub(super) use kit::*;
