//! The host's [`VersionsSource`]: a read goes to the actor ([`Input::MatterRead`]), which hands it
//! to the matter's thread as a [`MatterJob::Read`](crate::matter_worker::MatterJob) only when that
//! thread is already live (`actor/matters.rs` `read`). It never starts or replaces one, which would
//! open and could relink the matter: with no live thread the read is answered as not open, and
//! after a failed send it is not answered and its caller gives up. The log's times and restore
//! sources are read from the store on the calling thread: each version's `CheckpointRecorded` row,
//! then one read of the matter's stream from the oldest to just past the newest (a restore's
//! `VersionRestored` follows its `CheckpointRecorded`).

use std::collections::BTreeSet;

use clauseo_protocol::{CheckpointId, DomainEvent, MatterId, Seq, StreamId};
use clauseo_store::{CheckpointRow, Store};

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

/// The log position of `row` when it is `matter`'s: a checkpoint row of another matter (never
/// asked for, since ids come from this matter's timeline) is left out.
fn own_seq(row: Option<CheckpointRow>, matter: MatterId) -> Option<u64> {
    row.filter(|row| row.matter == matter)
        .map(|row| row.seq.get())
}

/// See [`VersionsSource::logged`].
async fn logged(
    store: &Store,
    matter: MatterId,
    versions: &[CheckpointId],
) -> Result<Logged, clauseo_store::Error> {
    let mut seqs = Vec::new();
    for id in versions {
        if let Some(seq) = own_seq(store.checkpoint(*id).await?, matter) {
            seqs.push(seq);
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

#[cfg(test)]
mod tests {
    use clauseo_protocol::{CheckpointId, MatterId, Seq};
    use clauseo_store::CheckpointRow;

    use super::own_seq;

    #[test]
    fn a_checkpoint_row_of_another_matter_is_left_out() {
        let id = |n: u128| uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 | n);
        let ours = MatterId::new(id(1)).unwrap();
        let theirs = MatterId::new(id(2)).unwrap();
        let row = |matter| CheckpointRow {
            id: CheckpointId::new(id(3)).unwrap(),
            matter,
            commit_oid: "0".repeat(40),
            seq: Seq::new(7).unwrap(),
        };
        assert_eq!(own_seq(Some(row(ours)), ours), Some(7));
        assert_eq!(own_seq(Some(row(theirs)), ours), None);
        assert_eq!(own_seq(None, ours), None);
    }
}
