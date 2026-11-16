//! The file action holding a matter's writer (schema 7, `pending_actions`): one row per matter,
//! a later put replaces it, a clear forgets it, and what is left survives a close and a reopen,
//! as the actions a crash left unfinished.

use clauseo_protocol::{Count, InterruptedKind, ThreadId};
use clauseo_store::PendingAction;
use futures::executor::block_on;

use crate::common::{TestDir, id, matter, ts};

fn action(n: u64, kind: InterruptedKind, files: Option<u64>) -> PendingAction {
    PendingAction {
        matter: matter(n),
        kind,
        files: files.map(|files| Count::new(files).unwrap()),
        thread: (kind == InterruptedKind::Undo).then(|| id::<ThreadId>(n)),
        at: ts(i64::try_from(n).unwrap()),
    }
}

#[test]
fn pending_actions_put_replace_clear_and_survive_a_reopen() {
    let dir = TestDir::new("pending_actions_put_replace_clear");
    let store = dir.open();
    assert_eq!(block_on(store.pending_actions()).unwrap(), Vec::new());
    block_on(store.put_pending_action(action(1, InterruptedKind::Undo, Some(3)))).unwrap();
    block_on(store.put_pending_action(action(2, InterruptedKind::Restore, None))).unwrap();
    // The same matter's next action replaces its row.
    let replaced = action(2, InterruptedKind::AcceptChanges, Some(1));
    block_on(store.put_pending_action(replaced.clone())).unwrap();
    block_on(store.put_pending_action(action(3, InterruptedKind::RejectChanges, Some(1)))).unwrap();
    block_on(store.clear_pending_action(matter(3))).unwrap();
    // Clearing a matter with none is fine.
    block_on(store.clear_pending_action(matter(9))).unwrap();
    let left = vec![action(1, InterruptedKind::Undo, Some(3)), replaced];
    assert_eq!(block_on(store.pending_actions()).unwrap(), left);
    block_on(store.close()).unwrap();

    let store = dir.open();
    assert_eq!(block_on(store.pending_actions()).unwrap(), left);
    block_on(store.close()).unwrap();
}

#[test]
fn a_pending_action_of_an_unknown_kind_is_skipped() {
    let dir = TestDir::new("pending_action_unknown_kind");
    let store = dir.open();
    block_on(store.put_pending_action(action(1, InterruptedKind::Undo, Some(2)))).unwrap();
    block_on(store.close()).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join(clauseo_store::DB_FILE)).unwrap();
    conn.execute(
        "INSERT INTO pending_actions (matter, kind, files, thread, at) VALUES (?1, 'merge', 2, NULL, 5)",
        [matter(2).to_string()],
    )
    .unwrap();
    drop(conn);
    let store = dir.open();
    assert_eq!(
        block_on(store.pending_actions()).unwrap(),
        vec![action(1, InterruptedKind::Undo, Some(2))]
    );
    block_on(store.close()).unwrap();
}
