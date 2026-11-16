//! A Claude launch's guard and policy checks, on the actor (split from `profile_loads.rs` for
//! its size). The guard hook (`agent::loads::guard_entry`) is what a launch rests on when it
//! has no sandbox (Windows, decision record 0030) or must ask before every change (forced
//! ask); elsewhere the sandbox is the baseline and the guard is one more layer. So:
//!
//! - **A policy that weakens the launch** (`policy_keys::finding`): a setting in an admin tier
//!   or the firm's server-managed settings that outranks a value Clauseo pins and would weaken
//!   the launch (`sandbox.enabled: false`, `sandbox.excludedCommands`, …;
//!   `policy_keys/weakens.rs` lists them), or an admin document that cannot be read, refuses
//!   every Claude launch (`agent.firm_policy_weakens`) and stops a live conversation at its next
//!   prompt ([`user_policy_refusal`]). The refusal names the setting and where it was found,
//!   so the firm's IT can act. A value that only tightens passes.
//! - **A policy that changes Clauseo's own launch values** (`weakens::launch_pins`): its `env`
//!   setting one of Clauseo's launch variables (the launch's `PATH` among them, read from the
//!   environment the agent got, `Agent::own_env`) to another value, or a flag setting Clauseo
//!   sets to another value, refuses the same way.
//! - **The user tier** (`policy_keys`): the HKCU policy, which only IT or a program running as
//!   administrator can write (laptop check, 16 Nov), that applies and weakens the launch (the same
//!   settings, and the hooks keys) refuses it the same way, as `agent.user_policy`; Claude Code
//!   itself polls it every 30 minutes.
//! - **The firm's policy turning hooks off** (`disableAllHooks` or `allowManagedHooksOnly` in an
//!   admin tier, audit row 13): a warn line at every Claude launch; a launch resting on the guard
//!   is refused (`agent.firm_hooks_off`). Elsewhere the sandbox is the baseline and the guard
//!   one more layer, so it is not a weakening here.
//! - **No guard to give** (`loads::guard_missing`, audit rows 10 and 11): a launch resting on the
//!   guard is refused (`agent.guard_missing`); elsewhere a warn line.
//!
//! The reads are a few small files and, on Windows, two registry values.

use std::path::{Path, PathBuf};

use clauseo_protocol::{ErrorCode, HostError, ThreadId};

use super::super::super::Actor;
use super::super::offline::probe::settings::policy_keys::weakens::{OwnEnv, launch_pins};
use super::super::offline::probe::settings::policy_keys::{
    self, AdminPolicy, Doc, Finding, Sourced,
};
use crate::agent::loads;
use crate::agent::loads::pinned::security_pins;
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
    policies(&managed(actor), claude_home(actor, thread).as_deref()).0
}

/// What the admin tiers in `managed` (and the OS's, but in unit tests) and the server-managed
/// settings in `home` say, and every one of their documents with its source.
fn policies(managed: &Path, home: Option<&Path>) -> (AdminPolicy, Vec<Sourced>) {
    let mut docs = policy_keys::sourced_admin_docs(managed, !cfg!(test));
    let policy = AdminPolicy::of(&docs.iter().map(|(_, doc)| doc.clone()).collect::<Vec<_>>());
    let Some(home) = home else {
        return (policy, docs);
    };
    let remote = policy_keys::remote_doc(home);
    let policy = policy.with_remote(&remote);
    docs.push((policy_keys::REMOTE, remote));
    (policy, docs)
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

/// Keeps on `thread`'s agent whether its launch found an admin tier (`Agent::admin_tier`), and
/// whether it carries Claude's sandbox (`Agent::sandboxed`, for the policy checks at prompts).
pub(super) fn keep_admin_tier(
    actor: &mut Actor,
    thread: ThreadId,
    admin: AdminPolicy,
    sandboxed: bool,
) {
    let agent = super::super::state(actor, thread).and_then(|state| state.agent.as_deref_mut());
    if let Some(agent) = agent {
        agent.sandboxed = sandboxed;
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

/// `thread`'s agent's own launch values (`Agent::own_env`), when it has an agent.
fn own_env(actor: &Actor, thread: ThreadId) -> Option<OwnEnv> {
    let entry = actor.threads.get(&thread)?;
    Some(entry.turn.agent.as_deref()?.vars.own.clone())
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

/// The refusal when a policy weakens the launch (the module doc): before each prompt of a live
/// Claude conversation, with the launch's pins (`Agent::forced_ask`, `Agent::sandboxed`).
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
    let agent = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.agent.as_deref());
    let launch = agent.map_or((false, false), |agent| (agent.forced_ask, agent.sandboxed));
    let own = own_env(actor, thread);
    move || {
        let (admin, docs) = policies(&managed, home.as_deref());
        policy_refusal(admin, &docs, &user.read(), launch, own.as_ref())
    }
}

/// The refusal, if any, of the policy documents for a launch in forced ask or not, with
/// Claude's sandbox or not (`(forced_ask, sandboxed)`): the user tier's when it applies
/// (`agent.user_policy`), else the admin and server tiers' (`agent.firm_policy_weakens`).
/// Each names the setting and where it is in its detail, for logs, and carries the setting
/// (`HostError::setting`, from a fixed list) for the app's line. Clauseo's own launch values
/// count too (`launch_pins`), the launch's environment's (`own`) once it is known.
fn policy_refusal(
    admin: AdminPolicy,
    docs: &[Sourced],
    user: &Doc,
    (forced_ask, sandboxed): (bool, bool),
    own: Option<&OwnEnv>,
) -> Option<Error> {
    let mut pins = security_pins(sandboxed, forced_ask);
    pins.extend(launch_pins(own));
    if let Some(found) = policy_keys::user_policy_touches(admin, user, &pins, sandboxed) {
        return Some(refusal(ErrorCode::AgentUserPolicy, found));
    }
    let found = policy_keys::finding(docs, &pins, sandboxed, false)?;
    Some(refusal(ErrorCode::AgentFirmPolicyWeakens, found))
}

/// The refusal `code` for `found`, with a warn line naming the setting and its source (a key
/// or a file name, never a value), and the setting the app names (`Finding::policy_setting`).
fn refusal(code: ErrorCode, found: Finding) -> Error {
    let (setting, source) = (found.setting(), found.source());
    tracing::warn!(%code, %setting, source, "a Claude Code policy weakens the launch: refused");
    let detail = match found {
        Finding::Weakens(_) => format!(
            "the Claude Code policy at {source} sets `{setting}`, which weakens Clauseo's \
             protections; only an administrator can change it, so Claude is not run"
        ),
        Finding::Unreadable(_) => format!(
            "the Claude Code policy `{setting}` cannot be read, so whether it weakens \
             Clauseo's protections is unknown and Claude is not run"
        ),
    };
    let mut error = HostError::new(code, detail);
    if let Some(setting) = found.policy_setting() {
        error = error.with_setting(setting);
    }
    Error::Refused(error)
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
    let (admin, docs) = policies(&managed(actor), claude_home(actor, thread).as_deref());
    let user = UserTier::of(actor).read();
    let own = own_env(actor, thread);
    let launch = (forced_ask, sandboxed);
    if let Some(error) = policy_refusal(admin, &docs, &user, launch, own.as_ref()) {
        tracing::warn!(%matter, "a Claude Code policy weakens this launch: refused");
        return Err(error);
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
