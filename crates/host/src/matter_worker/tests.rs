use std::path::PathBuf;
use std::sync::mpsc;

use clauseo_matters::{CheckpointOutcome, DEFAULT_MARKER, NewMatter, TurnMeta};
use clauseo_protocol::{CheckpointId, MatterId, MatterPath, ThreadId, Timestamp, TurnId};
use uuid::Uuid;

use super::{MatterJob, MatterReply, MatterWorker, Registry, Relink, matter_settings, tool_paths};
use crate::sent::TurnPaths;
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
    assert!(matches!(reply, MatterReply::Created(Ok(_))), "{reply:?}");
    let (tag, reply, _) = rx.recv().unwrap();
    assert_eq!(tag, 2);
    let MatterReply::BeforeTurn(Ok(before)) = reply else {
        panic!("{reply:?}")
    };
    // Nothing changed since the import checkpoint; the reply carries its timeline entry.
    assert_eq!(
        before.checkpoint.outcome,
        CheckpointOutcome::Reused(root_checkpoint)
    );
    let entry = before.reused.and_then(|reused| reused.entry).unwrap();
    assert_eq!(entry.kind, clauseo_protocol::CheckpointKind::Import);

    // The turn writes a file no tool call named.
    write::write_file(&folder.join("notes.txt"), b"Alpha\n").unwrap();
    worker
        .send(
            MatterJob::AfterTurn {
                meta: meta(4),
                tool_paths: TurnPaths::default(),
                before: Some(root_checkpoint),
            },
            3,
        )
        .unwrap();
    let (tag, reply, _) = rx.recv().unwrap();
    assert_eq!(tag, 3);
    let MatterReply::AfterTurn(Ok(after)) = reply else {
        panic!("{reply:?}")
    };
    assert!(after.reused.is_none(), "a new checkpoint was made");
    let after = after.checkpoint;
    assert_eq!(
        after.outcome,
        CheckpointOutcome::Created(meta(4).checkpoint)
    );
    assert_eq!(after.changes.len(), 1);
    assert!(
        after
            .changes
            .iter()
            .all(clauseo_protocol::FileChange::may_include_lawyer_edits)
    );

    // R8 reuses that checkpoint for the next after-turn: the reply has its changes since
    // the import, as if the log had lost it.
    worker
        .send(
            MatterJob::AfterTurn {
                meta: meta(5),
                tool_paths: TurnPaths::default(),
                before: Some(root_checkpoint),
            },
            4,
        )
        .unwrap();
    let (_, reply, _) = rx.recv().unwrap();
    let MatterReply::AfterTurn(Ok(again)) = reply else {
        panic!("{reply:?}")
    };
    let reused = again.reused.unwrap();
    assert_eq!(reused.entry.map(|entry| entry.id), Some(meta(4).checkpoint));
    let paths: Vec<_> = reused.changes.iter().map(|c| c.path().as_str()).collect();
    assert_eq!(paths, ["notes.txt"]);

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
            MatterJob::BeforeTurn(TurnMeta {
                checkpoint: CheckpointId::new(v7(1)).unwrap(),
                thread: ThreadId::new(v7(2)).unwrap(),
                turn: TurnId::new(v7(3)).unwrap(),
                at: Timestamp::new(1_790_000_000_000).unwrap(),
            }),
            (),
        )
        .unwrap();
    let reply = rx.recv().unwrap();
    assert!(
        matches!(
            reply,
            MatterReply::BeforeTurn(Err(crate::error::Error::Internal(_)))
        ),
        "{reply:?}"
    );
    worker.stop().unwrap();
}

/// A worker whose replies come back on the returned channel.
fn worker(root: &std::path::Path) -> (MatterWorker<u32>, mpsc::Receiver<MatterReply>) {
    let (tx, rx) = mpsc::channel::<MatterReply>();
    let worker = MatterWorker::<u32>::start(
        matter_settings(&root.join("data")),
        Box::new(move |_tag, reply| tx.send(reply).unwrap()),
    )
    .unwrap();
    (worker, rx)
}

/// `Open` of `id` in `folder`, with a relink and an empty registry.
fn open_with_relink(id: MatterId, folder: &std::path::Path, at: Timestamp) -> MatterJob {
    MatterJob::Open {
        id,
        folder: folder.to_owned(),
        relink: Some(Box::new(Relink {
            created_at: at,
            registry: Registry {
                refused: Vec::new(),
                others: Vec::new(),
            },
        })),
    }
}

#[test]
fn the_first_open_spends_the_relink_whatever_it_finds() {
    let root = test_dir("relink-spent");
    let folder = root.join("matter");
    let away = root.join("away");
    let marker = folder.join(".clauseo/matter.json");
    let at = Timestamp::new(1_790_000_000_000).unwrap();
    let id = MatterId::new(v7(2)).unwrap();
    let (made, replies) = worker(&root);
    let new = NewMatter {
        id,
        folder: folder.clone(),
        created_at: at,
        marker: DEFAULT_MARKER,
        checkpoint: CheckpointId::new(v7(1)).unwrap(),
    };
    made.send(MatterJob::Create(new), 0).unwrap();
    assert!(matches!(
        replies.recv().unwrap(),
        MatterReply::Created(Ok(_))
    ));
    made.stop().unwrap();

    // A deleted marker in the recorded folder is written again by the first open.
    write::remove_file(&marker).unwrap();
    let (relinks, replies) = worker(&root);
    relinks.send(open_with_relink(id, &folder, at), 0).unwrap();
    let reply = replies.recv().unwrap();
    assert!(matches!(reply, MatterReply::Opened(Ok(_))), "{reply:?}");
    assert!(marker.exists());
    relinks.stop().unwrap();

    // A first open that finds no folder spends it: when the folder comes back without its
    // marker, a later open does not write one.
    write::rename(&folder, &away).unwrap();
    let (spent, replies) = worker(&root);
    spent.send(open_with_relink(id, &folder, at), 0).unwrap();
    let reply = replies.recv().unwrap();
    let MatterReply::Opened(Err(error)) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(
        error.code(),
        clauseo_protocol::ErrorCode::MatterFolderMissing
    );
    write::rename(&away, &folder).unwrap();
    write::remove_file(&marker).unwrap();
    let meta = TurnMeta {
        checkpoint: CheckpointId::new(v7(3)).unwrap(),
        thread: ThreadId::new(v7(10)).unwrap(),
        turn: TurnId::new(v7(11)).unwrap(),
        at,
    };
    spent.send(MatterJob::BeforeTurn(meta), 0).unwrap();
    let reply = replies.recv().unwrap();
    let MatterReply::BeforeTurn(Err(error)) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(
        error.code(),
        clauseo_protocol::ErrorCode::MatterFolderMissing
    );
    assert!(!marker.exists(), "a later open wrote matter.json");
    spent.stop().unwrap();
}

/// An `Outside` place that resolves into the matter on the disk (through a symlink to it, with
/// `..`, a file the turn created) counts as the matter path it names; one that lies elsewhere,
/// or does not resolve, does not.
#[cfg(unix)]
#[test]
fn tool_paths_resolve_outside_spellings_into_the_matter() {
    let root = test_dir("tool-paths");
    let real = root.join("real");
    write::create_dir_all(&real.join("notes")).unwrap();
    write::write_file(&real.join("brief.txt"), b"Clause 4\n").unwrap();
    write::write_file(&root.join("elsewhere.txt"), b"x\n").unwrap();
    let link = root.join("link");
    write::symlink(&real, &link).unwrap();
    let matter = |path: &str| MatterPath::new(path).unwrap();
    let text = |path: PathBuf| path.display().to_string();
    let paths = TurnPaths {
        inside: [matter("kept.txt")].into(),
        outside: [
            text(link.join("brief.txt")),
            text(real.join("..").join("real").join("notes").join("new.txt")),
            "../real/brief.txt".to_owned(),
            text(root.join("elsewhere.txt")),
            text(root.join("gone").join("a.txt")),
        ]
        .into(),
    };
    // The matter folder as the lawyer picked it, through the link.
    let found = tool_paths(&link, paths);
    assert_eq!(
        found,
        [
            matter("brief.txt"),
            matter("kept.txt"),
            matter("notes/new.txt")
        ]
        .into()
    );
    write::remove_dir_all(&root).unwrap();
}
