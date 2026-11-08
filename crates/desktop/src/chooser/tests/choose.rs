//! Choosing: a ref is issued only after the native confirm says yes, and then exactly one,
//! through the bridge's own `pick_folder`.

use std::collections::HashSet;

use futures::channel::oneshot;

use super::super::choose::FolderConfirm;
use super::*;
use crate::dialogs::{BoxFuture, LastPick};
use crate::test_support::{Scratch, block_on};

/// A confirm that answers `answer`, or never answers (`None`: its dialog went away), and the
/// paths it was asked about.
struct Answers {
    answer: Option<bool>,
    asked: Mutex<Vec<String>>,
}

impl FolderConfirm for Answers {
    fn confirm(&self, path: &str) -> BoxFuture<'_, bool> {
        self.asked.lock().unwrap().push(path.to_owned());
        let (tx, rx) = oneshot::channel();
        if let Some(answer) = self.answer {
            tx.send(answer).unwrap();
        }
        Box::pin(async move { rx.await.unwrap_or(false) })
    }
}

fn answers(answer: Option<bool>) -> Answers {
    Answers {
        answer,
        asked: Mutex::new(Vec::new()),
    }
}

/// Choosing issues a ref only after the confirm says yes, and then exactly one, through the
/// bridge's `pick_folder`; the last pick is recorded and the session closes. With no folder
/// shown, a confirm that never answers, or a decline, nothing is issued and nothing recorded.
#[test]
fn choosing_issues_one_ref_only_after_the_confirm() {
    let scratch = Scratch::new("chooser-choose", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let last = LastPick::default();
    let (chooser, _release) = fake_chooser(r"C:\Users\rmehra\Desktop");
    let now = crate::open::now();
    let id = open(&chooser, now).chooser;
    let yes = answers(Some(true));
    let none = block_on(choose_folder(&chooser, &bridge, &yes, &last, None, id));
    assert_eq!(none.unwrap_err().code(), ErrorCode::CommandInvalid);
    assert!(yes.asked.lock().unwrap().is_empty());

    chooser
        .list(
            id,
            Go::Typed {
                path: r"C:\Cases\Acme".to_owned(),
            },
            now,
        )
        .unwrap();
    for confirm in [answers(None), answers(Some(false))] {
        let picked = block_on(choose_folder(&chooser, &bridge, &confirm, &last, None, id));
        assert_eq!(picked.unwrap(), None);
        assert_eq!(*confirm.asked.lock().unwrap(), [r"C:\Cases\Acme"]);
        assert!(bridge.issued().is_empty());
        assert_eq!(last.get(), None);
    }
    // Declined, the session is still open; a folder that has gone is not offered to confirm.
    chooser
        .list(
            id,
            Go::Typed {
                path: r"C:\Gone".to_owned(),
            },
            now,
        )
        .unwrap();
    let gone = block_on(choose_folder(&chooser, &bridge, &yes, &last, None, id));
    assert_eq!(gone.unwrap(), None);
    assert!(yes.asked.lock().unwrap().is_empty());

    chooser
        .list(
            id,
            Go::Typed {
                path: r"C:\Cases\Acme".to_owned(),
            },
            now,
        )
        .unwrap();
    let picked = block_on(choose_folder(&chooser, &bridge, &yes, &last, None, id));
    let picked = picked.unwrap().unwrap();
    assert_eq!(bridge.issued(), HashSet::from([picked.folder]));
    assert_eq!(picked.display, r"C:\Cases\Acme");
    assert_eq!(last.get().unwrap().to_str(), Some(r"C:\Cases\Acme"));
    assert_eq!(
        code(chooser.list(id, Go::Again, now)),
        ErrorCode::CommandInvalid
    );
}

/// The e2e build picks its `E2E_MATTER_DIR` before the variable and the pre-check: `Fixed`
/// even with `CLAUSEO_FOLDER_PICKER=app` on Windows (the check never runs), then `Picked` with
/// one ref issued through the bridge's `pick_folder`.
#[cfg(feature = "e2e")]
#[test]
fn the_e2e_folder_comes_before_the_chooser() {
    let route = route(true, true, Some("app"), || panic!("the pre-check ran"));
    assert_eq!(route, Route::Fixed);
    let scratch = Scratch::new("chooser-e2e-fixed", &["work"]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let folder = scratch.path().join("work");
    let answer = block_on(pick_fixed(&bridge, folder, Some(scratch.path()))).unwrap();
    let PickAnswer::Picked { folder } = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(folder.display, "work");
    assert_eq!(bridge.issued(), HashSet::from([folder.folder]));
}
