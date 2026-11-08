//! The tests' fakes: a file system in memory, a sink, a session, and the helpers.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use super::super::{Asks, Event, Fs, Kind, Launch, Meta, Session, Watch, flags};
use crate::{Denial, DenialSink, GitItem};

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

    fn list(&self, dir: &Path) -> io::Result<Vec<(OsString, Kind)>> {
        let disk = self.0.lock().unwrap();
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
        Ok(())
    }

    fn rename_new(&self, from: &Path, to: &Path) -> io::Result<()> {
        let mut disk = self.0.lock().unwrap();
        if let Some(code) = disk.fail_moves
            && to.starts_with(SET_ASIDE)
        {
            return Err(io::Error::from_raw_os_error(code));
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

    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(489_600)
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
}

/// A session whose tree goes when told, or when ended (if `dies`).
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
    let notices = Arc::new(Notices::default());
    let paths = |p: &[&str]| p.iter().map(PathBuf::from).collect();
    let launch = Launch {
        git: paths(git),
        links: paths(links),
    };
    let n = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    let watch = Watch::new(
        fs.clone(),
        PathBuf::from(MATTER),
        PathBuf::from(SET_ASIDE),
        format!("clauseo-1-0-{n}"),
        &launch,
        Arc::clone(&notices) as Arc<dyn DenialSink>,
    );
    (watch, notices)
}

pub(super) fn event(path: &str, flags: u32) -> Event {
    Event {
        path: PathBuf::from(path),
        flags,
    }
}

/// A check that must not ask to end the session.
pub(super) fn quiet(asks: Asks) {
    assert!(!asks.end, "{asks:?}");
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
        moved_to,
        session_ended,
    } = &notices[0]
    else {
        panic!("{notices:?}");
    };
    assert_eq!((path.as_path(), *what), (Path::new(rel), want));
    assert!(
        moved_to.as_ref().unwrap().starts_with(SET_ASIDE),
        "{moved_to:?}"
    );
    assert!(moved_to.as_ref().unwrap().ends_with("item"));
    assert!(!session_ended);
}
