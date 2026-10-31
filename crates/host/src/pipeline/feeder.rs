//! The feeder: one task on the actor's pool that hands commits to the store in send order.
//!
//! It awaits only `Store::send_commit`, which returns once the commit is in the writer's
//! queue, so the queue holds a stream's commits in seq order and group commit shares one sync
//! among those waiting. Each commit's reply (durable or failed) is awaited on a task of its own
//! that posts the result to the actor.

use std::rc::Rc;

use clauseo_store::{Commit, Committed, Store};
use futures::executor::LocalSpawner;
use futures::task::LocalSpawnExt;

use super::Ticket;
use crate::error::Error;

/// Where a commit's result goes: the actor's input channel.
pub(crate) type Post = Rc<dyn Fn(Ticket, Result<Committed, clauseo_store::Error>)>;

/// The actor's side of the feeder. Dropping it ends the feeder once the commits sent are queued.
#[derive(Debug, Clone)]
pub(crate) struct FeederTx(async_channel::Sender<(Ticket, Commit)>);

impl FeederTx {
    /// Queues one commit for the store without waiting. Fails with [`Error::Closed`] once the
    /// feeder has ended.
    pub(super) fn send(&self, ticket: Ticket, commit: Commit) -> Result<(), Error> {
        self.0
            .try_send((ticket, commit))
            .map_err(|_closed| Error::Closed)
    }
}

/// Starts the feeder on `spawner`'s pool, writing to `store` and posting each result with
/// `post`. Fails with [`Error::Closed`] when the pool has shut down.
pub(crate) fn spawn_feeder(
    spawner: &LocalSpawner,
    store: Store,
    post: Post,
) -> Result<FeederTx, Error> {
    let (sender, receiver) = async_channel::unbounded();
    spawner
        .spawn_local(feed(receiver, store, spawner.clone(), post))
        .map_err(|_closed| Error::Closed)?;
    Ok(FeederTx(sender))
}

/// The feeder's loop: enqueue each commit in order, then await its reply on its own task.
async fn feed(
    commits: async_channel::Receiver<(Ticket, Commit)>,
    store: Store,
    spawner: LocalSpawner,
    post: Post,
) {
    while let Ok((ticket, commit)) = commits.recv().await {
        match store.send_commit(commit).await {
            Ok(reply) => {
                let post_reply = Rc::clone(&post);
                let spawned = spawner.spawn_local(async move {
                    post_reply(ticket, reply.await);
                });
                if spawned.is_err() {
                    post(ticket, Err(clauseo_store::Error::Closed));
                }
            }
            Err(error) => post(ticket, Err(error)),
        }
    }
}
