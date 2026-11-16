//! A Claude launch's guard and policy checks, on the actor (split from `profile_loads.rs` for
//! its size). The guard hook (`agent::loads::guard_entry`) is what a launch rests on when it
//! has no sandbox (Windows, decision record 0030) or must ask before every change (forced
//! ask); elsewhere the sandbox is the baseline and the guard is one more layer. So:
//!
//! - **The user tier** (`policy_keys`): the HKCU policy, which any program the user runs can
//!   write, that applies and sets permissions, sandbox or hooks refuses every Claude launch
//!   (`agent.user_policy`), and stops a live conversation at its next prompt
//!   ([`user_policy_refusal`]); Claude Code itself polls it every 30 minutes.
//! - **The firm's policy turning hooks off** (`disableAllHooks` or `allowManagedHooksOnly` in an
//!   admin tier, audit row 13): a warn line at every Claude launch; a launch resting on the guard
//!   is refused (`agent.firm_hooks_off`).
//! - **No guard to give** (`loads::guard_missing`, audit rows 10 and 11): a launch resting on the
//!   guard is refused (`agent.guard_missing`); elsewhere a warn line.
//!
//! The reads are a few small files and, on Windows, two registry values.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::offline::probe::settings::policy_keys::{self, AdminPolicy, Doc};
use crate::agent::loads;
use crate::error::Error;

/// What refuses a launch, given whether it rests on the guard (`rests`), the admin policy's key
/// that turns hooks off, and why there is no guard to give.
pub(crate) fn guard_verdict(
    rests: bool,
    hooks_off: Option<&str>,
    missing: Option<&str>,
) -> Option<ErrorCode> {
    if !rests {
        return None;
    }
    if hooks_off.is_some() {
        return Some(ErrorCode::AgentFirmHooksOff);
    }
    missing.map(|_| ErrorCode::AgentGuardMissing)
}

/// The folder of Claude Code's `managed-settings.json`.
fn managed(actor: &Actor) -> PathBuf {
    actor
        .managed_mcp
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// What this machine's admin tiers say for `thread`'s agent, with the server-managed settings
/// in its Claude home when it has one; the OS tiers are not read in unit tests. For a launch's
/// checks, and for `asks` (stage D: no read of a network share is approved unasked while an
/// admin tier is present, `content || remote`).
pub(crate) fn admin(actor: &Actor, thread: ThreadId) -> AdminPolicy {
    let policy = AdminPolicy::of(&policy_keys::admin_docs(&managed(actor), !cfg!(test)));
    match claude_home(actor, thread) {
        Some(home) => policy.with_remote(&policy_keys::remote_doc(&home)),
        None => policy,
    }
}

/// Whether an admin policy tier (or the firm's server-managed settings) has content; "not
/// computed yet" reads as `Present`. Stage D's `agent/network_matter/admin_policy.rs` has the
/// identical type: whichever PR lands second deletes its copy and imports the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdminTier {
    /// An admin tier has content, or it is not known yet.
    Present,
    /// No admin tier has content.
    Absent,
}

/// Keeps on `thread`'s agent whether its launch found an admin tier (`Agent::admin_tier`).
pub(super) fn keep_admin_tier(actor: &mut Actor, thread: ThreadId, admin: AdminPolicy) {
    let agent = super::super::state(actor, thread).and_then(|state| state.agent.as_deref_mut());
    if let Some(agent) = agent {
        agent.admin_tier = if admin.content || admin.remote {
            AdminTier::Present
        } else {
            AdminTier::Absent
        };
    }
}

/// `thread`'s agent's Claude home, when it has one.
fn claude_home(actor: &Actor, thread: ThreadId) -> Option<PathBuf> {
    let entry = actor.threads.get(&thread)?;
    entry.turn.agent.as_deref()?.claude_home.clone()
}

/// Reads the user tier's document: HKCU's value, or a unit test's.
#[derive(Debug, Clone)]
struct UserTier {
    #[cfg(test)]
    played: Option<std::sync::Arc<std::sync::Mutex<super::super::wide::UserTierRead>>>,
}

impl UserTier {
    fn of(actor: &Actor) -> Self {
        let _ = actor;
        Self {
            #[cfg(test)]
            played: actor.turn_path.user_policy_for_tests.clone(),
        }
    }

    fn read(&self) -> Doc {
        #[cfg(not(test))]
        let Self {} = self;
        #[cfg(test)]
        {
            self.played.as_ref().map_or(Doc::Absent, |read| {
                Doc::from_read(
                    &read
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                )
            })
        }
        #[cfg(not(test))]
        Doc::from_read(&policy_keys::user_tier())
    }
}

/// The refusal when the user tier applies and touches permissions, sandbox or hooks; for each
/// Claude launch, and before each prompt of a live Claude conversation.
pub(crate) fn user_policy_refusal(actor: &Actor, thread: ThreadId) -> Option<Error> {
    user_policy_reader(actor, thread)()
}

/// [`user_policy_refusal`] as a check that needs no actor, for a poll inside a running turn:
/// it reads the admin tiers, the server-managed settings and the user tier again at each call.
pub(crate) fn user_policy_reader(
    actor: &Actor,
    thread: ThreadId,
) -> impl Fn() -> Option<Error> + 'static {
    let (managed, home, user) = (
        managed(actor),
        claude_home(actor, thread),
        UserTier::of(actor),
    );
    move || {
        let policy = AdminPolicy::of(&policy_keys::admin_docs(&managed, !cfg!(test)));
        let policy = match &home {
            Some(home) => policy.with_remote(&policy_keys::remote_doc(home)),
            None => policy,
        };
        user_refusal(policy, &user.read())
    }
}

/// The `agent.user_policy` refusal when `user` applies over `admin` and touches a guarded key.
fn user_refusal(admin: AdminPolicy, user: &Doc) -> Option<Error> {
    policy_keys::user_policy_touches(admin, user).then(|| {
        Error::refused(
            ErrorCode::AgentUserPolicy,
            "a Claude Code policy that any program on this computer can write (the user's \
             registry) sets permissions, the sandbox or hooks, so Claude is not run",
        )
    })
}

/// A Claude launch's checks of its guard and of the policy tier (the module doc); `sandboxed`
/// says whether the launch carries Claude's sandbox. Gives what the admin tiers say, which the
/// launch keeps on the agent (`Agent::admin_tier`).
pub(super) fn check_guard(
    actor: &Actor,
    thread: ThreadId,
    (forced_ask, sandboxed): (bool, bool),
    guard: Option<loads::GuardHook<'_>>,
) -> Result<AdminPolicy, Error> {
    let matter = actor.check_thread(thread)?.row.matter;
    let admin = admin(actor, thread);
    if let Some(error) = user_refusal(admin, &UserTier::of(actor).read()) {
        tracing::warn!(
            %matter,
            "a user-writable Claude Code policy sets permissions, sandbox or hooks: the launch is \
             refused"
        );
        return Err(error);
    }
    if admin.unreadable {
        tracing::warn!(%matter, "a Claude Code admin policy cannot be read; its hooks keys are unknown");
    }
    let rests = forced_ask || !sandboxed;
    let missing = loads::guard_missing(guard);
    if let Some(key) = admin.hooks_off {
        tracing::warn!(
            %matter,
            key,
            forced_ask,
            sandboxed,
            "the firm's Claude Code policy turns hooks off, so Clauseo's guard hook won't run"
        );
    }
    match guard_verdict(rests, admin.hooks_off, missing) {
        Some(ErrorCode::AgentFirmHooksOff) => Err(Error::refused(
            ErrorCode::AgentFirmHooksOff,
            format!(
                "the firm's Claude Code policy sets {}, which turns off the guard this launch \
                 rests on",
                admin.hooks_off.unwrap_or_default()
            ),
        )),
        Some(code) => {
            tracing::warn!(
                %matter,
                reason = missing.unwrap_or_default(),
                forced_ask,
                "a Claude launch that rests on the guard hook has none to give: the launch is \
                 refused"
            );
            let detail = if forced_ask {
                "this matter's folder is unprotected, and the guard that makes Claude ask before \
                 each change could not be set up"
            } else {
                "Claude has no sandbox on this computer, and the guard that checks each change \
                 could not be set up"
            };
            Err(Error::refused(code, detail))
        }
        None => {
            if let Some(reason) = missing {
                tracing::warn!(%thread, reason, "a Claude launch goes without the guard hook");
            }
            Ok(admin)
        }
    }
}

#[cfg(test)]
mod tests;
