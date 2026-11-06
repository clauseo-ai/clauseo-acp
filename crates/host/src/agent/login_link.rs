//! The sign-in hook (decision record 0048, ruling 2; R20 brief, "The split check"). Desktop
//! fills [`LoginCheck`] from `agents` (`codex_link_holds`, `codex_login_shared`) and the
//! sign-in-again test ([`SignInAgain`], `is_sign_in_again`), so no file-id code is copied
//! into the host. An unset hook means no check.
//!
//! The host asks the hook at every Codex launch, off the actor on a `clauseo-codex-fs`
//! thread, waiting at most [`CHECK_WAIT`] on the host's clock ([`check`]). Its answer decides
//! two things:
//! - the split check: while desktop has said the login is reused
//!   (`HostHandle::set_reused_login`), any answer but [`LoginLink::Shared`] refuses the launch
//!   with `agent.auth_required` and sends a [`LoginSplit`] on `HostHandle::login_splits`;
//! - the guard: while [`LoginCheck::shared`] says the home's `auth.json` is the lawyer's file
//!   (a symbolic link included, which the split check sees as a split), or cannot say, a typed
//!   `/login` to that Codex agent is refused with `signin.login_shared` (`/logout` is refused
//!   always, P4a), and the API-key `authenticate` retry is not armed (P4d).
//!
//! A check that does not answer in time is [`Seen::Unknown`], never a sign-in answer: it is no
//! split and changes no flag. The split check refuses the launch (`agent.launch_failed`, which
//! may be retried), and the guard refuses as for a shared file ([`treated_as_shared`]), as S18's
//! checks do for a stat that fails. The logs name the agent and the outcome, never a path.
//!
//! `HostHandle::remove_login_links` removes each matter's `auth.json` link ([`remove_links`],
//! every `<data>/agent-state/*/codex-acp/auth.json`, R17 PR-C) on a thread of its own, never
//! the actor's. A matter's link itself is made at its launch (`agent/state_folder.rs`): on
//! Clauseo's own login from the login home whenever the file ids differ; while reused only when
//! this hook finds the home's link holding, and a matter's link that is not the home's file is
//! a split, refused with no re-link.
//!
//! TODO(R17-C2): stage C's blocking point 2, the shared-login swap check (the shared login is
//! writable from every matter's session), waits on the orchestrator's ruling.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{AgentId, ErrorCode, MatterId};

use super::codex::prepare::{Job, off_actor};
use super::launch_refusals::LaunchRefusals;
use super::login_splits::{self, LoginSplits, SplitSender};
use crate::clock::Clock;
use crate::error::Error;

/// How long a launch waits for the hook's answer.
pub(crate) const CHECK_WAIT: Duration = Duration::from_secs(5);

/// How long `remove_login_links` waits for its removals.
const REMOVE_WAIT: Duration = Duration::from_secs(5);

/// Whether a Codex agent's home holds the lawyer's own login file. Desktop implements it with
/// S18's file-id checks; the host never opens either file and never compares ids itself.
pub trait LoginCheck {
    /// The login in `agent_home` (Clauseo's `Locations::agent_home` of `agent`, its
    /// `CODEX_HOME`) against the lawyer's `~/.codex/auth.json`, by stat only, never an open.
    /// Called off the actor; a call that takes too long is given up on.
    fn check(&self, agent: &AgentId, agent_home: &Path) -> LoginLink;

    /// Whether `agent_home`'s `auth.json` is the lawyer's `~/.codex/auth.json`, however it
    /// got there: a hard link, or a symbolic link (which [`Self::check`] answers
    /// [`LoginLink::Split`]). By stat only, never an open; called off the actor, beside
    /// [`Self::check`], and given up on with it. Only the `/login` and `authenticate` guard
    /// reads it, and the host counts [`Shared::Unknown`] as shared.
    fn shared(&self, agent: &AgentId, agent_home: &Path) -> Shared;
}

/// The hook's answer to [`LoginCheck::shared`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shared {
    /// The home's `auth.json` is the lawyer's file.
    Yes,
    /// It is not, or there is none.
    No,
    /// A stat failed, so it is not known.
    Unknown,
}

/// Whether the guard treats `shared` as shared: `Yes`, and `Unknown` (fail closed). The one
/// place the host decides it.
pub(crate) fn treated_as_shared(shared: Shared) -> bool {
    shared != Shared::No
}

/// The hook's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginLink {
    /// The home's login is Clauseo's own, or there is none: no file is shared.
    Own,
    /// The home's `auth.json` and the lawyer's are one file (the reused login's link holds),
    /// or a stat failed and that cannot be ruled out.
    Shared,
    /// Both files are there and are not one file: the link split.
    Split,
    /// The lawyer's `auth.json` is gone.
    Gone,
}

/// The sign-in-again test: whether an agent error's text is one the agent's login cannot
/// recover from, such as Codex's `refresh_token_reused` (S18's `is_sign_in_again`). A turn
/// failing so is treated as `agent.auth_required`.
pub type SignInAgain = fn(&str) -> bool;

/// A reused Codex login that split, sent on `HostHandle::login_splits`: desktop runs S18's
/// `stop_using_login` with `CodexLinkSplit` (`ReusedSignedOut` when the lawyer's file is gone)
/// and the leave order. The host has already set its own reused flag for the agent to false.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSplit {
    /// The agent whose launch was refused.
    pub agent: AgentId,
    /// The lawyer's `auth.json` is gone, not replaced.
    pub lawyers_file_gone: bool,
}

/// How `HostHandle::stop_agents` ended its wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsStopped {
    /// Every process of the agent id exited.
    Exited,
    /// The wait ran out first, or a process outlived its kill: it is reaped later.
    TimedOut,
}

/// Why `HostHandle::stop_agents_for` stops an agent: it sets the reason its held queues show
/// (`queue_held_reason`, `turn/queue/held.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopFor {
    /// The login is switching (what `HostHandle::stop_agents` does): the held queues show
    /// `switching` until `restart_agents`.
    LoginSwitch,
    /// The account on the shared login changed: the held queues show `sign_in`, so the app
    /// asks the user to sign in again. The questions stay held and run after the new sign-in.
    SignInAgain,
}

/// What a launch's check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seen {
    /// The hook answered.
    Answered(LoginLink),
    /// The hook did not answer in time, or its thread failed: no answer.
    Unknown,
}

/// What a launch's check found: [`LoginCheck::check`]'s answer for the split check, and
/// [`LoginCheck::shared`]'s for the guard (`Unknown` when the hook did not answer in time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Checked {
    pub(crate) link: Seen,
    pub(crate) shared: Shared,
}

/// The host's sign-in state: the hook and the sign-in-again test a caller set, the channel of
/// splits, and per agent whether desktop said its login is reused and what its last launch's
/// check found. Read and changed on the actor only.
#[derive(Clone)]
pub(crate) struct SignIn {
    /// The hook; `None` is no check.
    pub(crate) check: Option<Arc<dyn LoginCheck + Send + Sync>>,
    /// The sign-in-again test; `None` treats only `agent.auth_required` as a sign-in.
    pub(crate) again: Option<SignInAgain>,
    splits: SplitSender,
    receiver: LoginSplits,
    /// The agents whose login desktop said is reused (`HostHandle::set_reused_login`).
    pub(crate) reused: BTreeSet<AgentId>,
    /// What each Codex agent's last launch's check found.
    pub(crate) seen: BTreeMap<AgentId, Checked>,
    /// The agents `HostHandle::stop_agents_for` holds until `restart_agents`, each with why: none
    /// of them launches, and their prompts queue held. The latest stop's reason wins. In memory
    /// only: a host restart clears it.
    pub(crate) held: BTreeMap<AgentId, StopFor>,
    /// The agents whose new launches desktop refuses, with the code their turns fail with
    /// (`HostHandle::set_launch_refused`, `HostConfig::with_launch_refused`). In memory only.
    pub(crate) refused: LaunchRefusals,
}

impl Default for SignIn {
    fn default() -> Self {
        let (splits, receiver) = login_splits::channel();
        Self {
            check: None,
            again: None,
            splits,
            receiver,
            reused: BTreeSet::new(),
            seen: BTreeMap::new(),
            held: BTreeMap::new(),
            refused: LaunchRefusals::default(),
        }
    }
}

impl std::fmt::Debug for SignIn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignIn")
            .field("check", &self.check.is_some())
            .field("again", &self.again.is_some())
            .field("reused", &self.reused)
            .field("seen", &self.seen)
            .field("held", &self.held)
            .field("refused", &self.refused)
            .finish_non_exhaustive()
    }
}

impl SignIn {
    /// The splits' receiving end, for `HostHandle::login_splits`.
    pub(crate) fn receiver(&self) -> LoginSplits {
        self.receiver.clone()
    }

    /// Sends `split` to desktop without waiting, in place of its agent's unread one
    /// (`login_splits.rs`).
    pub(crate) fn send_split(&self, split: LoginSplit) {
        self.splits.send(split);
    }

    /// Whether the guard refuses a login change for `agent` (a typed `/login`): with a hook
    /// set, while its login is reused or its last check's `shared` counts as shared.
    pub(crate) fn guards(&self, agent: &AgentId) -> bool {
        self.check.is_some()
            && (self.reused.contains(agent)
                || self
                    .seen
                    .get(agent)
                    .is_some_and(|seen| treated_as_shared(seen.shared)))
    }

    /// Whether `text`, an agent error's, asks for a sign-in again by the test a caller set.
    pub(crate) fn sign_in_again(&self, text: &str) -> bool {
        self.again.is_some_and(|again| again(text))
    }
}

/// Asks `hook` about `agent`'s login in `home` off the actor, both its calls in one job, for
/// `matter`'s launch, waiting at most [`CHECK_WAIT`] on `clock`.
pub(crate) async fn check(
    clock: Arc<dyn Clock>,
    matter: MatterId,
    hook: Arc<dyn LoginCheck + Send + Sync>,
    agent: AgentId,
    home: PathBuf,
) -> Checked {
    let job = Job {
        what: "login check",
        matter: Some(matter),
        wait: CHECK_WAIT,
    };
    let asked = agent.clone();
    let asked_both = move || Ok((hook.check(&asked, &home), hook.shared(&asked, &home)));
    match off_actor(clock, job, asked_both).await {
        Ok((link, shared)) => {
            tracing::info!(%agent, ?link, ?shared, "the login check answered");
            Checked {
                link: Seen::Answered(link),
                shared,
            }
        }
        Err(error) => {
            tracing::warn!(%agent, code = error.code().as_str(), "the login check gave no answer");
            Checked {
                link: Seen::Unknown,
                shared: Shared::Unknown,
            }
        }
    }
}

/// Removes the `auth.json` link in each of `agent`'s state folders under `data`
/// (`state_folder::linked_folders`: every matter's, listed through no link) on a
/// `clauseo-codex-fs` thread, waiting at most [`REMOVE_WAIT`] on the system clock; a missing file
/// is no error, and an agent with no links (Claude) removes nothing. The first failure fails the
/// call with `agent.launch_failed`, its message naming no path.
pub(crate) async fn remove_links(agent: AgentId, data: PathBuf) -> Result<(), Error> {
    let job = Job {
        what: "login link removal",
        matter: None,
        wait: REMOVE_WAIT,
    };
    let clock: Arc<dyn Clock> = Arc::new(crate::clock::SystemClock);
    let listed = agent.clone();
    let removed = off_actor(clock, job, move || {
        let folders = super::state_folder::linked_folders(&data, listed.as_str());
        remove_in(&folders)
    })
    .await;
    if let Err(error) = &removed {
        tracing::warn!(%agent, code = error.code().as_str(), "the login links were not removed");
    }
    removed
}

/// Removes the `auth.json` link in each of `folders`, never what it points to.
fn remove_in(folders: &[PathBuf]) -> Result<(), Error> {
    for folder in folders {
        match crate::write::remove_link(&folder.join(super::state_folder::LOGIN_FILE)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::refused(
                    ErrorCode::AgentLaunchFailed,
                    format!("a matter's login link was not removed: {}", error.kind()),
                ));
            }
        }
    }
    Ok(())
}
