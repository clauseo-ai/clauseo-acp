//! `RecheckBrowserPolicy` on the actor (decision record 0029, the 2026-11-08 amendment): the
//! lawyer's "Try again" on a browser whose policy could not be read. It takes the path a Chrome
//! download's end takes (`browser_install.rs`): the work runs on a thread of its own, its result
//! goes to every app subscriber (`HostEvent::BrowserPolicy`) and into every app catch-up, and no
//! running session changes. The next session started reads the policy again in its launch prep,
//! as every launch does, and gets the browser when nothing blocks it.
//!
//! - The command publishes `Checking`, starts the read (`RdPolicy::recheck`, the launch's rule,
//!   off the actor: the registry and the policy files are never read here) and answers `Done`,
//!   with no receipt and no log line. While one runs it answers `Done` and starts nothing: the
//!   running read's event answers both.
//! - Each read has a number and a deadline on the host clock ([`READ_WAIT`]). A read still
//!   running then (a registry or file read that hangs) ends as `Unreadable`, and the next "Try
//!   again" starts a new read; the late answer, its number no longer current, is dropped.
//! - The read's end: `Unreadable`, `Blocked` or `Clear`. On `Blocked`, every header that showed
//!   `BrowserPolicyUnreadable` shows `BrowserBlockedByPolicy` (`header_tools::policy_blocked`).
//!   `Unreadable` and `Clear` change no header: each says what its session has.
//! - Once the app has heard a state, each launch that read the policy keeps it current
//!   ([`on_launch_read`]): an earlier `Clear` does not outlive a later read error, nor an
//!   `Unreadable` a later clean read.

use std::sync::Arc;
use std::time::Duration;

use futures::future::AbortHandle;

use clauseo_protocol::{BrowserPolicyState, CommandOutcome, HostEvent, ToolsUnavailableReason};

use super::Actor;
use super::commands::CommandCtx;
use super::input::Input;
use crate::error::Error;

/// The read thread's name.
const THREAD_NAME: &str = "clauseo-browser-policy";

/// How long a read may run, on the host clock, before it counts as `Unreadable`.
pub(crate) const READ_WAIT: Duration = Duration::from_secs(10);

/// The re-read's state on the actor.
#[derive(Debug, Default)]
pub(crate) struct PolicyCheck {
    /// The last read's number; each read takes the next.
    last: u64,
    /// The running read: its number and its deadline's timer.
    running: Option<(u64, AbortHandle)>,
    /// The latest state published, in every app catch-up.
    latest: Option<BrowserPolicyState>,
}

/// What reaches the actor for a read, by its number.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PolicyInput {
    /// The read ended: why the browser would stay off, or `None`.
    Read {
        read: u64,
        found: Option<ToolsUnavailableReason>,
    },
    /// The read's deadline passed.
    TimedOut { read: u64 },
}

/// `RecheckBrowserPolicy`: the read starts and the command answers; while one runs, it joins.
pub(super) fn recheck(actor: &mut Actor, ctx: &CommandCtx) {
    let result = begin(actor).map(|()| CommandOutcome::Done);
    actor.finish_command(ctx.id, result);
}

fn begin(actor: &mut Actor) -> Result<(), Error> {
    if actor.browser_policy.running.is_some() {
        return Ok(());
    }
    let read = actor.browser_policy.last + 1;
    let rd_policy = Arc::clone(&actor.wiring.rd_policy);
    let allowed_anyway = actor
        .effective
        .settings()
        .browser
        .allow_when_remote_debugging_disabled;
    let post = actor.post.clone();
    std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || {
            let found = rd_policy.recheck(allowed_anyway);
            let _gone = post.try_send(Input::BrowserPolicy(PolicyInput::Read { read, found }));
        })
        .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))?;
    let deadline = actor.post_after(
        READ_WAIT,
        Input::BrowserPolicy(PolicyInput::TimedOut { read }),
    );
    actor.browser_policy.last = read;
    actor.browser_policy.running = Some((read, deadline));
    publish(actor, BrowserPolicyState::Checking);
    Ok(())
}

/// A read's end or its deadline. Only the running read's counts: a late answer is dropped.
pub(super) fn on_input(actor: &mut Actor, input: PolicyInput) {
    let (read, state) = match input {
        PolicyInput::Read { read, found } => (read, state_of(found)),
        PolicyInput::TimedOut { read } => (read, BrowserPolicyState::Unreadable),
    };
    if actor.browser_policy.running.as_ref().map(|(n, _)| *n) != Some(read) {
        return;
    }
    if let Some((_, deadline)) = actor.browser_policy.running.take() {
        deadline.abort();
    }
    settle(actor, state);
}

/// A launch prep read the firm's browser policy and found `found` (why it blocks, or `None`):
/// once the app has heard a state, and no re-read runs, that state follows the launch's read.
pub(crate) fn on_launch_read(actor: &mut Actor, found: Option<ToolsUnavailableReason>) {
    if actor.browser_policy.running.is_some() || actor.browser_policy.latest.is_none() {
        return;
    }
    let state = state_of(found);
    if actor.browser_policy.latest != Some(state) {
        settle(actor, state);
    }
}

/// The state a read found, published; on `Blocked`, the headers that said the policy was not
/// read say it blocks.
fn settle(actor: &mut Actor, state: BrowserPolicyState) {
    publish(actor, state);
    if state == BrowserPolicyState::Blocked {
        super::header_tools::policy_blocked(actor);
    }
}

/// The state for a read's verdict.
fn state_of(found: Option<ToolsUnavailableReason>) -> BrowserPolicyState {
    if found.is_none() {
        BrowserPolicyState::Clear
    } else if found == Some(ToolsUnavailableReason::BrowserBlockedByPolicy) {
        BrowserPolicyState::Blocked
    } else {
        // `RdPolicy` gives only the two policy reasons; any other keeps the browser off too.
        BrowserPolicyState::Unreadable
    }
}

fn publish(actor: &mut Actor, state: BrowserPolicyState) {
    actor.browser_policy.latest = Some(state);
    super::app::publish(actor, &HostEvent::BrowserPolicy { state });
}

/// The app catch-up's part: the latest state, when there is one.
pub(super) fn current(actor: &Actor) -> Option<HostEvent> {
    let state = actor.browser_policy.latest?;
    Some(HostEvent::BrowserPolicy { state })
}
