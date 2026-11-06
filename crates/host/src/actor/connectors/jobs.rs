//! The connector commands' work off the actor (R24, PR5). Each job runs on a thread of its own,
//! calls the connectors crate (keychain, files and HTTPS, all blocking) and posts what it found
//! as a [`ConnectorInput`]; the actor never waits on one. A job that ends without posting its
//! result (a panic) posts a failure from `Drop` ([`Unfinished`]), so no command waits forever
//! and no sign-in stays shown.
//!
//! Logs name the connector and the kind of failure ([`kind`]), never a URL, a pay link, an
//! account id or the connector's own text; refusals carry fixed words ([`Refusal`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_connectors::{
    Account, CancelFlag, ConnectorHandle, Connectors, CredentialStatus, Error as Failed, Liveness,
    PaymentRef, RechargeStatus, SignInError,
};
use clauseo_protocol::{CommandId, ErrorCode, RedactedString};

mod failures;

use super::ConnectorInput;
use crate::actor::input::Input;
use crate::connectors::ConnectorOpener;
use crate::error::Error;
pub(super) use failures::kind;
use failures::{refusal, sign_in_kind};

/// The job threads' name.
const THREAD_NAME: &str = "clauseo-connector";

/// The state's line for a sign-in that failed (not a cancelled one).
pub(super) const SIGN_IN_FAILED: &str = "the sign-in did not finish";
/// The state's line when the browser did not open the sign-in page.
const BROWSER_FAILED: &str = "the browser did not open the sign-in page";
/// The state's line for an API key that was not added.
const KEY_FAILED: &str = "the API key was not added";
/// The state's line for a job that ended without a result.
const JOB_FAILED: &str = "the connector's work ended without a result";
/// The refusal of a key whose check outlasted a sign-out, a remove or the switch going off.
const KEY_ENDED: &str = "the connector was signed out or turned off while the key was checked";

/// A command's refusal, in fixed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub(super) code: ErrorCode,
    pub(super) message: &'static str,
}

impl Refusal {
    pub(super) const fn new(code: ErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }

    pub(super) fn into_error(self) -> Error {
        Error::refused(self.code, self.message)
    }
}

/// What a job found; each field set replaces what the actor holds.
#[derive(Debug)]
pub(crate) struct Finished {
    /// The command to answer, when the job still owes it an answer.
    pub(super) command: Option<CommandId>,
    pub(super) reply: Result<(), Refusal>,
    pub(super) credential: Option<CredentialStatus>,
    pub(super) liveness: Option<Liveness>,
    pub(super) account: Option<Account>,
    /// The account read answered out of credit, with the payment it opened.
    pub(super) out_of_credit: Option<PaymentRef>,
    /// Whether the job held the actor's one-a-minute read slot.
    pub(super) slot: bool,
    pub(super) change: Change,
    /// The credential generation the job started under.
    pub(super) generation: u64,
}

impl Finished {
    pub(super) fn new(command: Option<CommandId>) -> Self {
        Self {
            command,
            reply: Ok(()),
            credential: None,
            liveness: None,
            account: None,
            out_of_credit: None,
            slot: false,
            change: Change::Nothing,
            generation: 0,
        }
    }

    fn refused(command: Option<CommandId>, refusal: Refusal) -> Self {
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
    /// A recharge settled: paid, or expired or failed.
    Recharge {
        recharge: String,
        paid: bool,
    },
}

/// One job.
pub(super) enum Job {
    /// The credential, read once at start (no request).
    Status,
    /// Liveness, the credential, and the account when `slot`.
    Check {
        slot: bool,
    },
    SignIn {
        generation: u64,
        opener: Arc<dyn ConnectorOpener>,
    },
    /// The pasted key, zeroed when the job drops it.
    ApiKey(RedactedString),
    /// `watch` is the actor's number for the watch that follows.
    Credits {
        pack: String,
        watch: u64,
        cancel: CancelFlag,
    },
    /// `number` is the actor's number for this watch.
    Watch {
        recharge: String,
        number: u64,
        cancel: CancelFlag,
    },
    SignOut,
    Remove,
    /// The account, holding the read slot.
    Read,
}

impl Job {
    /// What the actor shows if the job ends without a result.
    fn on_panic(&self) -> Change {
        match self {
            Job::SignIn { generation, .. } => Change::SignInEnded {
                generation: *generation,
                error: Some(JOB_FAILED),
            },
            Job::Status
            | Job::Check { .. }
            | Job::ApiKey(_)
            | Job::Credits { .. }
            | Job::Watch { .. }
            | Job::SignOut
            | Job::Remove
            | Job::Read => Change::Nothing,
        }
    }

    fn holds_slot(&self) -> bool {
        matches!(self, Job::Check { slot: true } | Job::Read)
    }

    /// The number of the recharge watch the job is, if it is one.
    fn watch(&self) -> Option<u64> {
        match self {
            Job::Credits { watch, .. } => Some(*watch),
            Job::Watch { number, .. } => Some(*number),
            Job::Status
            | Job::Check { .. }
            | Job::SignIn { .. }
            | Job::ApiKey(_)
            | Job::SignOut
            | Job::Remove
            | Job::Read => None,
        }
    }
}

/// The actor's address, for a job's results.
#[derive(Clone)]
pub(super) struct Post(pub(super) async_channel::Sender<Input>);

impl Post {
    pub(super) fn send(&self, input: ConnectorInput) {
        let _gone = self.0.try_send(Input::Connector(Box::new(input)));
    }
}

/// What a job needs of the app.
#[derive(Clone)]
pub(super) struct Reach {
    pub(super) connectors: Arc<Connectors>,
    pub(super) handle: ConnectorHandle,
    pub(super) post: Post,
    /// The actor's credential generation, and the one the job started under.
    pub(super) credentials: Arc<AtomicU64>,
    pub(super) generation: u64,
}

impl Reach {
    /// Whether no sign-out, remove or switch-off came in since the job started: asked under
    /// the connectors crate's credential lock before a credential is stored.
    fn still(&self) -> bool {
        self.credentials.load(Ordering::SeqCst) == self.generation
    }
}

/// Starts `job` for `command` on a thread of its own.
pub(super) fn spawn(reach: Reach, command: Option<CommandId>, job: Job) -> Result<(), Error> {
    std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            // Dropped last, on every way out: the watch never outlives its thread.
            let _watch = job.watch().map(|watch| WatchEnded {
                post: reach.post.clone(),
                watch,
            });
            let unfinished = Unfinished {
                post: Some(reach.post.clone()),
                command,
                slot: job.holds_slot(),
                change: job.on_panic(),
                generation: reach.generation,
            };
            if let Some(mut finished) = run(&reach, command, job) {
                finished.generation = reach.generation;
                reach
                    .post
                    .send(ConnectorInput::Finished(Box::new(finished)));
            }
            unfinished.disarm();
        })
        .map(drop)
        .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))
}

/// Tells the actor a recharge watch's thread ended, when dropped.
pub(super) struct WatchEnded {
    pub(super) post: Post,
    pub(super) watch: u64,
}

impl Drop for WatchEnded {
    fn drop(&mut self) {
        self.post
            .send(ConnectorInput::WatchEnded { watch: self.watch });
    }
}

/// Posts a failure if the job ends without posting its result.
struct Unfinished {
    post: Option<Post>,
    command: Option<CommandId>,
    slot: bool,
    change: Change,
    generation: u64,
}

impl Unfinished {
    fn disarm(mut self) {
        self.post = None;
    }
}

impl Drop for Unfinished {
    fn drop(&mut self) {
        if let Some(post) = self.post.take() {
            tracing::warn!(
                connector = "case-law",
                "a connector job ended without a result"
            );
            let refusal = Refusal::new(ErrorCode::Internal, JOB_FAILED);
            post.send(ConnectorInput::Finished(Box::new(Finished {
                slot: self.slot,
                change: self.change.clone(),
                generation: self.generation,
                ..Finished::refused(self.command, refusal)
            })));
        }
    }
}

/// Runs `job`; `None` when it posted its own last result.
fn run(reach: &Reach, command: Option<CommandId>, job: Job) -> Option<Finished> {
    let handle = &reach.handle;
    let mut finished = match job {
        Job::Status => Finished::new(command),
        Job::Check { slot } => {
            let liveness = reach.connectors.liveness();
            let mut finished = if slot {
                read(handle, command)
            } else {
                Finished::new(command)
            };
            finished.liveness = Some(liveness);
            finished
        }
        Job::SignIn { generation, opener } => sign_in(reach, command, generation, opener.as_ref()),
        Job::ApiKey(key) => api_key(reach, command, key.expose()),
        Job::Credits {
            pack,
            watch,
            cancel,
        } => credits(reach, command, (&pack, watch), &cancel)?,
        Job::Watch {
            recharge, cancel, ..
        } => watch(handle, recharge, &cancel)?,
        Job::SignOut | Job::Remove => sign_out(handle, command, matches!(job, Job::Remove)),
        Job::Read => read(handle, command),
    };
    // Read last, so it holds what every call above did to the credential.
    finished.credential = Some(handle.status());
    Some(finished)
}

/// The account when signed in, holding the read slot.
fn read(handle: &ConnectorHandle, command: Option<CommandId>) -> Finished {
    let mut finished = Finished::new(command);
    finished.slot = true;
    read_into(handle, &mut finished);
    finished
}

/// Reads the account into `finished` when signed in: the account, or the payment an
/// out-of-credit answer opened. A failure is logged and reads nothing.
fn read_into(handle: &ConnectorHandle, finished: &mut Finished) {
    if !matches!(handle.status(), CredentialStatus::SignedIn(_)) {
        return;
    }
    match handle.account() {
        Ok(account) => finished.account = Some(account),
        Err(Failed::InsufficientCredits(payment)) => finished.out_of_credit = Some(payment),
        Err(error) => {
            tracing::warn!(
                connector = "case-law",
                error = kind(&error),
                "the account read failed"
            );
        }
    }
}

/// `Add`: the sign-in page in the browser, then (the command answered) the wait for its end.
fn sign_in(
    reach: &Reach,
    command: Option<CommandId>,
    generation: u64,
    opener: &dyn ConnectorOpener,
) -> Finished {
    let handle = &reach.handle;
    let ended = |error| Change::SignInEnded { generation, error };
    let pending = match handle.begin_sign_in(None) {
        Ok(pending) => pending,
        Err(error) => {
            tracing::warn!(
                connector = "case-law",
                error = sign_in_kind(&error),
                "the sign-in did not start"
            );
            let refusal = Refusal::new(ErrorCode::SigninFailed, SIGN_IN_FAILED);
            return Finished {
                change: ended(Some(SIGN_IN_FAILED)),
                ..Finished::refused(command, refusal)
            };
        }
    };
    if let Err(error) = opener.open(pending.authorize_url().as_str()) {
        tracing::warn!(connector = "case-law", %error, "the sign-in page did not open");
        pending.cancel_flag().cancel();
        let refusal = Refusal::new(ErrorCode::SigninFailed, BROWSER_FAILED);
        return Finished {
            change: ended(Some(BROWSER_FAILED)),
            ..Finished::refused(command, refusal)
        };
    }
    reach.post.send(ConnectorInput::SignInStarted {
        command,
        generation,
        cancel: pending.cancel_flag(),
    });
    let result = pending.finish_while(&|| reach.still());
    let mut finished = Finished::new(None);
    finished.change = ended(match &result {
        Ok(()) | Err(SignInError::Cancelled) => None,
        Err(error) => {
            tracing::warn!(
                connector = "case-law",
                error = sign_in_kind(error),
                "the sign-in failed"
            );
            Some(SIGN_IN_FAILED)
        }
    });
    if result.is_ok() {
        read_into(handle, &mut finished);
    }
    finished
}

/// `AddApiKey`: the key checked with one account read and stored (unless the credential ended
/// meanwhile), then zeroed.
fn api_key(reach: &Reach, command: Option<CommandId>, key: &str) -> Finished {
    let handle = &reach.handle;
    match handle.add_api_key_while(key, &|| reach.still()) {
        Ok(()) => {
            let mut finished = Finished::new(command);
            // From memory: adding the key read the account.
            read_into(handle, &mut finished);
            finished
        }
        Err(Failed::Cancelled) => {
            tracing::info!(
                connector = "case-law",
                "the API key ended before it was stored"
            );
            let refusal = Refusal::new(ErrorCode::CommandConflict, KEY_ENDED);
            Finished::refused(command, refusal)
        }
        Err(error) => {
            tracing::warn!(
                connector = "case-law",
                error = kind(&error),
                "the API key was not added"
            );
            Finished {
                change: Change::Failed(KEY_FAILED),
                ..Finished::refused(command, refusal(&error))
            }
        }
    }
}

/// `AddCredits`: the pack checked against the live price list, the recharge opened and its pay
/// link posted (which answers the command), then the watch for its payment.
fn credits(
    reach: &Reach,
    command: Option<CommandId>,
    (pack, number): (&str, u64),
    cancel: &CancelFlag,
) -> Option<Finished> {
    let liveness = reach.connectors.liveness();
    let on_sale = liveness.live
        && liveness
            .pricing
            .as_ref()
            .is_some_and(|pricing| pricing.packs.iter().any(|on| on.pack == pack));
    if !on_sale {
        let refusal = Refusal::new(ErrorCode::CommandInvalid, "no such pack is on sale");
        return Some(Finished {
            liveness: Some(liveness),
            ..Finished::refused(command, refusal)
        });
    }
    let recharge = match reach.handle.create_recharge(pack) {
        Ok(recharge) => recharge,
        Err(error) => {
            tracing::warn!(
                connector = "case-law",
                error = kind(&error),
                "the recharge was not opened"
            );
            return Some(Finished::refused(command, refusal(&error)));
        }
    };
    let Some(pay_url) = recharge.url else {
        tracing::warn!(
            connector = "case-law",
            "the recharge came with no pay link of ours"
        );
        let refusal = Refusal::new(ErrorCode::Internal, "the connector sent no pay link");
        return Some(Finished::refused(command, refusal));
    };
    reach.post.send(ConnectorInput::PayLink {
        command,
        watch: number,
        recharge: recharge.recharge_id.clone(),
        pay_url,
    });
    watch(&reach.handle, recharge.recharge_id, cancel)
}

/// Waits for `recharge` to settle, then reads the account if it was paid. `None` when
/// cancelled: a newer recharge or a sign-out took its place.
fn watch(handle: &ConnectorHandle, recharge: String, cancel: &CancelFlag) -> Option<Finished> {
    let mut finished = Finished::new(None);
    let paid = match handle.wait_recharge(&recharge, cancel) {
        Ok(settled) => settled.status == RechargeStatus::Paid,
        Err(Failed::Cancelled) => return None,
        Err(error) => {
            tracing::info!(
                connector = "case-law",
                error = kind(&error),
                "the recharge watch ended"
            );
            false
        }
    };
    if paid {
        read_into(handle, &mut finished);
    }
    finished.change = Change::Recharge { recharge, paid };
    Some(finished)
}

/// `SignOut` and `Remove`: the keychain item goes whether or not the revoke worked.
fn sign_out(handle: &ConnectorHandle, command: Option<CommandId>, remove: bool) -> Finished {
    let result = if remove {
        handle.remove()
    } else {
        handle.sign_out().map(|report| {
            if !report.revoked {
                tracing::info!(
                    connector = "case-law",
                    "the connector did not confirm the revoke"
                );
            }
        })
    };
    let reply = result.map_err(|error| {
        tracing::warn!(
            connector = "case-law",
            error = kind(&error),
            remove,
            "signing out failed in part"
        );
        Refusal::new(
            ErrorCode::Internal,
            "the sign-in could not be fully removed",
        )
    });
    Finished {
        reply,
        change: Change::Reset,
        ..Finished::new(command)
    }
}

#[cfg(test)]
mod tests;
