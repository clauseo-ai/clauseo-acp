use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc;

use clauseo_matters::{CheckpointOutcome, DEFAULT_MARKER, NewMatter, TurnMeta};
use clauseo_protocol::{CheckpointId, MatterId, ThreadId, Timestamp, TurnId};
use uuid::Uuid;

use super::{MatterJob, MatterReply, MatterWorker, matter_settings};
use crate::write;

/// A valid v7 UUID, distinct per `n`.
fn v7(n: u32) -> Uuid {
    let mut random = [0_u8; 10];
    for (byte, from) in random.iter_mut().zip(n.to_be_bytes()) {
        *byte = from;
    }
    uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &random).into_uuid()
}

/// `<temp>/clauseo-host-unit-<name>-<pid>`, emptied first.
fn test_dir(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("clauseo-host-unit-{name}-{}", std::process::id()));
    write::remove_dir_all(&path).unwrap();
    write::create_dir_all(&path).unwrap();
    path
}

#[test]
fn create_then_checkpoints_on_the_matter_thread() {
    let root = test_dir("matter-worker");
    let folder = root.join("matter");
    let (tx, rx) = mpsc::channel::<(u32, MatterReply, Option<String>)>();
    let worker = MatterWorker::<u32>::start(
        matter_settings(&root.join("data")),
        Box::new(move |tag, reply| {
            let name = std::thread::current().name().map(str::to_owned);
            tx.send((tag, reply, name)).unwrap();
        }),
    )
    .unwrap();
    let at = Timestamp::new(1_790_000_000_000).unwrap();
    let root_checkpoint = CheckpointId::new(v7(1)).unwrap();
    worker
        .send(
            MatterJob::Create(NewMatter {
                id: MatterId::new(v7(2)).unwrap(),
                folder: folder.clone(),
                created_at: at,
                marker: DEFAULT_MARKER,
                checkpoint: root_checkpoint,
            }),
            1,
        )
        .unwrap();
    let meta = |n| TurnMeta {
        checkpoint: CheckpointId::new(v7(n)).unwrap(),
        thread: ThreadId::new(v7(10)).unwrap(),
        turn: TurnId::new(v7(11)).unwrap(),
        at,
    };
    worker.send(MatterJob::BeforeTurn(meta(3)), 2).unwrap();

    let (tag, reply, name) = rx.recv().unwrap();
    assert_eq!((tag, name.as_deref()), (1, Some("clauseo-matter")));
    assert!(matches!(reply, MatterReply::Created(Ok(()))), "{reply:?}");
    let (tag, reply, _) = rx.recv().unwrap();
    assert_eq!(tag, 2);
    let MatterReply::BeforeTurn(Ok(before)) = reply else {
        panic!("{reply:?}")
    };
    // Nothing changed since the import checkpoint.
    assert_eq!(before.outcome, CheckpointOutcome::Reused(root_checkpoint));

    // The turn writes a file no tool call named.
    write::write_file(&folder.join("notes.txt"), b"Alpha\n").unwrap();
    worker
        .send(
            MatterJob::AfterTurn {
                meta: meta(4),
                tool_paths: BTreeSet::new(),
            },
            3,
        )
        .unwrap();
    let (tag, reply, _) = rx.recv().unwrap();
    assert_eq!(tag, 3);
    let MatterReply::AfterTurn(Ok(after)) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(
        after.outcome,
        CheckpointOutcome::Created(meta(4).checkpoint)
    );
    assert_eq!(after.changes.len(), 1);
    assert!(
        after
            .changes
            .iter()
            .all(|change| change.may_include_lawyer_edits())
    );

    worker.stop().unwrap();
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn a_job_before_any_matter_is_an_internal_error() {
    let (tx, rx) = mpsc::channel();
    let root = std::env::temp_dir();
    let worker = MatterWorker::<()>::start(
        matter_settings(&root),
        Box::new(move |(), reply| tx.send(reply).unwrap()),
    )
    .unwrap();
    worker
        .send(
            MatterJob::EditedSince {
                before: CheckpointId::new(v7(1)).unwrap(),
                after: CheckpointId::new(v7(2)).unwrap(),
            },
            (),
        )
        .unwrap();
    let reply = rx.recv().unwrap();
    assert!(
        matches!(
            reply,
            MatterReply::EditedSince(Err(crate::error::Error::Internal(_)))
        ),
        "{reply:?}"
    );
    worker.stop().unwrap();
}
