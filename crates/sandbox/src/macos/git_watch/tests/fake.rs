//! The tests' fakes: a file system in memory, a sink, a session, and the helpers.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use super::super::aside::Aside;
use super::super::state::{MatterState, Store};
use super::super::{Asks, Event, Fs, Kind, Launch, Meta, Phase, Session, Stamp, Watch, flags};
use crate::{Denial, DenialSink, GitItem, GitOutcome};

/// The fake volume: every entry by path, with its kind and inode.
#[derive(Default)]
struct Disk {
    entries: BTreeMap<PathBuf, (Kind, u64)>,
    next: u64,
    /// Paths on a network volume.
    network: Vec<PathBuf>,
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

    /// How many folders have been listed.
    pub(super) fn lists(&self) -> u64 {
        self.0.lock().unwrap().lists
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

/// The records' bytes, by path (one test binary; paths include a unique session).
static RECORDS: Mutex<BTreeMap<PathBuf, Vec<u8>>> = Mutex::new(BTreeMap::new());

impl Fs for FakeFs {
    fn meta(&self, path: &Path) -> Option<Meta> {
        let disk = self.0.lock().unwrap();
        // A network volume is a volume of its own, as a real mount is.
        let volume = if disk.network.iter().any(|n| path.starts_with(n)) {
            2
        } else {
            1
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

    fn touch(&self, path: &Path) -> io::Result<()> {
        self.0.lock().unwrap().touched.push(path.to_owned());
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

    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        let disk = self.0.lock().unwrap();
        let mut at = path.to_owned();
        for _ in 0..8 {
            match disk.targets.get(&at) {
                Some(target) => at = target.clone(),
                None => return disk.entries.contains_key(&at).then_some(at),
            }
        }
        None
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

/// A matter state in memory.
#[derive(Clone, Default)]
pub(super) struct MemStore(pub(super) Arc<Mutex<MatterState>>);

impl Store for MemStore {
    fn update<T>(&self, change: impl FnOnce(&mut MatterState) -> T) -> io::Result<T> {
        Ok(change(&mut self.0.lock().unwrap()))
    }
}

/// The notices the watch sent.
#[derive(Default)]
pub(super) struct Notices(Mutex<Vec<Denial>>);

impl DenialSink for Notices {
    fn denied(&self, d: Denial) {
        self.0.lock().unwrap().push(d);
    }
}

impl Notices {
    pub(super) fn taken(&self) -> Vec<Denial> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    pub(super) fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

/// A session whose processes go when told, or when ended (if `dies`).
#[derive(Clone, Default)]
pub(super) struct FakeSession {
    pub(super) gone: Arc<AtomicBool>,
    pub(super) ends: Arc<AtomicU32>,
    pub(super) dies: bool,
}

impl Session for FakeSession {
    fn gone(&self) -> bool {
        self.gone.load(Ordering::SeqCst)
    }

    fn end(&self) {
        self.ends.fetch_add(1, Ordering::SeqCst);
        if self.dies {
            self.gone.store(true, Ordering::SeqCst);
        }
    }
}

static NEXT_SESSION: AtomicU32 = AtomicU32::new(0);

/// The aside of a watch of `/m` for a new session.
pub(super) fn aside(notices: &Arc<Notices>) -> Aside {
    let n = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    Aside::new(
        PathBuf::from(MATTER),
        PathBuf::from(SET_ASIDE),
        format!("clauseo-1-0-{n}"),
        Arc::clone(notices) as Arc<dyn DenialSink>,
    )
}

/// A watch of `/m`; `git` are the launch's `.git`s.
pub(super) fn watch(fs: &FakeFs, git: &[&str]) -> (Watch<FakeFs>, Arc<Notices>) {
    watch_with(fs, git, &[])
}

/// [`watch`] with the launch's links out too.
pub(super) fn watch_with(
    fs: &FakeFs,
    git: &[&str],
    links: &[&str],
) -> (Watch<FakeFs>, Arc<Notices>) {
    let paths = |p: &[&str]| p.iter().map(PathBuf::from).collect();
    watch_launch(
        fs,
        &Launch {
            git: paths(git),
            links: paths(links),
            ..Launch::default()
        },
    )
}

/// [`watch`] with the launch given whole.
pub(super) fn watch_launch(fs: &FakeFs, launch: &Launch) -> (Watch<FakeFs>, Arc<Notices>) {
    let notices = Arc::new(Notices::default());
    let watch = Watch::new(fs.clone(), PathBuf::from(MATTER), aside(&notices), launch);
    (watch, notices)
}

pub(super) fn event(path: &str, flags: u32) -> Event {
    Event {
        path: PathBuf::from(path),
        flags,
    }
}

/// A check that found nothing: it does not ask to end the session.
pub(super) fn quiet(asks: Asks) {
    assert!(!asks.end, "{asks:?}");
}

/// `events` find something while the agent may run: nothing moves, and the session must end.
/// Then, the session ended, the final scan sets aside.
pub(super) fn found(watch: &mut Watch<FakeFs>, events: &[Event]) {
    let asks = watch.events(events);
    assert!(asks.end, "{events:?}");
    watch.ended_session();
    quiet(watch.full_scan(Phase::Ended));
}

/// No notice came.
pub(super) fn none(notices: &Notices) {
    assert_eq!(notices.taken(), Vec::<Denial>::new());
}

pub(super) const MOVED_IN: u32 = flags::ITEM_RENAMED | 0x2_0000;

/// The one notice, for the `.git` at `rel`, moved into the set-aside folder.
pub(super) fn assert_moved(notices: &Notices, rel: &str) {
    assert_set_aside(notices, rel, GitItem::DotGit);
}

/// The one notice, for the `want` at `rel`, moved into the set-aside folder.
pub(super) fn assert_set_aside(notices: &Notices, rel: &str, want: GitItem) {
    let notices = notices.taken();
    assert_eq!(notices.len(), 1, "{notices:?}");
    let Denial::GitSetAside {
        path,
        what,
        outcome: GitOutcome::Moved(to),
        ..
    } = &notices[0]
    else {
        panic!("{notices:?}");
    };
    assert_eq!((path.as_path(), *what), (Path::new(rel), want));
    assert!(to.starts_with(SET_ASIDE) && to.ends_with("item"), "{to:?}");
}
