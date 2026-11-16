//! The host's [`VersionsSource`]: a read goes to the actor ([`Input::MatterRead`]), which hands
//! it to the matter's thread as a [`MatterJob::Read`](crate::matter_worker::MatterJob), starting
//! the thread when the matter has none and following it when a crashed one was replaced. The
//! log's times and restore sources are read from the store on the calling thread: each
//! version's `CheckpointRecorded` row, then one read of the matter's stream from the oldest to
//! just past the newest (a restore's `VersionRestored` follows its `CheckpointRecorded`).

use std::collections::BTreeSet;

use clauseo_protocol::{CheckpointId, DomainEvent, MatterId, Seq, StreamId};
use clauseo_store::Store;

use super::{Logged, MatterRead, Unreachable, VersionsSource};
use crate::actor::input::Input;
use crate::log_file::Cause;

/// The host's source: the actor's internal channel and the store.
pub(crate) struct HostSource {
    post: async_channel::Sender<Input>,
    store: Store,
}

impl HostSource {
    /// A source posting to the actor through `post` and reading the log from `store`.
    pub(crate) const fn new(post: async_channel::Sender<Input>, store: Store) -> Self {
        Self { post, store }
    }
}

impl VersionsSource for HostSource {
    fn on_matter(&self, matter: MatterId, read: MatterRead) -> Result<(), Unreachable> {
        self.post
            .try_send(Input::MatterRead { matter, read })
            .map_err(|_closed| Unreachable)
    }

    fn logged(&self, matter: MatterId, versions: &[CheckpointId]) -> Logged {
        futures::executor::block_on(logged(&self.store, matter, versions)).unwrap_or_else(|error| {
            tracing::warn!(error = %Cause(&error), "the versions' log was not read");
            Logged::default()
        })
    }
}

/// See [`VersionsSource::logged`].
async fn logged(
    store: &Store,
    matter: MatterId,
    versions: &[CheckpointId],
) -> Result<Logged, clauseo_store::Error> {
    let mut seqs = Vec::new();
    for id in versions {
        if let Some(row) = store.checkpoint(*id).await?
            && row.matter == matter
        {
            seqs.push(row.seq.get());
        }
    }
    let mut logged = Logged::default();
    let (Some(&low), Some(&high)) = (seqs.iter().min(), seqs.iter().max()) else {
        return Ok(logged);
    };
    let wanted: BTreeSet<CheckpointId> = versions.iter().copied().collect();
    let stream = StreamId::Matter(matter);
    let mut after = Seq::new(low.saturating_sub(1)).ok();
    loop {
        let page = store
            .read_after(stream, after, clauseo_store::READ_PAGE_MAX)
            .await?;
        let Some(last) = page.last().map(|envelope| envelope.seq) else {
            break;
        };
        after = Some(last);
        for envelope in &page {
            if let DomainEvent::CheckpointRecorded(recorded) = &envelope.event
                && wanted.contains(&recorded.checkpoint())
            {
                logged.at.insert(recorded.checkpoint(), envelope.ts);
            } else if let DomainEvent::VersionRestored(restored) = &envelope.event
                && wanted.contains(&restored.checkpoint)
            {
                logged
                    .restored_from
                    .insert(restored.checkpoint, restored.from);
            }
        }
        if last.get() > high {
            break;
        }
    }
    Ok(logged)
}
