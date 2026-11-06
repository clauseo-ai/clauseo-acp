//! The count of started processes this process holds, and the gate that closes new starts at
//! exit. [`spawn_in_tree`](crate::spawn_in_tree) takes a [`Live`] slot before anything else:
//! the count rises first, then the gate is read, both `SeqCst`. [`close_spawns`] closes the gate
//! first, then its caller reads the count. So a start either sees the gate closed and is refused
//! (`Error::Closing`), or was counted before the closer reads: a zero after `close_spawns`
//! means no tree started here runs, and none will. The gate never reopens.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::Error;

/// How many [`Live`] slots exist: one per start in progress or [`Handle`](crate::Handle) held.
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// Whether [`close_spawns`] has run.
static CLOSED: AtomicBool = AtomicBool::new(false);

/// How many processes this process has started, or is starting, and still holds a
/// [`Handle`](crate::Handle) for: each counts from the first step of
/// [`spawn_in_tree`](crate::spawn_in_tree), before the child exists, until its handle is
/// dropped (or the start fails), whether or not it has exited. One atomic read.
pub fn live_handles() -> usize {
    LIVE.load(Ordering::SeqCst)
}

/// Closes the gate for good: every later [`spawn_in_tree`](crate::spawn_in_tree) is refused
/// with `Error::Closing` and starts nothing. Desktop calls it at exit, then reads
/// [`live_handles`]. Never reopened: the process is ending.
pub fn close_spawns() {
    CLOSED.store(true, Ordering::SeqCst);
}

/// One counted start: taken first in `spawn_in_tree`, carried by the handle, and given back
/// when it drops (the start failed, or the handle was dropped).
#[derive(Debug)]
pub(crate) struct Live(());

impl Live {
    /// Counts a start, then reads the gate: closed, the count is given back and the start is
    /// refused.
    pub(crate) fn enter() -> Result<Live, Error> {
        LIVE.fetch_add(1, Ordering::SeqCst);
        let slot = Live(());
        if CLOSED.load(Ordering::SeqCst) {
            drop(slot);
            return Err(Error::Closing);
        }
        Ok(slot)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
