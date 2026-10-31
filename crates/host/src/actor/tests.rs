//! Smoke tests of the actor: start, the Scratch matter, commands with receipts, both
//! subscription kinds, shutdown, restart, and the store lock.

use std::path::Path;
use std::sync::Arc;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{
    AgentId, CommandEnvelope, CommandId, CommandOutcome, DomainEvent, ErrorCode, HostCommand,
    HostEvent, HostQuery, MatterId, MatterKind, Principal, QueryResult, Seq, StreamId, ThreadId,
    Timestamp, Title, ViewPatchBody,
};
use clauseo_settings::{Effective, Locations, SystemFiles};
use futures::StreamExt;
use futures::executor::block_on;
use uuid::Uuid;

use crate::pipeline::tests::support::TempDir;
use crate::{
    EventSubscription, Host, HostConfig, HostHandle, HostJoin, ManualClock, SequentialIds,
};

/// A host on `dir` with `fake-agent` in its settings, the manual clock and run `run`'s ids.
fn start(dir: &Path, run: u16) -> (HostHandle, HostJoin) {
    let settings = br#"{"agents": {"fake-agent": {"command": "/usr/bin/true"}}}"#;
    crate::write::write_file(&dir.join("settings.json"), settings).unwrap();
    let effective = Effective::load(&Locations::under_root(dir), &SystemFiles, &[]);
    let launcher = PassthroughLauncher::new(SpawnOptions::new().home(dir.join("home")));
    let config = HostConfig::new(effective, Arc::new(launcher))
        .with_clock(Arc::new(ManualClock::new(
            Timestamp::new(1_790_000_000_000).unwrap(),
        )))
        .with_ids(Box::new(SequentialIds::with_run(run)));
    Host::start(config).unwrap()
}

fn command_id(n: u128) -> CommandId {
    CommandId::new(Uuid::from_u128(n)).unwrap()
}

fn envelope(n: u128, command: HostCommand) -> CommandEnvelope {
    CommandEnvelope {
        command_id: command_id(n),
        command,
    }
}

fn matters(host: &HostHandle) -> Vec<(MatterId, MatterKind)> {
    match block_on(host.query(Principal::LocalDesktop, HostQuery::ListMatters)).unwrap() {
        QueryResult::Matters(matters) => matters.iter().map(|m| (m.matter, m.kind)).collect(),
        other @ (QueryResult::Threads(_)
        | QueryResult::Entries(_)
        | QueryResult::FileChanges(_)
        | QueryResult::Blob(_)) => panic!("not matters: {other:?}"),
    }
}

fn scratch(host: &HostHandle) -> MatterId {
    let all = matters(host);
    assert_eq!(all.len(), 1, "{all:?}");
    let (matter, kind) = all.first().copied().unwrap();
    assert_eq!(kind, MatterKind::Scratch);
    matter
}

fn create_thread(host: &HostHandle, n: u128, matter: MatterId) -> ThreadId {
    let command = HostCommand::CreateThread {
        matter,
        agent: AgentId::new("fake-agent").unwrap(),
    };
    match block_on(host.dispatch(envelope(n, command))).unwrap() {
        CommandOutcome::ThreadCreated { thread } => thread,
        other @ (CommandOutcome::Done
        | CommandOutcome::MatterCreated { .. }
        | CommandOutcome::PromptAccepted { .. }
        | CommandOutcome::StepUndone { .. }
        | CommandOutcome::VersionRestored { .. }) => panic!("not a thread: {other:?}"),
    }
}

/// Reads an event subscription until its `CaughtUp`: the replayed kinds and the last seq.
fn replay(events: &mut EventSubscription) -> (Vec<&'static str>, Vec<u64>, Seq) {
    let mut kinds = Vec::new();
    let mut seqs = Vec::new();
    loop {
        match block_on(events.next()).unwrap() {
            HostEvent::Events(envelopes) => {
                for e in envelopes {
                    kinds.push(kind(&e.event));
                    seqs.push(e.seq.get());
                }
            }
            HostEvent::CaughtUp { last_seq, .. } => return (kinds, seqs, last_seq),
            other @ (HostEvent::Patches(_) | HostEvent::AccountLimitsChanged(_)) => {
                panic!("unexpected {other:?}")
            }
        }
    }
}

fn kind(event: &DomainEvent) -> &'static str {
    let names = [
        (
            matches!(event, DomainEvent::MatterCreated(_)),
            "matter_created",
        ),
        (
            matches!(event, DomainEvent::CheckpointRecorded(_)),
            "checkpoint_recorded",
        ),
        (
            matches!(event, DomainEvent::ThreadCreated(_)),
            "thread_created",
        ),
        (
            matches!(event, DomainEvent::ThreadRenamed(_)),
            "thread_renamed",
        ),
        (
            matches!(event, DomainEvent::ThreadArchived(_)),
            "thread_archived",
        ),
    ];
    names
        .iter()
        .find(|(is, _)| *is)
        .map_or("other", |(_, name)| name)
}

#[test]
fn start_command_subscribe_shutdown_restart() {
    let dir = TempDir::new("actor-smoke");
    let (host, join) = start(dir.path(), 0);
    let matter = scratch(&host);

    // The Scratch matter's stream, replayed then caught up.
    let mut events =
        block_on(host.subscribe_events(Principal::LocalDesktop, StreamId::Matter(matter), None))
            .unwrap();
    let (kinds, seqs, last) = replay(&mut events);
    assert_eq!(kinds, ["matter_created", "checkpoint_recorded"]);
    assert_eq!(seqs, [1, 2]);
    assert_eq!(last.get(), 2);

    // A thread, its receipt on a repeat, and a conflict on another body.
    let thread = create_thread(&host, 1, matter);
    assert_eq!(create_thread(&host, 1, matter), thread);
    let other = HostCommand::ArchiveThread { thread };
    let conflict = block_on(host.dispatch(envelope(1, other))).unwrap_err();
    assert_eq!(conflict.code(), ErrorCode::CommandConflict);
    let stream = StreamId::Thread(thread);

    // A view subscription starts with a Reset; a rename reaches it as a header upsert.
    let mut view = block_on(host.subscribe(Principal::LocalDesktop, stream, None)).unwrap();
    let HostEvent::Patches(first) = block_on(view.next()).unwrap().decode().unwrap() else {
        panic!("not patches");
    };
    assert!(matches!(
        first.patches.first().map(|p| &p.body),
        Some(ViewPatchBody::Reset { .. })
    ));
    let rename = HostCommand::RenameThread {
        thread,
        title: Title::new("Lease review").unwrap(),
    };
    assert_eq!(
        block_on(host.dispatch(envelope(2, rename))).unwrap(),
        CommandOutcome::Done
    );
    let encoded = block_on(view.next()).unwrap();
    assert!(
        encoded.json().contains("Lease review"),
        "{}",
        encoded.json()
    );
    let listed = block_on(host.query(Principal::LocalDesktop, HostQuery::ListThreads { matter }));
    let QueryResult::Threads(threads) = listed.unwrap() else {
        panic!("not threads");
    };
    assert_eq!(threads.len(), 1);
    assert_eq!(
        threads.first().and_then(|t| t.title.as_deref()),
        Some("Lease review")
    );

    // An event subscription after seq 1 gets only what follows, then live envelopes.
    let mut thread_events =
        block_on(host.subscribe_events(Principal::LocalDesktop, stream, Some(Seq::FIRST))).unwrap();
    let (kinds, seqs, last) = replay(&mut thread_events);
    assert_eq!(kinds, ["thread_renamed"]);
    assert_eq!(seqs, [2]);
    assert_eq!(last.get(), 2);
    let archive = HostCommand::ArchiveThread { thread };
    block_on(host.dispatch(envelope(3, archive))).unwrap();
    let HostEvent::Events(live) = block_on(thread_events.next()).unwrap() else {
        panic!("not events");
    };
    assert_eq!(live.iter().map(|e| e.seq.get()).collect::<Vec<_>>(), [3]);

    // Unknown ids get their own codes; a principal this version does not know is refused.
    let unknown = HostCommand::CreateThread {
        matter: MatterId::new(Uuid::now_v7()).unwrap(),
        agent: AgentId::new("fake-agent").unwrap(),
    };
    let refused = block_on(host.dispatch(envelope(4, unknown))).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::CommandUnknownMatter);
    let stranger: Principal = serde_json::from_str("\"some_phone\"").unwrap();
    let refused = block_on(host.query(stranger, HostQuery::ListMatters)).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::TransportUnauthorized);

    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
    assert!(block_on(events.next()).is_none());
    let after = block_on(host.query(Principal::LocalDesktop, HostQuery::ListMatters));
    assert_eq!(after.unwrap_err().code(), ErrorCode::Internal);

    // A second start finds the same Scratch matter and the thread; no second Scratch.
    let (host, join) = start(dir.path(), 1);
    assert_eq!(scratch(&host), matter);
    let listed = block_on(host.query(Principal::LocalDesktop, HostQuery::ListThreads { matter }));
    let QueryResult::Threads(threads) = listed.unwrap() else {
        panic!("not threads");
    };
    assert_eq!(threads.len(), 1);
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}

#[test]
fn concurrent_duplicates_share_the_first_answer() {
    let dir = TempDir::new("actor-duplicates");
    let (host, join) = start(dir.path(), 0);
    let matter = scratch(&host);
    let command = || HostCommand::CreateThread {
        matter,
        agent: AgentId::new("fake-agent").unwrap(),
    };
    let (a, b) = block_on(futures::future::join(
        host.dispatch(envelope(7, command())),
        host.dispatch(envelope(7, command())),
    ));
    assert_eq!(a.unwrap(), b.unwrap());
    let listed = block_on(host.query(Principal::LocalDesktop, HostQuery::ListThreads { matter }));
    let QueryResult::Threads(threads) = listed.unwrap() else {
        panic!("not threads");
    };
    assert_eq!(threads.len(), 1, "one thread for two dispatches");
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}

#[test]
fn a_second_start_is_locked_and_abort_frees_the_folder() {
    let dir = TempDir::new("actor-lock");
    let (host, _join) = start(dir.path(), 0);
    let settings = b"{}";
    crate::write::write_file(&dir.path().join("settings.json"), settings).unwrap();
    let effective = Effective::load(&Locations::under_root(dir.path()), &SystemFiles, &[]);
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let second = Host::start(HostConfig::new(effective, Arc::new(launcher))).unwrap_err();
    assert_eq!(second.code(), ErrorCode::StoreLocked);
    block_on(host.abort_for_test());
    let (host, join) = start(dir.path(), 1);
    scratch(&host);
    block_on(host.shutdown()).unwrap();
    join.join().unwrap();
}
