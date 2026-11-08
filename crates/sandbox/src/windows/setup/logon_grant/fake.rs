//! [`FakeRights`]: the LSA and `LogonRightAdded` in memory, for the grant's tests and the
//! set-up and removal fakes. Its calls are kept apart from the machine's call lines, so the
//! engine's other tests read as they did.

use super::LogonRights;
use crate::windows::setup::MachineError;
use crate::windows::setup::rights_policy::{Pool, RightPolicy};
use crate::windows::sids::Sid;

/// The rights in memory.
#[derive(Debug, Clone)]
pub(crate) struct FakeRights {
    /// What policy says of every right.
    pub(crate) policy: RightPolicy,
    /// The SIDs local policy gives "Allow log on locally" to.
    pub(crate) holders: Vec<Sid>,
    /// `LogonRightAdded`.
    pub(crate) recorded: Option<Sid>,
    /// Each call, as a line.
    pub(crate) calls: Vec<String>,
    /// The call whose line this is fails, once.
    pub(crate) fail: Option<String>,
}

impl Default for FakeRights {
    /// No policy, and the right held by Users, as on a default Windows.
    fn default() -> FakeRights {
        FakeRights {
            policy: RightPolicy::NotDefined,
            holders: vec![Sid::parse("S-1-5-32-545").unwrap()],
            recorded: None,
            calls: Vec::new(),
            fail: None,
        }
    }
}

impl FakeRights {
    fn call(&mut self, line: &str) -> Result<(), MachineError> {
        let failing = self.fail.as_deref() == Some(line);
        self.calls.push(line.to_owned());
        if failing {
            self.fail = None;
            return Err(MachineError::Other(format!("injected: {line}")));
        }
        Ok(())
    }
}

impl LogonRights for FakeRights {
    fn policy(&mut self, right: &str, _pool: Pool<'_>) -> RightPolicy {
        self.calls.push(format!("policy {right}"));
        self.policy
    }

    fn holders(&mut self, right: &str) -> Result<Vec<Sid>, MachineError> {
        self.call(&format!("holders {right}"))?;
        Ok(self.holders.clone())
    }

    fn add_interactive(&mut self, sid: &Sid) -> Result<(), MachineError> {
        self.call(&format!("add {sid}"))?;
        if !self.holders.contains(sid) {
            self.holders.push(sid.clone());
        }
        Ok(())
    }

    fn remove_interactive(&mut self, sid: &Sid) -> Result<(), MachineError> {
        self.call(&format!("remove {sid}"))?;
        self.holders.retain(|held| held != sid);
        Ok(())
    }

    fn recorded(&mut self) -> Result<Option<Sid>, MachineError> {
        self.call("recorded")?;
        Ok(self.recorded.clone())
    }

    fn record(&mut self, sid: Option<&Sid>) -> Result<(), MachineError> {
        let line = match sid {
            Some(sid) => format!("record {sid}"),
            None => "record none".to_owned(),
        };
        self.call(&line)?;
        self.recorded = sid.cloned();
        Ok(())
    }
}
