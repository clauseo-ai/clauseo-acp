//! The notice's bookkeeping: one id per site per turn, counts, a fresh start per turn, the cap,
//! and forgetting a turn that ended.

use clauseo_protocol::{BlockedSiteId, ThreadId, TurnId};
use uuid::Uuid;

use super::{Blocks, MAX_HOSTS_PER_TURN, Noted};
use crate::error::Error;

/// A v7 UUID whose low bits are `n`.
fn v7(n: u64) -> Uuid {
    let timestamp: u128 = 0x0190_0000_0000;
    let bits = (timestamp << 80) | (0x7 << 76) | (0b10 << 62) | u128::from(n);
    Uuid::from_u128(bits)
}

fn thread(n: u64) -> ThreadId {
    ThreadId::new(v7(n)).unwrap()
}

fn turn_id(n: u64) -> TurnId {
    TurnId::new(v7(100 + n)).unwrap()
}

/// Mints ids 1000, 1001, ... in order.
struct Minter(u64);

impl Minter {
    fn mint(&mut self) -> impl FnOnce() -> Result<BlockedSiteId, Error> + '_ {
        move || {
            self.0 += 1;
            Ok(BlockedSiteId::new(v7(999 + self.0)).unwrap())
        }
    }
}

fn id(n: u64) -> BlockedSiteId {
    BlockedSiteId::new(v7(999 + n)).unwrap()
}

fn send(n: u64, count: u32) -> Noted {
    Noted::Send {
        block_id: id(n),
        count,
    }
}

#[test]
fn a_repeat_in_the_turn_keeps_the_id_and_counts_and_another_host_gets_its_own() {
    let (mut blocks, mut ids) = (Blocks::default(), Minter(0));
    let t = thread(1);
    let note = |blocks: &mut Blocks, ids: &mut Minter, host: &str| {
        blocks.note(t, Some(turn_id(1)), host, ids.mint()).unwrap()
    };
    assert_eq!(note(&mut blocks, &mut ids, "evil.example"), send(1, 1));
    assert_eq!(note(&mut blocks, &mut ids, "evil.example"), send(1, 2));
    // Case does not make another site.
    assert_eq!(note(&mut blocks, &mut ids, "EVIL.example"), send(1, 3));
    assert_eq!(note(&mut blocks, &mut ids, "phish.test"), send(2, 1));
    // Another thread's turn has its own notices.
    let other = blocks
        .note(thread(2), Some(turn_id(2)), "evil.example", ids.mint())
        .unwrap();
    assert_eq!(other, send(3, 1));
}

#[test]
fn a_new_turn_starts_fresh_and_no_turn_counts_too() {
    let (mut blocks, mut ids) = (Blocks::default(), Minter(0));
    let t = thread(1);
    let host = "1.2.3.4";
    assert_eq!(blocks.note(t, None, host, ids.mint()).unwrap(), send(1, 1));
    assert_eq!(blocks.note(t, None, host, ids.mint()).unwrap(), send(1, 2));
    assert_eq!(
        blocks.note(t, Some(turn_id(1)), host, ids.mint()).unwrap(),
        send(2, 1)
    );
    assert_eq!(
        blocks.note(t, Some(turn_id(2)), host, ids.mint()).unwrap(),
        send(3, 1)
    );
    assert_eq!(
        blocks.note(t, Some(turn_id(2)), host, ids.mint()).unwrap(),
        send(3, 2)
    );
}

#[test]
fn past_the_cap_a_new_host_is_dropped_and_a_known_one_still_counts() {
    let (mut blocks, mut ids) = (Blocks::default(), Minter(0));
    let t = thread(1);
    for n in 0..MAX_HOSTS_PER_TURN {
        let noted = blocks.note(t, Some(turn_id(1)), &format!("h{n}.test"), ids.mint());
        assert!(
            matches!(noted, Ok(Noted::Send { count: 1, .. })),
            "{noted:?}"
        );
    }
    let past = |blocks: &mut Blocks, ids: &mut Minter| {
        blocks
            .note(t, Some(turn_id(1)), "one-more.test", ids.mint())
            .unwrap()
    };
    assert_eq!(past(&mut blocks, &mut ids), Noted::Capped { first: true });
    assert_eq!(past(&mut blocks, &mut ids), Noted::Capped { first: false });
    assert_eq!(
        blocks
            .note(t, Some(turn_id(1)), "h0.test", ids.mint())
            .unwrap(),
        send(1, 2)
    );
    // A new turn has room again.
    assert_eq!(
        blocks
            .note(t, Some(turn_id(2)), "one-more.test", ids.mint())
            .unwrap(),
        send(65, 1)
    );
}

#[test]
fn a_turn_no_longer_running_is_forgotten() {
    let (mut blocks, mut ids) = (Blocks::default(), Minter(0));
    blocks
        .note(thread(1), Some(turn_id(1)), "a.test", ids.mint())
        .unwrap();
    blocks
        .note(thread(2), Some(turn_id(2)), "a.test", ids.mint())
        .unwrap();
    // Thread 1's turn ended; thread 2's runs.
    blocks.forget_ended(|t| {
        if t == thread(2) {
            Some(turn_id(2))
        } else {
            None
        }
    });
    assert_eq!(blocks.threads(), 1);
    blocks.forget_ended(|_| None);
    assert_eq!(blocks.threads(), 0);
}

#[test]
fn a_failed_mint_notes_nothing() {
    let mut blocks = Blocks::default();
    let failed = blocks.note(thread(1), Some(turn_id(1)), "a.test", || {
        Err(Error::Internal("no id".to_owned()))
    });
    assert!(failed.is_err());
    let mut ids = Minter(0);
    assert_eq!(
        blocks
            .note(thread(1), Some(turn_id(1)), "a.test", ids.mint())
            .unwrap(),
        send(1, 1)
    );
}
