//! Which pool user a matter gets (S15 `PR4c`, R14). A matter that holds one keeps it. Otherwise
//! a pool user may go to it only when all of these hold:
//!
//! - no matter in `matters.json` holds it ([`MatterRow::holds`]);
//! - no hand-back is owed for it ([`Ledger::handback_owed`], a kept owed entry included);
//! - no ledger entry names it, in any state ([`Ledger::names_sid`], a kept entry included);
//! - it owns no item set aside and still on disk ([`SetAsideOwners`]): a moved item keeps its
//!   owner, who has the implied `WRITE_DAC` on it;
//! - no matter that held it, now or before ([`MatterRow::held_before`]), has an open session
//!   ([`OpenSessions`]).
//!
//! So a pool user is never shared between live sessions of different matters (D-B5); two
//! sessions of one matter share its user. Among the free users, one never used goes first, in
//! pool order, then the one released longest ago. With none free, [`Assign::PoolFull`] names each
//! user and why it is held back, which PR6 turns into tier C with its cause.
//!
//! Pure: the decision reads the rows, the ledger, the set-aside owners and the open sessions
//! given. `MatterSids::assign` reads the rows, the ledger and the set-aside owners inside
//! `matters.lock`, makes the decision there and saves the result.

use std::collections::BTreeSet;

use clauseo_protocol::MatterId;

use crate::SetupError;
use crate::windows::ledger::Ledger;
use crate::windows::matters::{MatterRow, Rows};
use crate::windows::sids::Sid;

/// The matters with an open session: a session launched whose after-session pass hasn't run,
/// running or ended without one. That is the set-aside record's open-sessions set, which
/// `clean_close` reads per matter through `set_aside::under_lock`; the caller gathers it.
///
/// The caller's read can be stale, so it isn't the only guard. The rule for a launch: its
/// session is marked open (`before_launch`) **before** `MatterSids::assign`. `assign` enforces
/// it for a new pick, refusing a matter with no open session in its set-aside record, and
/// `MatterSids::release` refuses while the matter has an open session; both read the record
/// under the matter's set-aside lock inside `matters.lock`. So no release can fall between a
/// launch's `assign` and its session, a matter that holds a user keeps it (`Held`) while any
/// session of it is open, and a released matter launched again gets a fresh pick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenSessions {
    matters: BTreeSet<MatterId>,
}

impl OpenSessions {
    /// The matters in `matters` have an open session; no other matter has.
    pub fn new(matters: impl IntoIterator<Item = MatterId>) -> OpenSessions {
        OpenSessions {
            matters: matters.into_iter().collect(),
        }
    }

    /// Whether `matter` has an open session.
    pub fn is_open(&self, matter: &MatterId) -> bool {
        self.matters.contains(matter)
    }

    /// Whether no matter has an open session.
    pub fn is_empty(&self) -> bool {
        self.matters.is_empty()
    }
}

/// Why a pool user can't go to another matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// A hand-back is still owed for it (a forget hasn't finished).
    Owed,
    /// A ledger entry names it.
    Entries,
    /// A matter that held it has an open session.
    Open,
    /// This matter holds it.
    Held(MatterId),
    /// It owns an item set aside and still on disk, or a set-aside manifest that might name it
    /// can't be read.
    SetAside,
}

/// The owners of the items set aside and still on disk, from every matter's manifest; or all
/// users, when a manifest or an owner can't be read (failing closed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetAsideOwners {
    /// Each owner's SID, upper-cased.
    owners: BTreeSet<String>,
    /// Every user is held: something that might name any of them can't be read.
    all: bool,
}

impl SetAsideOwners {
    /// Every user held.
    pub fn all() -> SetAsideOwners {
        SetAsideOwners {
            owners: BTreeSet::new(),
            all: true,
        }
    }

    /// The owners `owners` (SID strings), and no others.
    pub fn new<S: AsRef<str>>(owners: impl IntoIterator<Item = S>) -> SetAsideOwners {
        SetAsideOwners {
            owners: owners
                .into_iter()
                .map(|o| o.as_ref().to_ascii_uppercase())
                .collect(),
            all: false,
        }
    }

    /// Adds a manifest entry's `owner` field: a string is that owner; missing or null (the
    /// owner couldn't be read at the move) holds every user. False for any other value.
    pub(crate) fn add(&mut self, owner: Option<&serde_json::Value>) -> bool {
        match owner {
            Some(serde_json::Value::String(sid)) => {
                self.owners.insert(sid.to_ascii_uppercase());
                true
            }
            None | Some(serde_json::Value::Null) => {
                self.all = true;
                true
            }
            Some(_) => false,
        }
    }

    /// Adds `owner`, read from an item or a child in an item's tree.
    pub(crate) fn add_owner(&mut self, owner: &Sid) {
        self.owners.insert(owner.as_str().to_ascii_uppercase());
    }

    /// Whether `user` is held: it owns an item, or every user is.
    pub fn holds(&self, user: &Sid) -> bool {
        self.all || self.owners.contains(&user.as_str().to_ascii_uppercase())
    }
}

/// A pool user held back, and why: the first reason found, checked in the order `Held`, `Owed`,
/// `Entries`, `SetAside`, `Open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldBack {
    /// The pool user's SID.
    pub pool: Sid,
    /// Why it is held back.
    pub why: Why,
}

/// Why no pool user was assigned.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Assign {
    /// No pool user is free: each of the pool's `size` users is in `held_back`, in pool order.
    #[error("no sandbox user is free: the pool has {size}, all held back")]
    PoolFull {
        /// How many users the pool has.
        size: usize,
        /// Each user and why it is held back.
        held_back: Vec<HeldBack>,
    },
    /// `matters.json` or the ACL ledger couldn't be read, checked or saved, or the matter holds a
    /// user not in the pool.
    #[error(transparent)]
    Records(#[from] SetupError),
}

/// What [`decide`] found for a matter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Pick {
    /// The matter holds this user already.
    Kept(Sid),
    /// This user is free and goes to the matter.
    New(Sid),
}

/// The pool user `matter` gets from `pool`, given every matter's row, the ledger, the set-aside
/// owners and the matters with an open session, by the rules in this module's doc. A matter that
/// holds a user not in `pool` is refused with [`Assign::Records`]: the machine record lost it.
pub(crate) fn decide(
    matter: &MatterId,
    rows: &Rows,
    pool: &[Sid],
    ledger: &Ledger,
    set_aside: &SetAsideOwners,
    open: &OpenSessions,
) -> Result<Pick, Assign> {
    if let Some(user) = rows.get(matter).and_then(MatterRow::holds) {
        if !pool.contains(user) {
            return Err(Assign::Records(SetupError::Failed(format!(
                "{matter}'s sandbox user {user} is not in this machine's pool"
            ))));
        }
        return Ok(Pick::Kept(user.clone()));
    }
    let mut seen = BTreeSet::new();
    let mut held_back = Vec::new();
    let mut free = Vec::new();
    for (index, user) in pool.iter().enumerate() {
        if !seen.insert(user) {
            continue;
        }
        match held_back_why(user, rows, ledger, set_aside, open) {
            Some(why) => held_back.push(HeldBack {
                pool: user.clone(),
                why,
            }),
            None => free.push((last_released(user, rows), index, user)),
        }
    }
    // Never used (`None`) first, then the earliest release; pool order breaks ties.
    let pick = free
        .into_iter()
        .min_by_key(|&(released, index, _)| (released.is_some(), released, index));
    match pick {
        Some((_, _, user)) => Ok(Pick::New(user.clone())),
        None => Err(Assign::PoolFull {
            size: seen.len(),
            held_back,
        }),
    }
}

/// Why `user` can't go to another matter, or `None` when it may.
fn held_back_why(
    user: &Sid,
    rows: &Rows,
    ledger: &Ledger,
    set_aside: &SetAsideOwners,
    open: &OpenSessions,
) -> Option<Why> {
    if let Some((holder, _)) = rows.iter().find(|(_, row)| row.holds() == Some(user)) {
        return Some(Why::Held(*holder));
    }
    if ledger.handback_owed(user) {
        return Some(Why::Owed);
    }
    if ledger.names_sid(user) {
        return Some(Why::Entries);
    }
    if set_aside.holds(user) {
        return Some(Why::SetAside);
    }
    let held_by_open = rows
        .iter()
        .any(|(matter, row)| row.named(user) && open.is_open(matter));
    held_by_open.then_some(Why::Open)
}

/// When `user` was last released, or `None` when no row names it: never used. A user only in
/// rows' `held_before` counts as released at 0, as its time isn't kept.
fn last_released(user: &Sid, rows: &Rows) -> Option<u64> {
    rows.values()
        .filter_map(|row| {
            if row.pool_user.as_ref() == Some(user) {
                row.released_at
            } else {
                row.held_before.contains(user).then_some(0)
            }
        })
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;
    use crate::windows::ledger::ledger_path;
    use crate::windows::sids::Reserved;

    fn sid(rid: u32) -> Sid {
        Sid::parse(&format!("S-1-5-21-1111111111-2222222222-3333333333-{rid}")).unwrap()
    }

    fn matter(n: u8) -> MatterId {
        format!("01890a5d-ac96-774b-8cce-b302099a80{n:02}")
            .parse()
            .unwrap()
    }

    fn released(user: Sid, at: u64) -> MatterRow {
        MatterRow {
            sid: None,
            pool_user: Some(user),
            released_at: Some(at),
            held_before: Vec::new(),
        }
    }

    fn empty_ledger(temp: &TempDir) -> Ledger {
        Ledger::open(ledger_path(temp.path()), &[], &Reserved::default()).unwrap()
    }

    #[test]
    fn never_used_users_go_first_then_the_longest_free() {
        let temp = TempDir::new("assign-order");
        let ledger = empty_ledger(&temp);
        let pool = [sid(1001), sid(1002), sid(1003), sid(1004)];
        let open = OpenSessions::default();
        let mut rows = Rows::new();
        rows.insert(matter(1), released(sid(1001), 300));
        rows.insert(matter(2), released(sid(1002), 100));
        rows.insert(matter(3), released(sid(1003), 200));
        // 1004 is never used, so it goes first though it is last in the pool.
        let new = matter(9);
        let pick = decide(
            &new,
            &rows,
            &pool,
            &ledger,
            &SetAsideOwners::default(),
            &open,
        )
        .unwrap();
        assert_eq!(pick, Pick::New(sid(1004)));
        rows.insert(matter(4), released(sid(1004), 400));
        // Then the one released longest ago, whatever its place in the pool.
        let pick = decide(
            &new,
            &rows,
            &pool,
            &ledger,
            &SetAsideOwners::default(),
            &open,
        )
        .unwrap();
        assert_eq!(pick, Pick::New(sid(1002)));
        // A later row for 1002 counts: its last release is what matters.
        rows.insert(matter(5), released(sid(1002), 500));
        let pick = decide(
            &new,
            &rows,
            &pool,
            &ledger,
            &SetAsideOwners::default(),
            &open,
        )
        .unwrap();
        assert_eq!(pick, Pick::New(sid(1003)));
    }

    #[test]
    fn ties_go_in_pool_order() {
        let temp = TempDir::new("assign-ties");
        let ledger = empty_ledger(&temp);
        let open = OpenSessions::default();
        let pool = [sid(1003), sid(1001), sid(1002)];
        let rows = Rows::new();
        let pick = decide(
            &matter(9),
            &rows,
            &pool,
            &ledger,
            &SetAsideOwners::default(),
            &open,
        )
        .unwrap();
        assert_eq!(pick, Pick::New(sid(1003)));
        let mut rows = Rows::new();
        rows.insert(matter(1), released(sid(1001), 50));
        rows.insert(matter(2), released(sid(1002), 50));
        rows.insert(matter(3), released(sid(1003), 60));
        let pick = decide(
            &matter(9),
            &rows,
            &pool,
            &ledger,
            &SetAsideOwners::default(),
            &open,
        )
        .unwrap();
        assert_eq!(pick, Pick::New(sid(1001)));
    }

    #[test]
    fn a_held_user_outside_the_pool_is_refused() {
        let temp = TempDir::new("assign-lost");
        let ledger = empty_ledger(&temp);
        let mut rows = Rows::new();
        rows.insert(
            matter(1),
            MatterRow {
                sid: None,
                pool_user: Some(sid(1009)),
                released_at: None,
                held_before: Vec::new(),
            },
        );
        let got = decide(
            &matter(1),
            &rows,
            &[sid(1001)],
            &ledger,
            &SetAsideOwners::default(),
            &OpenSessions::default(),
        );
        assert!(matches!(got, Err(Assign::Records(_))), "{got:?}");
    }

    #[test]
    fn an_empty_pool_is_full() {
        let temp = TempDir::new("assign-empty");
        let ledger = empty_ledger(&temp);
        let got = decide(
            &matter(1),
            &Rows::new(),
            &[],
            &ledger,
            &SetAsideOwners::default(),
            &OpenSessions::default(),
        );
        let full = Assign::PoolFull {
            size: 0,
            held_back: vec![],
        };
        assert_eq!(got, Err(full));
    }
}
