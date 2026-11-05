//! The matter list and the thread lists over `subscribe_app`: `MattersChanged` and
//! `ThreadsChanged`, sent when a row the rail shows changes.
//!
//! The host keeps the rows it last sent ([`Published`], only while anyone is subscribed) and,
//! after each input the actor handles, compares what changed since with them ([`flush`]). What
//! can have changed is marked as it happens, so an input that changed nothing costs a check of
//! two empty sets:
//!
//! - every durable envelope of a thread's stream marks that thread (`rail::note_commit`), and so
//!   does unloading its stream (its kept header and last stop are set then);
//! - every answered command marks everything (`commands::finish_command`): matter rows change
//!   only by commands (create, update, open, forget, bring back, relocate) and by a
//!   `ListMatters` folder read, and both are the lawyer's doing, so a whole comparison there is
//!   cheap;
//! - a `ListMatters` folder read that changes a matter's folder state marks everything
//!   (`query::on_folder_states`).
//!
//! A marked thread's row is built and compared with the one sent; only when it differs is its
//! matter's row built and compared too. Rows are compared on every field but `last_activity`,
//! which moves with every event (every text chunk): it is never a reason to send, and rides
//! along, current, with the next row sent for another change. A streaming turn so sends its rows
//! when it starts, waits on the lawyer, and ends, and not per chunk.
//!
//! A matter that leaves the list goes out in `MattersChanged::removed`, and its threads with it
//! (no `ThreadsChanged`). A matter that comes (created, or brought back) goes out as a changed
//! row, and its threads, if any, as one `ThreadsChanged` of new rows.
//!
//! [`current`] is the whole state as `all: true` lists: the first frames of every app
//! subscription and the catch-up of one that lagged (`app.rs`).

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{
    HostEvent, MatterId, MatterSummary, MattersChanged, ThreadId, ThreadSummary, ThreadsChanged,
};

use super::{Actor, rail};

/// What the app subscribers were sent last, and what changed since.
#[derive(Debug, Default)]
pub(crate) struct Lists {
    /// The rows every app subscriber holds; `None` while nobody is subscribed.
    published: Option<Published>,
    /// Compare every row at the next flush.
    all: bool,
    /// Threads whose rows may have changed.
    threads: BTreeSet<ThreadId>,
}

impl Lists {
    /// `thread`'s row may have changed. Kept only while anyone is subscribed.
    pub(crate) fn touch_thread(&mut self, thread: ThreadId) {
        if self.published.is_some() {
            self.threads.insert(thread);
        }
    }

    /// Any row may have changed.
    pub(crate) fn touch_all(&mut self) {
        if self.published.is_some() {
            self.all = true;
        }
    }
}

/// The rows last sent: each listed matter's, and each of their threads' with whether a
/// permission of it waited (which the matter's status shows and the thread's row does not).
#[derive(Debug, Default)]
struct Published {
    matters: BTreeMap<MatterId, MatterSummary>,
    threads: BTreeMap<ThreadId, (ThreadSummary, bool)>,
}

/// Every matter's row in `ListMatters`' order, each with its threads in `ListThreads`' order.
type Whole = Vec<(MatterSummary, Vec<(ThreadSummary, bool)>)>;

fn whole(actor: &Actor) -> Whole {
    let mut threads: BTreeMap<MatterId, Vec<(ThreadSummary, bool)>> = BTreeMap::new();
    for thread in actor.threads.values() {
        if actor.matters.contains_key(&thread.row.matter) {
            threads
                .entry(thread.row.matter)
                .or_default()
                .push(rail::thread_summary(actor, thread));
        }
    }
    rail::summaries(actor)
        .into_iter()
        .map(|matter| {
            let rows = threads.remove(&matter.matter).unwrap_or_default();
            (matter, rows)
        })
        .collect()
}

/// The whole state as `all: true` lists: one `MattersChanged` with every matter, then one
/// `ThreadsChanged` per matter in the list's order (one with no thread included). Starts keeping
/// the rows sent when nobody was subscribed before.
pub(super) fn current(actor: &mut Actor) -> Vec<HostEvent> {
    let whole = whole(actor);
    if actor.lists.published.is_none() {
        let mut published = Published::default();
        for (matter, threads) in &whole {
            published.matters.insert(matter.matter, matter.clone());
            for (thread, pending) in threads {
                published
                    .threads
                    .insert(thread.thread, (thread.clone(), *pending));
            }
        }
        actor.lists.published = Some(published);
    }
    let mut events = Vec::with_capacity(whole.len() + 1);
    let mut lists = Vec::with_capacity(whole.len());
    let mut matters = Vec::with_capacity(whole.len());
    for (matter, threads) in whole {
        lists.push(ThreadsChanged {
            matter: matter.matter,
            threads: threads.into_iter().map(|(thread, _)| thread).collect(),
            removed: Vec::new(),
            all: true,
        });
        matters.push(matter);
    }
    events.push(HostEvent::MattersChanged(MattersChanged {
        matters,
        removed: Vec::new(),
        all: true,
    }));
    events.extend(lists.into_iter().map(HostEvent::ThreadsChanged));
    events
}

/// After each input: sends the rows that changed since the last flush to every app subscriber,
/// and forgets the rows sent once nobody is subscribed.
pub(super) fn flush(actor: &mut Actor) {
    if actor.app_subs.is_empty() {
        if actor.lists.published.is_some() {
            actor.lists = Lists::default();
        }
        return;
    }
    if !actor.lists.all && actor.lists.threads.is_empty() {
        return;
    }
    let Some(mut published) = actor.lists.published.take() else {
        return;
    };
    let all = std::mem::take(&mut actor.lists.all);
    let touched = std::mem::take(&mut actor.lists.threads);
    let events = if all {
        compare_all(actor, &mut published)
    } else {
        compare_threads(actor, &mut published, touched)
    };
    actor.lists.published = Some(published);
    for event in &events {
        super::app::publish(actor, event);
    }
}

/// Compares the touched threads' rows, then the rows of the matters whose threads changed.
fn compare_threads(
    actor: &Actor,
    published: &mut Published,
    touched: BTreeSet<ThreadId>,
) -> Vec<HostEvent> {
    let mut threads: BTreeMap<MatterId, Vec<ThreadSummary>> = BTreeMap::new();
    for id in touched {
        let Some(thread) = actor.threads.get(&id) else {
            continue;
        };
        if !actor.matters.contains_key(&thread.row.matter) {
            continue;
        }
        let row = rail::thread_summary(actor, thread);
        if let Some(changed) = keep_thread(published, row) {
            threads.entry(changed.matter).or_default().push(changed);
        }
    }
    let mut matters = Vec::new();
    for &matter in threads.keys() {
        if let Some(row) = rail::matter_summary(actor, matter)
            && let Some(changed) = keep_matter(published, row)
        {
            matters.push(changed);
        }
    }
    events(matters, Vec::new(), threads)
}

/// Compares every row, and finds the matters that left the list.
fn compare_all(actor: &Actor, published: &mut Published) -> Vec<HostEvent> {
    let mut listed = BTreeSet::new();
    let mut matters = Vec::new();
    let mut threads: BTreeMap<MatterId, Vec<ThreadSummary>> = BTreeMap::new();
    for (matter, rows) in whole(actor) {
        listed.insert(matter.matter);
        for row in rows {
            if let Some(changed) = keep_thread(published, row) {
                threads.entry(changed.matter).or_default().push(changed);
            }
        }
        if let Some(changed) = keep_matter(published, matter) {
            matters.push(changed);
        }
    }
    let removed: Vec<MatterId> = published
        .matters
        .keys()
        .filter(|matter| !listed.contains(matter))
        .copied()
        .collect();
    for matter in &removed {
        published.matters.remove(matter);
    }
    published
        .threads
        .retain(|_, (row, _)| listed.contains(&row.matter));
    events(matters, removed, threads)
}

/// `row` when it differs from the one sent (which it then replaces); `None` when it does not.
fn keep_thread(published: &mut Published, row: (ThreadSummary, bool)) -> Option<ThreadSummary> {
    let same = published
        .threads
        .get(&row.0.thread)
        .is_some_and(|(old, pending)| *pending == row.1 && same_thread(old, &row.0));
    if same {
        return None;
    }
    let changed = row.0.clone();
    published.threads.insert(row.0.thread, row);
    Some(changed)
}

/// `row` when it differs from the one sent (which it then replaces); `None` when it does not.
fn keep_matter(published: &mut Published, row: MatterSummary) -> Option<MatterSummary> {
    if published
        .matters
        .get(&row.matter)
        .is_some_and(|old| same_matter(old, &row))
    {
        return None;
    }
    published.matters.insert(row.matter, row.clone());
    Some(row)
}

/// The events for what changed: the matters first, then each matter's threads.
fn events(
    matters: Vec<MatterSummary>,
    removed: Vec<MatterId>,
    threads: BTreeMap<MatterId, Vec<ThreadSummary>>,
) -> Vec<HostEvent> {
    let mut events = Vec::with_capacity(threads.len() + 1);
    if !matters.is_empty() || !removed.is_empty() {
        events.push(HostEvent::MattersChanged(MattersChanged {
            matters,
            removed,
            all: false,
        }));
    }
    events.extend(threads.into_iter().map(|(matter, threads)| {
        HostEvent::ThreadsChanged(ThreadsChanged {
            matter,
            threads,
            removed: Vec::new(),
            all: false,
        })
    }));
    events
}

/// Whether two matter rows show the same: every field but `last_activity`. Written out field by
/// field, so a new field fails to compile here until it is decided.
fn same_matter(a: &MatterSummary, b: &MatterSummary) -> bool {
    let MatterSummary {
        matter,
        name,
        kind,
        last_activity: _,
        client,
        next_date,
        pinned,
        last_opened,
        status,
        folder_display,
        folder_state,
        folder_support,
    } = a;
    *matter == b.matter
        && *name == b.name
        && *kind == b.kind
        && *client == b.client
        && *next_date == b.next_date
        && *pinned == b.pinned
        && *last_opened == b.last_opened
        && *status == b.status
        && *folder_display == b.folder_display
        && *folder_state == b.folder_state
        && *folder_support == b.folder_support
}

/// Whether two thread rows show the same: every field but `last_activity`.
fn same_thread(a: &ThreadSummary, b: &ThreadSummary) -> bool {
    let ThreadSummary {
        thread,
        matter,
        title,
        agent,
        status,
        queue_held,
        seen,
        snoozed_until,
        done,
        last_stop,
        last_activity: _,
    } = a;
    *thread == b.thread
        && *matter == b.matter
        && *title == b.title
        && *agent == b.agent
        && *status == b.status
        && *queue_held == b.queue_held
        && *seen == b.seen
        && *snoozed_until == b.snoozed_until
        && *done == b.done
        && *last_stop == b.last_stop
}

#[cfg(test)]
mod tests;
