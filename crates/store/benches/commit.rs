//! Saving events: one `commit` of one 400-byte `TextAppended` (`commit_400b`), one of an 8 KiB
//! one, the host's flush size (`commit_8kib`), each with a receipt, and 8 commits of one 400-byte
//! `TextAppended` in 8 streams sent together so they group (`commit_group_8`, the time of all
//! 8). Wall clock: it reports and never fails a pull request.

#![expect(
    clippy::unwrap_used,
    reason = "bench setup and timed calls: a failure ends the bench run with its error"
)]

#[path = "common/dir.rs"]
mod dir;

use clauseo_protocol::{
    CommandId, CommandOutcome, Envelope, EventId, MessageId, Seq, StreamId, TextAppended, TextKind,
    ThreadId, Timestamp, TurnId,
};
use clauseo_store::{Commit, Receipt, Store, StoreOptions};
use criterion::{Criterion, criterion_group, criterion_main};
use futures::executor::block_on;

fn id<T: std::str::FromStr>(n: u64) -> T
where
    T::Err: std::fmt::Debug,
{
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

/// A commit of one `TextAppended` of `bytes` at `seq` in `stream`, with a receipt for command
/// `command` when it is `Some`.
fn commit(stream: StreamId, seq: u64, bytes: usize, command: Option<u64>) -> Commit {
    let text = "x".repeat(bytes);
    let event = TextAppended {
        turn: id::<TurnId>(1),
        message: id::<MessageId>(2),
        acp_message_id: None,
        kind: TextKind::Answer,
        text,
    };
    let ts = Timestamp::new(1_800_000_000_000).unwrap();
    let envelope = Envelope::new(
        id::<EventId>(seq),
        stream,
        Seq::new(seq).unwrap(),
        ts,
        command.map(id::<CommandId>),
        event.into(),
    );
    Commit {
        events: vec![envelope],
        receipt: command.map(|n| Receipt {
            command_id: id(n),
            stream: Some(stream),
            payload_hash: [7; 32],
            result: CommandOutcome::Done,
            ts,
        }),
        ..Commit::default()
    }
}

fn lone(c: &mut Criterion, name: &str, bytes: usize, stream_n: u64, store: &Store) {
    let stream = StreamId::Thread(id::<ThreadId>(stream_n));
    let mut seq = 0;
    c.bench_function(name, |b| {
        b.iter(|| {
            seq += 1;
            let command = stream_n * 10_000_000 + seq;
            block_on(store.commit(commit(stream, seq, bytes, Some(command)))).unwrap()
        });
    });
}

fn commits(c: &mut Criterion) {
    let dir = dir::BenchDir::new("commit").unwrap();
    let store = block_on(Store::open(StoreOptions::new(dir.path()))).unwrap();
    lone(c, "commit_400b", 400, 1, &store);
    lone(c, "commit_8kib", 8 * 1024, 2, &store);
    let streams: Vec<StreamId> = (100..108).map(|n| StreamId::Thread(id(n))).collect();
    let mut seq = 0;
    c.bench_function("commit_group_8", |b| {
        b.iter(|| {
            seq += 1;
            block_on(async {
                let mut replies = Vec::with_capacity(streams.len());
                for stream in &streams {
                    replies.push(
                        store
                            .send_commit(commit(*stream, seq, 400, None))
                            .await
                            .unwrap(),
                    );
                }
                for reply in replies {
                    reply.await.unwrap();
                }
            });
        });
    });
    block_on(store.close()).unwrap();
}

criterion_group!(benches, commits);
criterion_main!(benches);
