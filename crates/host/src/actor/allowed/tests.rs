//! The notices an allow can name: each thread's latest [`MAX_NOTICES_PER_THREAD`], at most
//! [`MAX_NOTICES`] in all, a re-sent one kept once.

use clauseo_protocol::{BlockedSiteId, MatterId, ThreadId};
use uuid::Uuid;

use super::{AllowState, MAX_NOTICES, MAX_NOTICES_PER_THREAD};
use crate::pipeline::tests::support::TempDir;

fn id() -> BlockedSiteId {
    BlockedSiteId::new(Uuid::now_v7()).unwrap()
}

fn thread() -> ThreadId {
    ThreadId::new(Uuid::now_v7()).unwrap()
}

#[test]
fn a_threads_latest_notices_are_kept_and_its_oldest_dropped() {
    let dir = TempDir::new("allowed-notices");
    let matter = MatterId::new(Uuid::now_v7()).unwrap();
    let looping = thread();
    let mut state = AllowState::new(dir.path());
    let first = id();
    state.noticed(first, "first.test", matter, looping);
    state.noticed(first, "first.test", matter, looping);
    let second = id();
    state.noticed(second, "second.test", matter, looping);
    assert_eq!(state.notices.len(), 2);
    for n in 0..MAX_NOTICES_PER_THREAD - 1 {
        state.noticed(id(), &format!("h{n}.test"), matter, looping);
    }
    assert_eq!(state.notices.len(), MAX_NOTICES_PER_THREAD);
    assert!(state.notice(first).is_none());
    let kept = state.notice(second).unwrap();
    assert_eq!((kept.host.as_str(), kept.matter), ("second.test", matter));
}

/// An agent looping in one thread drops only that thread's notices, never another's.
#[test]
fn one_threads_flood_keeps_another_threads_notice() {
    let dir = TempDir::new("allowed-notices-flood");
    let matter = MatterId::new(Uuid::now_v7()).unwrap();
    let mut state = AllowState::new(dir.path());
    let quiet = id();
    state.noticed(quiet, "quiet.test", matter, thread());
    let looping = thread();
    for n in 0..MAX_NOTICES * 2 {
        state.noticed(id(), &format!("h{n}.test"), matter, looping);
    }
    assert_eq!(state.notices.len(), MAX_NOTICES_PER_THREAD + 1);
    assert_eq!(state.notice(quiet).unwrap().host, "quiet.test");
}

/// Past [`MAX_NOTICES`] in all, the oldest of any thread goes first.
#[test]
fn the_notices_in_all_are_capped() {
    let dir = TempDir::new("allowed-notices-all");
    let matter = MatterId::new(Uuid::now_v7()).unwrap();
    let mut state = AllowState::new(dir.path());
    let oldest = id();
    state.noticed(oldest, "oldest.test", matter, thread());
    while state.notices.len() < MAX_NOTICES {
        let busy = thread();
        for n in 0..MAX_NOTICES_PER_THREAD.min(MAX_NOTICES - state.notices.len()) {
            state.noticed(id(), &format!("h{n}.test"), matter, busy);
        }
    }
    let newest = id();
    state.noticed(newest, "newest.test", matter, thread());
    assert_eq!(state.notices.len(), MAX_NOTICES);
    assert!(state.notice(oldest).is_none());
    assert!(state.notice(newest).is_some());
}
