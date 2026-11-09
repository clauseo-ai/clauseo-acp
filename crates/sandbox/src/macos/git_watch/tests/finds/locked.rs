//! Folders the watch can't read: while the agent may run one ends the session, as it could
//! hide a repository; the final scan keeps it as stuck, which refuses the matter's launches
//! until it can be read. `Locked` is the fake file system with some folders unreadable, as
//! after a `chmod 000`.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use super::super::super::state::{self, Host};
use super::super::super::{Fs, Kind, Launch, Meta, Phase, Stamp, Watch};
use super::super::fake::{FakeFs, MATTER, MOVED_IN, MemStore, Notices, aside, event, quiet};
use crate::DenialSink;

/// [`FakeFs`] whose folders in `locked` can't be opened or listed.
#[derive(Clone)]
struct Locked {
    fs: FakeFs,
    locked: Arc<Mutex<Vec<PathBuf>>>,
}

impl Locked {
    fn new(fs: &FakeFs) -> Locked {
        Locked {
            fs: fs.clone(),
            locked: Arc::default(),
        }
    }

    fn lock(&self, dir: &str) {
        self.locked.lock().unwrap().push(PathBuf::from(dir));
    }

    fn unlock(&self) {
        self.locked.lock().unwrap().clear();
    }

    /// Refused as `chmod 000` refuses.
    fn check(&self, dir: &Path) -> io::Result<()> {
        if self.locked.lock().unwrap().iter().any(|at| at == dir) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }
}

impl Fs for Locked {
    type Dir = PathBuf;
    fn meta(&self, path: &Path) -> Option<Meta> {
        self.fs.meta(path)
    }
    fn dated(&self, path: &Path) -> Option<(Meta, Stamp)> {
        self.fs.dated(path)
    }
    fn touch(&self, path: &Path) -> io::Result<()> {
        self.fs.touch(path)
    }
    fn list(&self, dir: &Path) -> io::Result<Vec<(OsString, Kind)>> {
        self.check(dir)?;
        self.fs.list(dir)
    }
    fn open_dir(&self, dir: &Path) -> io::Result<Option<(PathBuf, Meta)>> {
        self.check(dir)?;
        self.fs.open_dir(dir)
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
        self.fs.leads_out(link, root)
    }
    fn local(&self, dir: &Path) -> bool {
        self.fs.local(dir)
    }
    fn make_dirs(&self, dir: &Path) -> io::Result<()> {
        self.fs.make_dirs(dir)
    }
    fn make_dir(&self, dir: &Path) -> io::Result<()> {
        self.fs.make_dir(dir)
    }
    fn rename_new(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.fs.rename_new(from, to)
    }
    fn write_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.fs.write_new(path, bytes)
    }
    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.fs.read_link(path)
    }
    fn remove_link(&self, path: &Path) -> io::Result<()> {
        self.fs.remove_link(path)
    }
    fn random(&self) -> u64 {
        self.fs.random()
    }
    fn now(&self) -> SystemTime {
        self.fs.now()
    }
}

/// A watch of `/m` through `fs`, as for the matter.
fn watch_of(fs: &Locked) -> (Watch<Locked>, Arc<Notices>) {
    let notices = Arc::new(Notices::default());
    let launch = Launch {
        keep_repos: true,
        ..Launch::default()
    };
    let watch = Watch::new(fs.clone(), PathBuf::from(MATTER), aside(&notices), &launch);
    (watch, notices)
}

#[test]
fn an_unreadable_folder_ends_the_session_while_the_agent_may_run() {
    let fake = FakeFs::new();
    fake.dir("/m/docs");
    let fs = Locked::new(&fake);
    let (mut watch, notices) = watch_of(&fs);
    quiet(watch.full_scan(Phase::Running));
    quiet(watch.sweep(&|| false).0);
    fs.lock("/m/docs");
    assert!(watch.full_scan(Phase::Running).end, "a full scan");
    // A changed folder the 30 s pass can't list.
    fake.file("/m/docs/new.txt");
    assert!(watch.sweep(&|| false).0.end, "the 30 s pass");
    // A folder arriving unreadable, by its event's scan.
    fs.unlock();
    fake.dir("/m/in");
    fs.lock("/m/in");
    assert!(
        watch.events(&[event("/m/in", MOVED_IN)]).end,
        "an event's scan"
    );
    // A network volume the watch never enters is no find.
    fs.unlock();
    fake.remove("/m/in");
    fake.dir("/m/share");
    fake.network("/m/share");
    quiet(watch.full_scan(Phase::Running));
    quiet(watch.events(&[event("/m/share", MOVED_IN)]));
    assert_eq!(notices.taken(), Vec::<crate::Denial>::new());
}

#[test]
fn the_final_scan_keeps_an_unreadable_folder_as_stuck_until_it_can_be_read() {
    let fake = FakeFs::new();
    fake.dir("/m/s\u{202E}cret");
    fake.dir("/m/s\u{202E}cret/inner");
    let fs = Locked::new(&fake);
    let (mut watch, _notices) = watch_of(&fs);
    fs.lock("/m/s\u{202E}cret");
    quiet(watch.full_scan(Phase::Ended));
    let stuck = watch.take_stuck();
    assert_eq!(stuck.len(), 1, "{stuck:?}");
    assert!(stuck[0].unreadable);
    assert_eq!(stuck[0].path, PathBuf::from("/m/s\u{202E}cret"));
    let store = MemStore::default();
    state::end(&store, "s", stuck);
    let sink = Arc::new(Notices::default()) as Arc<dyn DenialSink>;
    let host = Host {
        dead: &|_| false,
        kill: &|_| true,
    };
    let begin = || state::begin(&store, &fs, Path::new(MATTER), &sink, &host);
    let error = begin().unwrap_err().to_string();
    assert!(
        error.starts_with(
            r"Clauseo won't start an AI tool in this matter until it can read s\u{202e}cret: "
        ),
        "{error}"
    );
    assert!(error.contains("permission denied"), "{error}");
    // Still refused while unreadable; let through once it can be read.
    assert!(begin().is_err());
    fs.unlock();
    begin().unwrap();
}
