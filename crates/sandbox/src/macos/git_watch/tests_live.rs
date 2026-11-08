//! The git watch on this Mac's `FSEvents`, one temp matter per test: a `git init` folder moved
//! in, a bare repository moved in, a repository cloned whole by `clonefile(2)`, and links out
//! of the matter, each set aside within 2 s with its record; a link inside the matter stays.

#![expect(
    clippy::disallowed_methods,
    reason = "the live tests make their temp folders and move the repositories with std"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::run::start_with;
use super::{Launch, Session, Watch, fsevents, os};
use crate::{Clock as _, Denial, DenialSink, GitItem, SystemClock};

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
        panic!("the move must not fail on one volume");
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
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(format!("/private/tmp/cgw-{}-{n}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let matter = root.join("matter");
        let tmp = root.join("tmp");
        std::fs::create_dir_all(&matter).unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        let live = Live {
            set_aside: root.join("data/sandbox/git-set-aside"),
            root,
            matter,
            tmp,
            notices: Arc::new(Notices::default()),
            gone: Arc::new(AtomicBool::new(false)),
        };
        let watch = Watch::new(
            os::Real,
            live.matter.clone(),
            live.set_aside.clone(),
            format!("clauseo-live-{n}"),
            &Launch::default(),
            Arc::clone(&live.notices) as Arc<dyn DenialSink>,
        );
        let watching = start_with(watch, fsevents::start).unwrap();
        watching.attach(Box::new(Running(Arc::clone(&live.gone))));
        live
    }

    /// Waits up to 2 s for `path` to leave, then for its one record; returns the latency, the
    /// item's set-aside folder and the notices.
    fn set_aside(&self, path: &Path) -> (Duration, PathBuf, Vec<Denial>) {
        let latency = wait_for("the set-aside", Duration::from_secs(2), || {
            std::fs::symlink_metadata(path).is_err()
        });
        println!("{} set aside {latency:?} after the change", path.display());
        let folders: Vec<PathBuf> = std::fs::read_dir(&self.set_aside)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(folders.len(), 1, "{folders:?}");
        let record = folders[0].join("record.json");
        wait_for("the record", Duration::from_secs(1), || record.exists());
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
        assert_eq!(record["original"], path.to_str().unwrap());
        assert_eq!(record["how"], "moved");
        wait_for("the notice", Duration::from_secs(1), || {
            !self.notices.0.lock().unwrap().is_empty()
        });
        let notices = std::mem::take(&mut *self.notices.0.lock().unwrap());
        (latency, folders[0].clone(), notices)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.gone.store(true, Ordering::SeqCst);
        drop(std::fs::remove_dir_all(&self.root));
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
    assert!(folder.join("item/HEAD").exists());
    assert_eq!(
        notices,
        [Denial::GitSetAside {
            path: Path::new("foo/.git").to_owned(),
            what: GitItem::DotGit,
            moved_to: Some(folder.join("item")),
            session_ended: false,
        }]
    );
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
