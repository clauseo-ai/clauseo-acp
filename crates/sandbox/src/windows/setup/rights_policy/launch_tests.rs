//! The SYSTEM refresh's update of the record's `LogonPolicy` ([`update_launch_record`]),
//! through a fake record and a fake LSA.

use std::path::PathBuf;

use super::*;
use crate::windows::pool::MachineRecord;
use crate::windows::setup::MachineError;

const GROUP: &str = "S-1-5-21-5-5-5-1000";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// The record in memory, for [`update_launch_record`].
struct FakeRecord {
    record: Option<MachineRecord>,
    policy: RightPolicy,
    /// Who holds "Allow log on locally" in effect, as the fake LSA says; `None` fails the read.
    holders: Option<Vec<Sid>>,
    /// `LogonPolicy` and `LogonPolicyAt`, as they stand.
    value: Option<(PolicyState, u64)>,
    reads: usize,
}

impl LaunchRecord for FakeRecord {
    fn read(&mut self) -> Result<Option<MachineRecord>, MachineError> {
        Ok(self.record.clone())
    }

    fn policy(&mut self, pool: Pool<'_>) -> RightPolicy {
        assert_eq!(pool.group, self.record.as_ref().map(|r| &r.group));
        self.reads += 1;
        self.policy
    }

    fn holders(&mut self) -> Result<Vec<Sid>, MachineError> {
        self.holders
            .clone()
            .ok_or_else(|| MachineError::Other("the LSA couldn't be read".to_owned()))
    }

    fn write(&mut self, state: PolicyState, at: u64) -> Result<(), MachineError> {
        // Only into a key that is there, as `write_logon_policy(.., false)`.
        if self.record.is_none() {
            return Err(MachineError::Other("the key is gone".to_owned()));
        }
        self.value = Some((state, at));
        Ok(())
    }
}

fn a_record(removing: bool) -> MachineRecord {
    MachineRecord {
        owner: sid("S-1-5-21-7-8-9-1001"),
        group: sid(GROUP),
        pool: vec![sid("S-1-5-21-5-5-5-1001")],
        data_dir: PathBuf::from(r"C:\Users\Lawyer\AppData\Local\Clauseo"),
        removing,
    }
}

#[test]
fn the_refresh_records_what_policy_says_into_a_record_that_is_there() {
    let without = RightPolicy::Defined {
        includes_pool: false,
    };
    let mut record = FakeRecord {
        record: Some(a_record(false)),
        policy: without,
        value: Some((PolicyState::NotDefined, 10)),
        holders: Some(vec![sid("S-1-5-32-545")]),
        reads: 0,
    };
    let written = update_launch_record(&mut record, 20).unwrap();
    assert_eq!(
        written,
        LaunchUpdate::Written(PolicyState::DefinedWithoutPool)
    );
    assert_eq!(record.value, Some((PolicyState::DefinedWithoutPool, 20)));
}

#[test]
fn the_refresh_makes_no_record_key_and_touches_none_being_removed() {
    for found in [None, Some(a_record(true))] {
        let mut record = FakeRecord {
            record: found,
            policy: RightPolicy::NotDefined,
            value: None,
            holders: Some(vec![sid("S-1-5-32-545")]),
            reads: 0,
        };
        assert_eq!(
            update_launch_record(&mut record, 20),
            Ok(LaunchUpdate::NoRecord)
        );
        assert_eq!(record.value, None);
        assert_eq!(record.reads, 0, "the policy isn't even read");
    }
}

#[test]
fn an_unreadable_policy_keeps_the_last_known_state_and_its_time() {
    let mut record = FakeRecord {
        record: Some(a_record(false)),
        policy: RightPolicy::Unknown,
        value: Some((PolicyState::DefinedWithoutPool, 10)),
        holders: None,
        reads: 0,
    };
    assert_eq!(
        update_launch_record(&mut record, 20),
        Ok(LaunchUpdate::KeptLast)
    );
    assert_eq!(record.value, Some((PolicyState::DefinedWithoutPool, 10)));
}

#[test]
fn the_refresh_records_a_policy_when_the_group_has_lost_the_right_in_effect() {
    let group = sid(GROUP);
    // Set-up granted the group; a policy the read can't see replaced the list since.
    for read in [RightPolicy::NotDefined, RightPolicy::Unknown] {
        let mut record = FakeRecord {
            record: Some(a_record(false)),
            policy: read,
            value: Some((PolicyState::NotDefined, 10)),
            holders: Some(vec![sid("S-1-5-32-544")]),
            reads: 0,
        };
        let written = update_launch_record(&mut record, 20).unwrap();
        assert_eq!(
            written,
            LaunchUpdate::Written(PolicyState::DefinedWithoutPool),
            "{read:?}"
        );
        assert_eq!(record.value, Some((PolicyState::DefinedWithoutPool, 20)));
    }
    // The group, Users, Everyone or Authenticated Users still holding it: the read decides.
    for holder in [GROUP, "S-1-5-32-545", "S-1-1-0", "S-1-5-11"] {
        let mut record = FakeRecord {
            record: Some(a_record(false)),
            policy: RightPolicy::NotDefined,
            value: None,
            holders: Some(vec![sid("S-1-5-32-544"), sid(holder)]),
            reads: 0,
        };
        let written = update_launch_record(&mut record, 20).unwrap();
        assert_eq!(
            written,
            LaunchUpdate::Written(PolicyState::NotDefined),
            "{holder}"
        );
    }
    // An LSA that can't be read leaves the read's answer.
    let mut record = FakeRecord {
        record: Some(a_record(false)),
        policy: RightPolicy::NotDefined,
        value: None,
        holders: None,
        reads: 0,
    };
    let written = update_launch_record(&mut record, 20).unwrap();
    assert_eq!(written, LaunchUpdate::Written(PolicyState::NotDefined));
    assert!(admits_pool(std::slice::from_ref(&group), &group));
    assert!(!admits_pool(&[sid("S-1-5-32-544")], &group));
}
