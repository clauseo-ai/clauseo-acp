use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use futures::future::Either;

use notify::event::{
    AccessKind, AccessMode, CreateKind, EventKind, MetadataKind, ModifyKind, RemoveKind, RenameMode,
};

use super::{Burst, Look, Timing, bursts, look, root_id, shows};
use crate::actor::input::Input;
use crate::pipeline::tests::support::TempDir;

/// Whether an event of `kind` on `paths` counts for the folder at `/m`.
fn seen(kind: EventKind, paths: &[&str]) -> bool {
    let mut event = notify::Event::new(kind);
    for path in paths {
        event = event.add_path(PathBuf::from(path));
    }
    shows(&roots(), &Ok(event))
}

fn roots() -> Vec<PathBuf> {
    vec![PathBuf::from("/m"), PathBuf::from("/private/m")]
}

const MADE: EventKind = EventKind::Create(CreateKind::File);

#[test]
fn a_paper_made_or_renamed_counts_under_either_spelling_of_the_folder() {
    assert!(seen(MADE, &["/m/brief.docx"]));
    assert!(seen(MADE, &["/private/m/pleadings/claim.pdf"]));
    let renamed = EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both));
    assert!(seen(renamed, &["/m/~$a.docx", "/m/a.docx"]));
}

#[test]
fn the_folder_itself_and_a_lost_track_count() {
    let gone = EventKind::Remove(notify::event::RemoveKind::Folder);
    assert!(seen(gone, &["/m"]));
    assert!(seen(EventKind::Any, &[]));
    assert!(shows(&roots(), &Err(notify::Error::generic("overflow"))));
    // A path outside both spellings is not one the filter can judge: it counts.
    assert!(seen(MADE, &["/elsewhere/a.txt"]));
}

#[test]
fn files_the_listing_never_shows_and_reads_do_not_count() {
    for path in [
        "/m/~$brief.docx",
        "/m/a/draft.tmp",
        "/m/.clauseo/matter.json",
        "/m/.git/HEAD",
    ] {
        assert!(!seen(MADE, &[path]), "{path}");
    }
    let opened = EventKind::Access(AccessKind::Open(AccessMode::Read));
    assert!(!seen(opened, &["/m/brief.docx"]));
    let closed = EventKind::Access(AccessKind::Close(AccessMode::Write));
    assert!(!seen(closed, &["/m/brief.docx"]));
    let atime = EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime));
    assert!(!seen(atime, &["/m/brief.docx"]));
}

#[test]
fn a_folder_the_listing_shows_counts_though_its_name_reads_as_a_passed_over_file() {
    // A folder `Drafts.tmp` moved in or out: its papers are listed, so the change counts.
    let folder = EventKind::Create(CreateKind::Folder);
    assert!(seen(folder, &["/m/Drafts.tmp"]));
    let renamed = EventKind::Modify(ModifyKind::Name(RenameMode::Any));
    assert!(seen(renamed, &["/m/Drafts.tmp"]));
    assert!(!seen(renamed, &["/m/.git/index"]));
    assert!(!seen(
        EventKind::Remove(RemoveKind::File),
        &["/m/~$brief.docx"]
    ));
    assert!(!seen(
        EventKind::Remove(RemoveKind::Folder),
        &["/m/.clauseo"]
    ));
}

/// Waits are short in these tests; none is a fixed sleep the result depends on.
const SHORT: Timing = Timing {
    quiet: Duration::from_millis(150),
    max_wait: Duration::from_millis(600),
    recheck: Duration::ZERO,
};

/// A burst thread over the folder `dir`, fed by the returned sender; its posts come on the
/// returned receiver.
fn burst_thread(
    dir: &TempDir,
    timing: Timing,
) -> (mpsc::Sender<()>, async_channel::Receiver<Input>) {
    let (send, changes) = mpsc::channel();
    let (post, posts) = async_channel::unbounded();
    let burst = Burst {
        matter: "01900000-0000-7000-8000-000000000001".parse().unwrap(),
        generation: 7,
        folder: dir.path().to_path_buf(),
        root: root_id(dir.path()),
        clock: std::sync::Arc::new(crate::clock::SystemClock),
    };
    std::thread::spawn(move || bursts(&burst, timing, &changes, &post));
    (send, posts)
}

/// The next `FolderChanged` posted, waiting up to `patience`.
fn told(posts: &async_channel::Receiver<Input>, patience: Duration) -> Option<(u64, Look)> {
    futures::executor::block_on(async {
        let next = std::pin::pin!(posts.recv());
        match futures::future::select(next, async_io::Timer::after(patience)).await {
            Either::Left((Ok(Input::PapersChanged(changed)), _timer)) => {
                Some((changed.generation, changed.look))
            }
            Either::Left((other, _timer)) => panic!("not a change: {other:?}"),
            Either::Right(_) => None,
        }
    })
}

#[test]
fn a_lone_change_is_told_once_the_folder_is_quiet() {
    let dir = TempDir::new("watch-quiet");
    let (send, posts) = burst_thread(&dir, SHORT);
    send.send(()).unwrap();
    send.send(()).unwrap();
    assert_eq!(
        told(&posts, Duration::from_secs(10)),
        Some((7, Look::Found))
    );
    assert_eq!(
        told(&posts, Duration::from_millis(400)),
        None,
        "one burst, one post"
    );
}

#[test]
fn a_trickle_that_never_goes_quiet_is_told_at_the_longest_wait() {
    let dir = TempDir::new("watch-trickle");
    let (send, posts) = burst_thread(&dir, SHORT);
    // A change at most every 30 ms, never the 150 ms of quiet, for up to 6 s: ten times the
    // 600 ms wait.
    let mut first = None;
    for _ in 0..200 {
        send.send(()).unwrap();
        first = told(&posts, Duration::from_millis(30));
        if first.is_some() {
            break;
        }
    }
    assert_eq!(
        first,
        Some((7, Look::Found)),
        "told while the trickle still ran"
    );
}

#[test]
fn a_folder_gone_reads_missing_and_one_swapped_in_reads_replaced() {
    let dir = TempDir::new("watch-look");
    let folder = dir.path().join("m");
    let other = dir.path().join("other");
    crate::write::create_dir_all(&folder).unwrap();
    crate::write::create_dir_all(&other).unwrap();
    let root = root_id(&folder);
    let (_send, changes) = mpsc::channel();
    let seen = |what| look(&folder, root, Duration::ZERO, &changes) == Some(what);
    assert!(seen(Look::Found));
    crate::write::rename(&folder, &dir.path().join("m-old")).unwrap();
    assert!(seen(Look::Missing));
    crate::write::rename(&other, &folder).unwrap();
    assert!(seen(Look::Replaced));
}
