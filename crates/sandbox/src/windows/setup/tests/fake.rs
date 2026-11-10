//! The set-up's seams faked in memory: [`FakeMachine`] records every call as a line of text and
//! fails the calls it is told to; its `Machine` impl is in `fake_machine.rs`. [`FakeAcl`]
//! (`fake_acl.rs`) keeps the entries written, and `fake_more.rs` holds the surroundings, the call
//! lines' folder names, and the machine's helpers outside the trait impl (its constructor and
//! call recording).
//! Profiles and `DeviceSync` are real folders in a temp folder, since the shared-write apply
//! checks they exist, and the ledger is a real file there.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use crate::test_support::TempDir;
use crate::windows::acl_plan::Sid;
use crate::windows::fence::RegisteredMatter;
use crate::windows::pool::MachineRecord;
use crate::windows::setup::{LogonRight, MachineError, ServiceStart};

pub(super) use super::fake_acl::FakeAcl;
pub(super) use super::fake_more::{Account, State, Stranded};
use super::fake_more::{Around, ProfileFaults};

/// The owner's SID, and its account's name.
pub(super) const OWNER: &str = "S-1-5-21-7-8-9-1001";
pub(super) const OWNER_NAME: &str = "Rohan";
/// Another Windows user.
pub(super) const OTHER_OWNER: &str = "S-1-5-21-7-8-9-1002";
/// The volume the fake lists for the change journal.
pub(super) const VOLUME: &str = "C:";

/// `SYSTEM` and `BUILTIN\Administrators`.
pub(super) const SYSTEM: &str = "S-1-5-18";
pub(super) const ADMINISTRATORS: &str = "S-1-5-32-544";

/// The SID the fake gives the account or group with relative id `rid`.
pub(super) fn sid(rid: u32) -> Sid {
    Sid::parse(&format!("S-1-5-21-5-5-5-{rid}")).unwrap()
}

/// `sid`'s last part, as the call lines write it.
pub(super) fn rid(sid: &Sid) -> String {
    sid.as_str().rsplit('-').next().unwrap().to_owned()
}

/// SIDs as their relative ids, joined with commas.
pub(super) fn rids(sids: &[Sid]) -> String {
    sids.iter().map(rid).collect::<Vec<_>>().join(",")
}

/// The machine, in memory.
pub(super) struct FakeMachine {
    pub(super) dir: TempDir,
    /// Every call, as a line: the method and what it names.
    pub(super) calls: Vec<String>,
    /// Each fails the first call whose line it is, once, with its error.
    pub(super) fails: Vec<(String, MachineError)>,
    /// The call whose line this is panics.
    pub(super) panic_on: Option<String>,
    pub(super) state: State,
    /// Every record written, in order.
    pub(super) written: Vec<MachineRecord>,
    /// Accounts whose logon gets a temporary profile.
    pub(super) temporary: BTreeSet<String>,
    pub(super) seclogon: ServiceStart,
    pub(super) logon_right: LogonRight,
    pub(super) lock_held: bool,
    pub(super) sessions_open: bool,
    pub(super) journals: BTreeMap<PathBuf, Option<u64>>,
    /// `create_user` of this name answers the owner's SID.
    pub(super) owner_sid_for: Option<String>,
    /// The owner the machine acts as ([`OWNER`] unless a test says otherwise).
    pub(super) owner: Sid,
    /// Whether the calling thread acts as the owner now: inside `as_owner`. A [`FakeAcl`]
    /// made with [`FakeAcl::of`] shares it, so its writes say whose they were.
    pub(super) acting: Rc<Cell<bool>>,
    /// Every call on the owner's files made while not acting as the owner: none, ever.
    pub(super) outside_owner: Vec<String>,
    /// `as_owner` fails, as an impersonation Windows refuses would.
    pub(super) refuse_owner: bool,
    /// The owner's registered matters, as `registered_matters` reads them.
    pub(super) matters: Vec<RegisteredMatter>,
    /// Each folder's owner; one not listed is `SYSTEM`'s.
    pub(super) owners: BTreeMap<PathBuf, Sid>,
    /// Accounts whose first logon leaves their `LocalLow` owned by the account itself.
    pub(super) self_owned_local_low: BTreeSet<String>,
    /// The folders (by their call-line name) whose `give_to_admins` succeeds but leaves the
    /// owner as it was.
    pub(super) stuck_owners: BTreeSet<String>,
    /// Accounts whose profile has a reparse point on the way to `LocalLow` (`AppData`):
    /// `in_profile` and `give_to_admins` refuse it as `ProfileChanged`.
    pub(super) changed_profiles: BTreeSet<String>,
    /// Accounts whose logon leaves their profile with no `LocalLow`.
    pub(super) no_local_low: BTreeSet<String>,
    /// The users the owner's `matters.json` marks as owed a profile reset.
    pub(super) needs_reset: BTreeSet<Sid>,
    /// The users the record's `ResetSids` lists (the stop task's marks).
    pub(super) stop_resets: BTreeSet<Sid>,
    /// The users the reuse rule holds back after the resets, as `held_users` reads them.
    pub(super) held: BTreeSet<Sid>,
    /// The `resets` each `held_users` call was given.
    pub(super) held_for: Vec<Vec<Sid>>,
    pub(super) faults: ProfileFaults,
    pub(super) next_rid: u32,
    pub(super) passwords: u32,
    pub(super) around: Around,
    /// The marks, rights and tasks `stranded.rs` reads and clears (`fake_more.rs`).
    pub(super) stranded: Stranded,
}
