//! The git watch on this Mac's `FSEvents`, one temp matter per test: a `git init` folder moved
//! in, a bare repository moved in, a repository cloned whole by `clonefile(2)`, a folder of
//! `HEAD` and `config` moved in, `objects` and `refs` added to a launch `HEAD` folder, and
//! links out of the matter: each ends the session, then is set aside within 2 s with its record;
//! a link inside the matter stays. Also: the canary's write reaches the stream, the host's
//! start runs a dead session's final scan, and the launch's walk and the watch count the same
//! dangling `objects` and `refs` links.

#![expect(
    clippy::disallowed_methods,
    reason = "the live tests make their temp folders and move the repositories with std"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::aside::Aside;
use super::run::{Mode, Msg, start_with};
use super::state::{self, FileStore, SessionRecord, Store as _};
use super::{Fs as _, Launch, Session, Watch, fsevents, host, os};
use crate::{Clock as _, Denial, DenialSink, GitItem, GitOutcome, SystemClock};

#[derive(Default)]
struct Notices(Mutex<Vec<Denial>>);

impl DenialSink for Notices {
    fn denied(&self, d: Denial) {
        self.0.lock().unwrap().push(d);
    }
}

struct Running(Arc<AtomicBool>);

impl Session for Running {
    fn gone(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    fn end(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A temp root holding `matter`, `tmp` (the agent's scratch stand-in) and the set-aside
/// folder, with a watch running on the matter. Removed on drop.
struct Live {
    root: PathBuf,
    matter: PathBuf,
    tmp: PathBuf,
    set_aside: PathBuf,
    notices: Arc<Notices>,
    gone: Arc<AtomicBool>,
}

impl Live {
    fn new() -> Live {
        Live::with(|_| {})
    }

    /// A live watch whose matter `setup` fills before the launch.
    fn with(setup: impl FnOnce(&Path)) -> Live {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(format!("/private/tmp/cgw-{}-{n}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let matter = root.join("matter");
        let tmp = root.join("tmp");
        std::fs::create_dir_all(&matter).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        setup(&matter);
        let live = Live {
            set_aside: root.join("data/sandbox/git-set-aside"),
            root,
            matter,
            tmp,
            notices: Arc::new(Notices::default()),
            gone: Arc::new(AtomicBool::new(false)),
        };
        let session = format!("clauseo-live-{n}");
        let aside = Aside::new(
            live.matter.clone(),
            live.set_aside.clone(),
            session.clone(),
            Arc::clone(&live.notices) as Arc<dyn DenialSink>,
        );
        let watch = Watch::new(os::Real, live.matter.clone(), aside, &Launch::default());
        let store = FileStore::new(&live.set_aside, &live.matter);
        let watching = start_with(
            watch,
            session,
            store,
            None,
            None,
            Mode::Live,
            fsevents::start,
        )
        .unwrap();
        watching.attach(Running(Arc::clone(&live.gone)));
        live
    }

    /// Waits up to 2 s for `path` to leave, then for its one record; returns the latency, the
    /// item's set-aside folder and the notices.
    fn set_aside(&self, path: &Path) -> (Duration, PathBuf, Vec<Denial>) {
        let latency = wait_for("the set-aside", Duration::from_secs(2), || {
            std::fs::symlink_metadata(path).is_err()
        });
        println!("{} set aside {latency:?} after the change", path.display());
        assert!(
            self.gone.load(Ordering::SeqCst),
            "the session was ended first"
        );
        let folders: Vec<PathBuf> = std::fs::read_dir(&self.set_aside)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| !p.ends_with("matters"))
            .collect();
        assert_eq!(folders.len(), 1, "{folders:?}");
        let record = folders[0].join("record.json");
        // The record is made, then written: wait until it reads whole.
        let parsed = || {
            std::fs::read(&record)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        };
        wait_for("the record", Duration::from_secs(1), || parsed().is_some());
        let record = parsed().unwrap();
        assert_eq!(record["original"], path.to_str().unwrap());
        assert!(record["outcome"]["moved"].is_string(), "{record}");
        wait_for("the notice", Duration::from_secs(1), || {
            !self.notices.0.lock().unwrap().is_empty()
        });
        let notices = std::mem::take(&mut *self.notices.0.lock().unwrap());
        (latency, folders[0].clone(), notices)
    }
}

impl Drop for Live {
    /// Ends the session and removes the root, again for up to 3 s while the watch's last state
    /// write races the removal.
    fn drop(&mut self) {
        self.gone.store(true, Ordering::SeqCst);
        let clock = SystemClock::new();
        while std::fs::remove_dir_all(&self.root).is_err()
            && self.root.exists()
            && clock.now() < Duration::from_secs(3)
        {
            async_io::block_on(async_io::Timer::after(Duration::from_millis(100)));
        }
    }
}

fn wait_for(what: &str, cap: Duration, mut done: impl FnMut() -> bool) -> Duration {
    let clock = SystemClock::new();
    while !done() {
        assert!(clock.now() < cap, "{what} not within {cap:?}");
        async_io::block_on(async_io::Timer::after(Duration::from_millis(10)));
    }
    clock.now()
}

fn run(program: &str, args: &[&str]) {
    let out = async_io::block_on(async_process::Command::new(program).args(args).output());
    let out = out.unwrap();
    assert!(out.status.success(), "{program} {args:?}: {out:?}");
}

fn git_init(args: &[&str], at: &Path) {
    let mut all = vec!["init", "-q"];
    all.extend(args);
    all.push(at.to_str().unwrap());
    run("/usr/bin/git", &all);
}

#[test]
fn a_repository_moved_into_a_watched_matter_is_set_aside_within_two_seconds() {
    let live = Live::new();
    let foo = live.tmp.join("foo");
    git_init(&[], &foo);
    std::fs::rename(&foo, live.matter.join("foo")).unwrap();
    let moved = live.matter.join("foo/.git");
    let (_, folder, notices) = live.set_aside(&moved);
    assert!(live.matter.join("foo").is_dir(), "the folder itself stays");
    assert!(folder.join("item/objects").is_dir());
    assert!(
        !folder.join("item/HEAD").exists(),
        "the set-aside item's HEAD is renamed"
    );
    assert_eq!(
        notices,
        [Denial::GitSetAside {
            path: Path::new("foo/.git").to_owned(),
            what: GitItem::DotGit,
            outcome: GitOutcome::Moved(folder.join("item")),
            session_ended: true,
        }]
    );
}

#[test]
fn a_folder_of_head_and_config_moved_in_loses_its_head() {
    let live = Live::new();
    let half = live.tmp.join("half");
    std::fs::create_dir(&half).unwrap();
    std::fs::write(half.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(half.join("config"), "[core]\n").unwrap();
    std::fs::rename(&half, live.matter.join("half")).unwrap();
    let (_, _, notices) = live.set_aside(&live.matter.join("half/HEAD"));
    assert!(live.matter.join("half/config").exists());
    assert!(matches!(
        &notices[..],
        [Denial::GitSetAside {
            what: GitItem::BareRepoHead,
            ..
        }]
    ));
}

#[test]
fn objects_and_refs_added_to_a_launch_head_folder_take_its_head() {
    let live = Live::with(|matter| {
        let half = matter.join("half");
        std::fs::create_dir(&half).unwrap();
        std::fs::write(half.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(half.join("config"), "[core]\n").unwrap();
    });
    let half = live.matter.join("half");
    std::fs::create_dir(half.join("objects")).unwrap();
    std::fs::create_dir(half.join("refs")).unwrap();
    let (_, _, notices) = live.set_aside(&half.join("HEAD"));
    assert!(half.join("objects").is_dir() && half.join("config").exists());
    assert!(matches!(
        &notices[..],
        [Denial::GitSetAside {
            what: GitItem::BareRepoHead,
            ..
        }]
    ));
}

#[test]
fn a_bare_repository_moved_in_is_set_aside_whole() {
    let live = Live::new();
    let bare = live.tmp.join("archive");
    git_init(&["--bare"], &bare);
    std::fs::create_dir(live.matter.join("drafts")).unwrap();
    let moved = live.matter.join("drafts/archive");
    std::fs::rename(&bare, &moved).unwrap();
    let (_, folder, notices) = live.set_aside(&moved);
    assert!(folder.join("item/objects").is_dir());
    assert!(matches!(
        &notices[..],
        [Denial::GitSetAside { what: GitItem::BareRepo, path, .. }] if path == Path::new("drafts/archive")
    ));
}

#[test]
fn a_clonefiled_repository_is_set_aside() {
    let live = Live::new();
    let foo = live.tmp.join("foo");
    git_init(&[], &foo);
    // One clonefile(2) of the whole folder, as APFS allows for a folder.
    let src = std::fs::File::open(&foo).unwrap();
    let dst = std::fs::File::open(&live.matter).unwrap();
    rustix::fs::fclonefileat(&src, &dst, "copy", rustix::fs::CloneFlags::empty()).unwrap();
    let (_, _, notices) = live.set_aside(&live.matter.join("copy/.git"));
    assert!(live.matter.join("copy").is_dir());
    assert!(matches!(
        &notices[..],
        [Denial::GitSetAside {
            what: GitItem::DotGit,
            ..
        }]
    ));
}

#[test]
fn a_link_out_of_the_matter_is_set_aside_and_one_inside_stays() {
    let live = Live::new();
    std::fs::create_dir(live.matter.join("docs")).unwrap();
    let inside = live.matter.join("shortcut");
    std::os::unix::fs::symlink(live.matter.join("docs"), &inside).unwrap();
    let foo = live.tmp.join("foo");
    git_init(&[], &foo);
    let out = live.matter.join("foo");
    std::os::unix::fs::symlink(&foo, &out).unwrap();
    let (_, folder, notices) = live.set_aside(&out);
    assert!(
        std::fs::symlink_metadata(folder.join("item"))
            .unwrap()
            .is_symlink()
    );
    assert!(
        foo.join(".git/HEAD").exists(),
        "the link's target is untouched"
    );
    assert!(inside.is_symlink(), "a link inside the matter stays");
    assert!(matches!(
        &notices[..],
        [Denial::GitSetAside {
            what: GitItem::OutsideLink,
            ..
        }]
    ));
}

/// A temp folder of its own for a test without a running watch; removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let root = PathBuf::from(format!("/private/tmp/cgw-{}-{name}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        std::fs::create_dir_all(root.join("matter")).unwrap();
        std::fs::create_dir_all(root.join("set-aside")).unwrap();
        Scratch(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

#[test]
fn the_canarys_write_reaches_the_stream() {
    let dir = Scratch::new("canary");
    let set_aside = dir.0.join("set-aside");
    let (tx, rx) = std::sync::mpsc::channel();
    let stream = fsevents::start(&[dir.0.join("matter"), set_aside.clone()], tx).unwrap();
    let canary = set_aside.join("watch-canary");
    os::Real.touch(&canary).unwrap();
    let clock = SystemClock::new();
    loop {
        let left = Duration::from_secs(2).saturating_sub(clock.now());
        match rx.recv_timeout(left) {
            Ok(Msg::Events(events, _)) if events.iter().any(|e| e.path == canary) => break,
            Ok(_) => {}
            Err(error) => panic!("no event for the canary within 2 s: {error}"),
        }
    }
    println!("the canary's event came {:?} after its write", clock.now());
    drop(stream);
}

#[test]
fn the_hosts_start_runs_a_dead_sessions_final_scan() {
    let dir = Scratch::new("start-up");
    let (matter, set_aside) = (dir.0.join("matter"), dir.0.join("set-aside"));
    let store = FileStore::new(&set_aside, &matter);
    store
        .update(|state| state.root = Some(matter.as_path().into()))
        .unwrap();
    // A session of a host that is gone (no process has pid u32::MAX), with no process left.
    let record = SessionRecord {
        session: "clauseo-dead-1".to_owned(),
        host_pid: u32::MAX,
        host_start: Some(1),
        probe: format!(
            "chat.clauseo.session.clauseo-test-dead-{}",
            std::process::id()
        ),
        set_aside: set_aside.as_path().into(),
        baseline: state::Baseline::default(),
        launched: None,
        outlived: false,
        finds: Vec::new(),
        mode: super::run::Mode::Live,
    };
    state::record(&store, record).unwrap();
    git_init(&[], &matter.join("foo"));
    let notices = Arc::new(Notices::default());
    let sink = Arc::clone(&notices) as Arc<dyn DenialSink>;
    assert_eq!(host::start_up(&set_aside, &sink).unwrap(), 1);
    assert!(!matter.join("foo/.git").exists());
    assert_eq!(notices.0.lock().unwrap().len(), 1);
    let sessions = store.update(|state| state.sessions.len()).unwrap();
    assert_eq!(sessions, 0);
    // A host start with nothing to do.
    assert_eq!(host::start_up(&set_aside, &sink).unwrap(), 0);
}

#[test]
fn the_walk_at_launch_and_the_watch_count_the_same_dangling_links() {
    let dir = Scratch::new("dangling");
    let matter = dir.0.join("matter");
    std::fs::create_dir(matter.join("r")).unwrap();
    std::fs::write(matter.join("r/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::os::unix::fs::symlink("../nowhere/objects", matter.join("r/objects")).unwrap();
    std::os::unix::fs::symlink("../nowhere/refs", matter.join("r/refs")).unwrap();
    let scan = crate::scan_matter(&matter).unwrap();
    assert_eq!(
        scan.bare_repos(),
        [matter.join("r")],
        "the policy makes it read-only"
    );
    let aside = Aside::new(
        matter.clone(),
        dir.0.join("set-aside"),
        "clauseo-dangling".to_owned(),
        Arc::new(Notices::default()) as Arc<dyn DenialSink>,
    );
    let launch = Launch {
        keep_repos: true,
        ..Launch::default()
    };
    let watch = Watch::new(os::Real, matter.clone(), aside, &launch);
    let repos: Vec<PathBuf> = watch
        .baseline()
        .repos
        .iter()
        .map(state::Known::path)
        .collect();
    assert_eq!(
        repos,
        [matter.join("r")],
        "the watch leaves the same folder alone"
    );
}

#[test]
fn a_stale_canary_is_removed_and_a_running_watchs_stays() {
    let dir = Scratch::new("canaries");
    let folder = dir.0.join("set-aside");
    for name in [
        "watch-canary-0000000000000001",
        "watch-canary-notes-0000000000000002",
        "keep",
    ] {
        std::fs::write(folder.join(name), "canary\n").unwrap();
    }
    run(
        "/usr/bin/touch",
        &[
            "-t",
            "200001010000",
            folder
                .join("watch-canary-0000000000000001")
                .to_str()
                .unwrap(),
        ],
    );
    run(
        "/usr/bin/touch",
        &["-t", "200001010000", folder.join("keep").to_str().unwrap()],
    );
    super::run::remove_stale_canaries(&os::Real, &folder);
    assert!(
        !folder.join("watch-canary-0000000000000001").exists(),
        "a crashed host's"
    );
    assert!(
        folder.join("watch-canary-notes-0000000000000002").exists(),
        "written just now"
    );
    assert!(folder.join("keep").exists(), "not a canary");
}
