//! [`WindowsRights`]: the real [`LogonRights`], putting together the LSA calls (`preflight.rs`),
//! the policy read (`rights_source.rs`) and the record's `LogonRightAdded` (`record_logon.rs`).

use super::logon_grant::LogonRights;
use super::machine::MachineError;
use super::preflight::{add_interactive, holders, remove_interactive};
use super::record_logon;
use super::rights_policy::{PolicyState, Pool, RightPolicy};
use super::rights_source;
use crate::windows::sids::Sid;

/// The real [`LogonRights`]: the LSA, the policy read afresh, and `LogonRightAdded` in the
/// record key `record_key`.
#[derive(Debug, Clone)]
pub(super) struct WindowsRights {
    /// The record's key under `HKLM`.
    pub(super) record_key: String,
}

impl LogonRights for WindowsRights {
    fn policy(&mut self, right: &str, pool: Pool<'_>) -> RightPolicy {
        rights_source::read_now(right, pool)
    }

    fn holders(&mut self, right: &str) -> Result<Vec<Sid>, MachineError> {
        holders(right)
    }

    fn add_interactive(&mut self, sid: &Sid) -> Result<(), MachineError> {
        add_interactive(sid)
    }

    fn remove_interactive(&mut self, sid: &Sid) -> Result<(), MachineError> {
        remove_interactive(sid)
    }

    fn recorded(&mut self) -> Result<Option<Sid>, MachineError> {
        record_logon::read_logon_grant(&self.record_key)
    }

    fn record(&mut self, sid: Option<&Sid>) -> Result<(), MachineError> {
        record_logon::write_logon_grant(&self.record_key, sid)
    }

    fn record_policy(&mut self, state: PolicyState, at: u64) -> Result<(), MachineError> {
        record_logon::write_logon_policy(&self.record_key, state, at, true)
    }
}
