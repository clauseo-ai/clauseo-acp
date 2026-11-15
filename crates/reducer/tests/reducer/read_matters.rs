//! The other matters a thread may read (`MatterReadAllowed`, `MatterReadWithdrawn`): kept on
//! the header in the order allowed, each once, through a snapshot at this schema, and left
//! alone by the wake rule.

use clauseo_protocol::{
    DomainEvent, HeaderEntry, MatterId, MatterReadAllowed, MatterReadWithdrawn,
};
use clauseo_reducer::{REDUCER_SCHEMA_VERSION, Snapshot, StreamState, wakes_thread};

use crate::fixtures::{Log, matter_id, start_turn, thread_created};
use crate::helpers::fold;

fn allowed(matter: MatterId) -> DomainEvent {
    MatterReadAllowed { matter }.into()
}

fn withdrawn(matter: MatterId) -> DomainEvent {
    MatterReadWithdrawn { matter }.into()
}

fn header(state: &StreamState) -> &HeaderEntry {
    state.as_thread().unwrap().header().unwrap()
}

#[test]
fn allowed_matters_are_kept_in_order_once_and_withdrawn_alone() {
    let (b, c) = (matter_id(21).unwrap(), matter_id(22).unwrap());
    let mut log = Log::thread(31).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    assert!(header(&fold(log.events())).read_matters.is_empty());
    log.push(allowed(b)).unwrap();
    log.push(allowed(c)).unwrap();
    log.push(allowed(b)).unwrap();
    assert_eq!(header(&fold(log.events())).read_matters, [b, c]);
    log.push(withdrawn(b)).unwrap();
    assert_eq!(header(&fold(log.events())).read_matters, [c]);
    // Withdrawing one not allowed changes nothing.
    log.push(withdrawn(b)).unwrap();
    assert_eq!(header(&fold(log.events())).read_matters, [c]);
    log.push(allowed(b)).unwrap();
    assert_eq!(header(&fold(log.events())).read_matters, [c, b]);
}

#[test]
fn neither_event_wakes_and_a_turn_leaves_them() {
    let b = matter_id(23).unwrap();
    assert!(!wakes_thread(&allowed(b)));
    assert!(!wakes_thread(&withdrawn(b)));
    let mut log = Log::thread(32).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    log.push(allowed(b)).unwrap();
    start_turn(&mut log, "Read the other lease.").unwrap();
    assert_eq!(header(&fold(log.events())).read_matters, [b]);
}

#[test]
fn allowed_matters_survive_a_snapshot_at_this_schema() {
    assert_eq!(REDUCER_SCHEMA_VERSION, 13);
    let b = matter_id(24).unwrap();
    let mut log = Log::thread(33).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    log.push(allowed(b)).unwrap();
    let state = fold(log.events());
    let bytes = Snapshot::of(&state).to_bytes().unwrap();
    let read = Snapshot::from_bytes(&bytes).unwrap().into_state().unwrap();
    assert_eq!(read, state);
    assert_eq!(header(&read).read_matters, [b]);
}
