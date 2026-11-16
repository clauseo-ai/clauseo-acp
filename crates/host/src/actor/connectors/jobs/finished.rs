//! What a job posts back: its [`Finished`] result, the [`Change`] beyond what it read, and a
//! command's [`Refusal`] in fixed words.

use clauseo_connectors::{Account, CredentialStatus, Liveness, PaymentRef};
use clauseo_protocol::{CommandId, ErrorCode, RechargeOutcome};

use crate::error::Error;

/// A command's refusal, in fixed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(in crate::actor::connectors) code: ErrorCode,
    pub(in crate::actor::connectors) message: &'static str,
}

impl Refusal {
    pub(in crate::actor::connectors) const fn new(code: ErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }

    pub(in crate::actor::connectors) fn into_error(self) -> Error {
        Error::refused(self.code, self.message)
    }
}

/// What a job found; each field set replaces what the actor holds.
#[derive(Debug)]
pub(crate) struct Finished {
    /// The command to answer, when the job still owes it an answer.
    pub(in crate::actor::connectors) command: Option<CommandId>,
    pub(in crate::actor::connectors) reply: Result<(), Refusal>,
    pub(in crate::actor::connectors) credential: Option<CredentialStatus>,
    pub(in crate::actor::connectors) liveness: Option<Liveness>,
    pub(in crate::actor::connectors) account: Option<Account>,
    /// The account read answered out of credit, with the payment it opened.
    pub(in crate::actor::connectors) out_of_credit: Option<PaymentRef>,
    /// Whether the job held the actor's one-a-minute read slot.
    pub(in crate::actor::connectors) slot: bool,
    /// Whether the job was the read after a relay's paid `recharge` result (`Job::Recharged`).
    pub(in crate::actor::connectors) recharged: bool,
    pub(in crate::actor::connectors) change: Change,
    /// The credential generation the job started under.
    pub(in crate::actor::connectors) generation: u64,
    /// The balance epoch (`limits::Recharged::epoch`) when the job started: the balance a
    /// recharge's answer carried is not applied once it moved, as that answer may be older.
    pub(in crate::actor::connectors) started: u64,
    /// The balance epoch right before the job read the account, if it did: what the read found
    /// is not applied once it moved, being from before a resumed charge.
    pub(in crate::actor::connectors) read_at: Option<u64>,
}

impl Finished {
    pub(in crate::actor::connectors) fn new(command: Option<CommandId>) -> Self {
        Self {
            command,
            reply: Ok(()),
            credential: None,
            liveness: None,
            account: None,
            out_of_credit: None,
            slot: false,
            recharged: false,
            change: Change::Nothing,
            generation: 0,
            started: 0,
            read_at: None,
        }
    }

    pub(super) fn refused(command: Option<CommandId>, refusal: Refusal) -> Self {
        Self {
            reply: Err(refusal),
            ..Self::new(command)
        }
    }
}

/// What a job changes beyond what it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    Nothing,
    /// A sign-in ended; `error` when it failed (not a cancelled one).
    SignInEnded {
        generation: u64,
        error: Option<&'static str>,
    },
    /// An API key failed: the state shows it until the next command.
    Failed(&'static str),
    /// Signed out or removed: the account, out of credit and the pay link go.
    Reset,
    /// A recharge settled: how, when the host can tell (`None` when the watch failed), and the
    /// balance its answer carried, shown when no account read came with it.
    Recharge {
        recharge: String,
        outcome: Option<RechargeOutcome>,
        balance: Option<i64>,
    },
    /// A liveness check ended without an answer (a panic): nothing waits on it any longer.
    NoLiveness,
}
