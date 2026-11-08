//! The pipe's instance bookkeeping over a fake [`Pipes`] that, like the kernel, refuses more than
//! [`MAX_INSTANCES`]: across accept, client-gone, failed-connect, failed-listen and
//! send-drops-all sequences the count of open instances never reaches zero, and a replacement
//! is always made before the instance it replaces is closed. A client that connects and leaves
//! before it is serviced (232, 233) is routine: the listener listens again in place, and only a
//! later instance that can't be made ends the run.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::super::Failed;
use super::super::instances::{Connect, Instances, MAX_INSTANCES, Pipes};

/// What the fake's handles are refused.
#[derive(Default, PartialEq, Eq)]
enum Refused {
    #[default]
    Nothing,
    /// A later instance's owner isn't SYSTEM.
    Owner,
    /// `PeekNamedPipe` answers access denied.
    Peek,
}

/// Instances by number, with what each poll or write finds scripted.
#[derive(Default)]
struct FakePipes {
    next: u32,
    open: BTreeSet<u32>,
    /// The fewest instances open just after any close.
    lowest: Option<usize>,
    accepted: HashMap<u32, Connect>,
    gone: HashSet<u32>,
    relisten_fails: bool,
    /// Lines written to each instance's client.
    received: HashMap<u32, usize>,
    writes_fail: bool,
    create_fails: bool,
    refused: Refused,
    calls: Vec<String>,
}

impl FakePipes {
    /// The fake, with the first instance (0) open.
    fn start() -> Instances<FakePipes> {
        let mut pipes = FakePipes::default();
        pipes.open.insert(0);
        pipes.next = 1;
        Instances::new(pipes, 0)
    }

    fn newest(&self) -> u32 {
        self.next - 1
    }
}

impl Pipes for FakePipes {
    type Pipe = u32;

    fn create(&mut self) -> Result<u32, Failed> {
        if self.create_fails || self.open.len() >= MAX_INSTANCES {
            self.calls.push("create failed".to_owned());
            return Err("CreateNamedPipeW failed (231)".to_owned());
        }
        let pipe = self.next;
        self.next += 1;
        self.open.insert(pipe);
        self.calls.push(format!("create {pipe}"));
        Ok(pipe)
    }

    fn owned_by_system(&mut self, _pipe: &u32) -> Result<bool, Failed> {
        Ok(self.refused != Refused::Owner)
    }

    fn accepted(&mut self, pipe: &mut u32) -> Connect {
        self.accepted.get(pipe).copied().unwrap_or(Connect::Waiting)
    }

    fn relisten(&mut self, pipe: &mut u32) -> Result<(), Failed> {
        assert!(self.open.contains(pipe), "{pipe} relistened closed");
        if self.relisten_fails {
            self.calls.push(format!("relisten {pipe} failed"));
            return Err("ConnectNamedPipe failed (5)".to_owned());
        }
        self.calls.push(format!("relisten {pipe}"));
        self.accepted.remove(pipe);
        Ok(())
    }

    fn alive(&mut self, pipe: &u32) -> Result<bool, Failed> {
        if self.refused == Refused::Peek {
            return Err("PeekNamedPipe was refused on the server end".to_owned());
        }
        Ok(!self.gone.contains(pipe))
    }

    fn write(&mut self, pipe: &u32, _line: &[u8]) -> bool {
        let went = !self.writes_fail && !self.gone.contains(pipe);
        if went {
            *self.received.entry(*pipe).or_default() += 1;
        }
        went
    }

    fn close(&mut self, pipe: u32) {
        assert!(self.open.remove(&pipe), "{pipe} closed twice");
        self.calls.push(format!("close {pipe}"));
        let left = self.open.len();
        self.lowest = Some(self.lowest.map_or(left, |low| low.min(left)));
    }
}

/// The current listener's client connects, and the instances are polled.
fn connect(inst: &mut Instances<FakePipes>) -> Result<usize, Failed> {
    let pipe = inst.pipes().newest();
    inst.pipes().accepted.insert(pipe, Connect::Connected);
    inst.poll()
}

/// Neither the bookkeeping nor the kernel ever went to zero instances.
fn never_zero(inst: &mut Instances<FakePipes>) {
    assert!(inst.count() >= 1);
    let pipes = inst.pipes();
    assert!(!pipes.open.is_empty(), "{:?}", pipes.calls);
    assert!(pipes.lowest.is_none_or(|low| low >= 1), "{:?}", pipes.calls);
}

/// Where `call` is in the fake's calls.
fn at(inst: &mut Instances<FakePipes>, call: &str) -> usize {
    let calls = &inst.pipes().calls;
    calls
        .iter()
        .position(|c| c == call)
        .unwrap_or_else(|| panic!("{call}: {calls:?}"))
}

#[test]
fn a_client_that_connects_and_leaves_within_one_poll_never_frees_the_name() {
    let mut inst = FakePipes::start();
    inst.pipes().gone.insert(0);
    assert_eq!(connect(&mut inst), Ok(0));
    assert!(at(&mut inst, "create 1") < at(&mut inst, "close 0"));
    assert_eq!(inst.count(), 1);
    never_zero(&mut inst);
}

#[test]
fn an_accepted_listener_is_replaced_before_it_leaves_the_slot_and_a_gone_client_closes() {
    let mut inst = FakePipes::start();
    assert_eq!(connect(&mut inst), Ok(1));
    assert_eq!(inst.count(), 2);
    inst.pipes().gone.insert(0);
    assert_eq!(inst.poll(), Ok(0));
    assert_eq!(inst.count(), 1);
    never_zero(&mut inst);
}

#[test]
fn a_failed_connect_is_closed_only_after_its_replacement() {
    let mut inst = FakePipes::start();
    inst.pipes().accepted.insert(0, Connect::Failed);
    assert_eq!(inst.poll(), Ok(0));
    assert!(at(&mut inst, "create 1") < at(&mut inst, "close 0"));
    never_zero(&mut inst);
}

#[test]
fn a_listen_that_fails_ends_the_run_with_the_instances_kept() {
    // On accept: the accepted instance stays held, to close after the name is deleted.
    let mut inst = FakePipes::start();
    inst.pipes().create_fails = true;
    assert!(connect(&mut inst).is_err());
    assert_eq!(inst.count(), 1);
    never_zero(&mut inst);
    // On a failed connect: the failed one stays until the name is deleted.
    let mut inst = FakePipes::start();
    inst.pipes().create_fails = true;
    inst.pipes().accepted.insert(0, Connect::Failed);
    assert!(inst.poll().is_err());
    never_zero(&mut inst);
    // On every client gone at the cap, with no listener: the last is kept rather than closed.
    let mut inst = FakePipes::start();
    for _ in 0..MAX_INSTANCES {
        connect(&mut inst).unwrap();
    }
    inst.pipes().gone.extend(0..8);
    inst.pipes().create_fails = true;
    assert!(inst.poll().is_err());
    assert_eq!(inst.count(), 1);
    never_zero(&mut inst);
    // Then the run closes all, once the name is deleted.
    inst.close_all();
    assert!(inst.pipes().open.is_empty());
}

#[test]
fn a_later_instance_not_owned_by_system_ends_the_run() {
    let mut inst = FakePipes::start();
    inst.pipes().refused = Refused::Owner;
    assert!(connect(&mut inst).unwrap_err().contains("SYSTEM"));
    // The foreign one is closed; ours is still held.
    assert_eq!(inst.count(), 1);
    never_zero(&mut inst);
}

#[test]
fn at_the_cap_every_client_gone_at_once_leaves_a_listener() {
    let mut inst = FakePipes::start();
    for n in 1..=MAX_INSTANCES {
        assert_eq!(connect(&mut inst), Ok(n));
        never_zero(&mut inst);
    }
    // All eight are clients, and no ninth instance could be made.
    assert_eq!(
        (inst.clients(), inst.count()),
        (MAX_INSTANCES, MAX_INSTANCES)
    );
    for pipe in 0..8 {
        inst.pipes().gone.insert(pipe);
    }
    assert_eq!(inst.poll(), Ok(0));
    assert_eq!(inst.count(), 1);
    never_zero(&mut inst);
}

#[test]
fn a_send_that_drops_every_client_keeps_an_instance() {
    // With a listener open.
    let mut inst = FakePipes::start();
    connect(&mut inst).unwrap();
    connect(&mut inst).unwrap();
    inst.pipes().writes_fail = true;
    assert_eq!(inst.send(b"x\n"), Ok(()));
    assert_eq!(inst.clients(), 0);
    never_zero(&mut inst);
    // At the cap, with none: a listener is made before the last client closes.
    let mut inst = FakePipes::start();
    for _ in 0..MAX_INSTANCES {
        connect(&mut inst).unwrap();
    }
    inst.pipes().writes_fail = true;
    assert_eq!(inst.send(b"x\n"), Ok(()));
    assert_eq!(inst.count(), 1);
    never_zero(&mut inst);
    // And when that listener can't be made, the run ends with the last client kept.
    let mut inst = FakePipes::start();
    for _ in 0..MAX_INSTANCES {
        connect(&mut inst).unwrap();
    }
    inst.pipes().writes_fail = true;
    inst.pipes().create_fails = true;
    assert!(inst.send(b"x\n").is_err());
    never_zero(&mut inst);
}

#[test]
fn a_long_mixed_run_never_reaches_zero() {
    let mut inst = FakePipes::start();
    for round in 0u32..200 {
        match round % 5 {
            0 | 1 => drop(connect(&mut inst)),
            2 => {
                let newest = inst.pipes().newest();
                inst.pipes().gone.extend(newest.saturating_sub(3)..=newest);
                drop(inst.poll());
            }
            3 => {
                let newest = inst.pipes().newest();
                inst.pipes().accepted.insert(newest, Connect::Failed);
                drop(inst.poll());
            }
            _ => {
                inst.pipes().writes_fail = round % 10 == 4;
                drop(inst.send(b"x\n"));
                inst.pipes().writes_fail = false;
            }
        }
        never_zero(&mut inst);
        assert!(inst.pipes().open.len() <= MAX_INSTANCES);
    }
}

#[test]
fn a_peek_the_handle_may_not_make_ends_the_run_and_drops_no_client() {
    let mut inst = FakePipes::start();
    connect(&mut inst).unwrap();
    connect(&mut inst).unwrap();
    inst.pipes().gone.insert(0);
    inst.pipes().refused = Refused::Peek;
    assert!(inst.poll().unwrap_err().contains("refused"));
    // Nothing closed: the run deletes the name, then closes them all.
    assert_eq!((inst.clients(), inst.count()), (2, 3));
    assert!(!inst.pipes().calls.iter().any(|c| c.starts_with("close")));
    never_zero(&mut inst);
}

/// The current listener's client connects and leaves before it is serviced (232 or 233, from
/// the connect or its completion), and the instances are polled.
fn come_and_go(inst: &mut Instances<FakePipes>) -> Result<usize, Failed> {
    let pipe = inst.pipes().newest();
    inst.pipes().accepted.insert(pipe, Connect::Left);
    inst.poll()
}

#[test]
fn a_client_gone_before_it_was_serviced_is_dropped_and_the_long_lived_one_keeps_receiving() {
    let mut inst = FakePipes::start();
    // The long-lived reader: instance 0.
    assert_eq!(connect(&mut inst), Ok(1));
    // At the listener (1): its client left before the connect was serviced.
    assert_eq!(come_and_go(&mut inst), Ok(1));
    assert!(inst.pipes().calls.contains(&"relisten 1".to_owned()));
    assert_eq!(inst.count(), 2);
    // A client that connected (2 replaces 1), then left before its first line: dropped.
    assert_eq!(connect(&mut inst), Ok(2));
    inst.pipes().gone.insert(1);
    assert_eq!(inst.send(b"x\n"), Ok(()));
    assert_eq!(inst.clients(), 1);
    assert_eq!(inst.send(b"y\n"), Ok(()));
    assert_eq!(inst.pipes().received.get(&0), Some(&2));
    assert_eq!(inst.poll(), Ok(1));
    never_zero(&mut inst);
}

#[test]
fn a_rapid_connect_disconnect_loop_never_ends_the_run() {
    let mut inst = FakePipes::start();
    connect(&mut inst).unwrap();
    for round in 0u32..500 {
        match round % 4 {
            // Gone before the connect was serviced: listens again in place.
            0 | 1 => assert_eq!(come_and_go(&mut inst), Ok(1)),
            // Connected, then gone by the next poll: dropped, the reader kept.
            2 => {
                assert_eq!(connect(&mut inst), Ok(2));
                let newest = inst.pipes().newest();
                inst.pipes().gone.insert(newest - 1);
                assert_eq!(inst.poll(), Ok(1));
            }
            _ => assert_eq!(inst.send(b"x\n"), Ok(())),
        }
        never_zero(&mut inst);
        assert!(inst.count() <= 2);
    }
    assert_eq!(inst.pipes().received.get(&0), Some(&125));
}

#[test]
fn a_listener_that_cant_listen_again_is_replaced_and_only_a_failed_create_ends_the_run() {
    let mut inst = FakePipes::start();
    connect(&mut inst).unwrap();
    inst.pipes().relisten_fails = true;
    assert_eq!(come_and_go(&mut inst), Ok(1));
    assert!(at(&mut inst, "create 2") < at(&mut inst, "close 1"));
    never_zero(&mut inst);
    // A genuine failure to keep a listener: no later instance can be made. 74.
    inst.pipes().create_fails = true;
    assert!(come_and_go(&mut inst).is_err());
    assert_eq!(inst.count(), 2);
    never_zero(&mut inst);
    // A 232 or 233 alone, with creates failing, never ends it: the instance is reused.
    let mut inst = FakePipes::start();
    connect(&mut inst).unwrap();
    inst.pipes().create_fails = true;
    for _ in 0..20 {
        assert_eq!(come_and_go(&mut inst), Ok(1));
    }
    never_zero(&mut inst);
}
