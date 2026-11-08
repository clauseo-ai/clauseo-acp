//! The fake readers: the limits readers' hold and entries, journaled.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::AgentLaunch;

use super::Journal;
use crate::bridge::lock;
use crate::dialogs::BoxFuture;
use crate::setup::readers::{LeaveReaders, ReadHold};

/// The limits readers' hold and entries, journaled.
pub(in crate::setup::backend::tests) struct FakeReaders {
    pub(in crate::setup::backend::tests) journal: Journal,
    pub(in crate::setup::backend::tests) entries: Mutex<Vec<(AgentKind, Option<AgentLaunch>)>>,
    /// The agents last told blocked, not journaled.
    pub(in crate::setup::backend::tests) blocked: Mutex<Option<BTreeSet<AgentId>>>,
}

/// A fake hold: journals its release.
struct FakeHold(Journal);

impl Drop for FakeHold {
    fn drop(&mut self) {
        lock(&self.0).push("readers.release".to_owned());
    }
}

impl LeaveReaders for FakeReaders {
    fn hold(&self, _agent: AgentKind) -> BoxFuture<'static, ReadHold> {
        lock(&self.journal).push("readers.hold".to_owned());
        let hold = ReadHold::new(FakeHold(Arc::clone(&self.journal)));
        Box::pin(std::future::ready(hold))
    }
    fn set_entry(&self, agent: AgentKind, launch: Option<AgentLaunch>) {
        lock(&self.journal).push("readers.set_entry".to_owned());
        lock(&self.entries).push((agent, launch));
    }
    fn set_blocked(&self, blocked: BTreeSet<AgentId>) {
        *lock(&self.blocked) = Some(blocked);
    }
}
