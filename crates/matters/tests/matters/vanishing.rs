//! Files and folders that vanish after the crate saw them and before it reaches them: each is
//! taken as deleted, never as an error and never as kept.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use clauseo_matters::{Imported, Matter, PlaceholderProbe, Settings};

use crate::fixture::{Fixture, cid, created, deleted, meta, path, tools, undo};
use crate::support;

/// What a [`Vanishing`] probe does when asked about a path: `true` once it has done it.
type Action = Box<dyn FnMut(&Path) -> bool + Send>;

/// A probe that runs its armed action each time it is asked about a file, until the action
/// says it is done. Never cloud-only. The walk asks it about each file right after that
/// file's stat, so the action runs between the walk's steps.
#[derive(Default)]
struct Vanishing {
    action: Mutex<Option<Action>>,
}

impl Vanishing {
    fn arm(&self, action: impl FnMut(&Path) -> bool + Send + 'static) {
        *self.action.lock().unwrap() = Some(Box::new(action));
    }
}

impl PlaceholderProbe for Vanishing {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        let mut action = self.action.lock().unwrap();
        if let Some(run) = action.as_mut()
            && run(path)
        {
            *action = None;
        }
        false
    }
}

/// Imports `fx`'s folder as matter 1 with `probe`.
fn import(fx: &Fixture, probe: &Arc<Vanishing>) -> Matter {
    let settings = Settings {
        probe: Arc::<Vanishing>::clone(probe),
        ..fx.settings()
    };
    let cancel = AtomicBool::new(false);
    let imported = Matter::import(settings, fx.new_matter(), &[], &mut |_| {}, &cancel);
    let Ok(Imported::Done { matter, .. }) = imported else {
        panic!("{imported:?}");
    };
    *matter
}

/// The paths of checkpoint `n`.
fn paths_at(matter: &Matter, n: u64) -> Vec<String> {
    matter
        .files_at(cid(n))
        .unwrap()
        .into_iter()
        .map(|file| file.path.as_str().to_owned())
        .collect()
}

/// A file deleted after the walk listed it and before its content was read is left out of
/// the checkpoint, which goes ahead.
#[test]
fn a_file_gone_before_its_read_is_left_out() {
    let fx = Fixture::new("vanish-before-read");
    fx.write("a.txt", "a");
    fx.write("gone.txt", "gone");
    let probe = Arc::new(Vanishing::default());
    let gone = fx.file("gone.txt");
    probe.arm(move |asked| {
        if asked != gone {
            return false;
        }
        support::remove_file(&gone);
        true
    });
    let matter = import(&fx, &probe);
    assert_eq!(paths_at(&matter, 1), ["a.txt"]);
}

/// A file deleted after its folder was listed and before its own stat is left out of the
/// checkpoint, which goes ahead.
#[test]
fn a_file_gone_before_its_stat_is_left_out() {
    let fx = Fixture::new("vanish-before-stat");
    fx.write("a.txt", "a");
    fx.write("b.txt", "b");
    let probe = Arc::new(Vanishing::default());
    // Asked about whichever file the listing gave first: the other one is listed but not
    // stat'd yet.
    let (a, b) = (fx.file("a.txt"), fx.file("b.txt"));
    let first = Arc::new(Mutex::new(None));
    let seen = Arc::clone(&first);
    probe.arm(move |asked| {
        let other = if asked == a { &b } else { &a };
        support::remove_file(other);
        *seen.lock().unwrap() = asked.file_name().map(std::borrow::ToOwned::to_owned);
        true
    });
    let matter = import(&fx, &probe);
    let kept = first.lock().unwrap().clone().unwrap();
    assert_eq!(paths_at(&matter, 1), [kept.to_str().unwrap()]);
}

/// A folder deleted after its parent was listed and before it was read is gone with all its
/// files: the checkpoint reports them deleted, not kept as an unreadable folder's would be.
#[test]
fn a_folder_gone_before_it_is_listed_has_its_files_deleted() {
    let fx = Fixture::new("vanish-folder");
    fx.write("a.txt", "a");
    fx.write("sub/b.txt", "b");
    let probe = Arc::new(Vanishing::default());
    let mut matter = import(&fx, &probe);

    // The walk reads the root's entries (asking about a.txt) before any subfolder.
    let (a, sub) = (fx.file("a.txt"), fx.file("sub"));
    probe.arm(move |asked| {
        if asked != a {
            return false;
        }
        support::remove_dir_all(&sub);
        true
    });
    let changes = created(matter.before_turn(&meta(2)).unwrap(), 2);
    assert_eq!(changes, [deleted("sub/b.txt", b"b")]);
}

/// Undoing a step that added a file the lawyer has deleted since goes ahead: the file is
/// already gone, which is what the undo asked for.
#[test]
fn undoing_an_addition_already_deleted_goes_ahead() {
    let fx = Fixture::new("vanish-undo-added");
    fx.write("keep.txt", "keep");
    let mut matter = fx.import();
    fx.write("new.txt", "new");
    created(
        matter.after_turn(&meta(3), &tools(&["new.txt"])).unwrap(),
        3,
    );
    support::remove_file(fx.file("new.txt"));

    let undone = undo(&mut matter, (1, 3), &["new.txt"], (4, 5)).unwrap();
    assert_eq!(undone.restored, [path("new.txt")]);
    assert!(!fx.exists("new.txt"));
    assert_eq!(fx.read("keep.txt"), b"keep");
}
