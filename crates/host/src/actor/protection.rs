//! This computer's protection on the actor (decision record 0051, R17 PR-B1): the cache of the
//! sandbox's detection, the view `HostQuery::Protection` answers and
//! `HostEvent::ProtectionViewChanged` carries, each matter's protection (the computer's, until
//! R18's switch and PR-E's pool), and `sandbox.required`'s refusals. The two commands that
//! change it run in `protection/work.rs`; a forgotten matter's sandbox state goes in
//! `protection/forget.rs`.
//!
//! - The cache is never filled on the actor: `detect` reads files and the registry, and macOS'
//!   first call starts `sandbox-exec`. `Host::start` detects once on its calling thread before
//!   the host serves ([`observe`]); each set-up, removal or check detects again on its own
//!   thread, then posts the result here.
//! - With no seam the computer is `Unsupported { why: not_built }`, never what
//!   `clauseo_sandbox::detect` would say on its own (tier A on macOS, with nothing jailing).
//! - The view goes to every app subscriber whenever it changes: the detection, the work in
//!   progress, `retry_asks_admin`, or either setting (the loop's [`tick`] compares the
//!   settings after every input, so any path that changes them is covered). Like
//!   `ProtectionChanged`, no app catch-up carries it: the query gives the current view.
//! - A change of the computer's protection sends `ProtectionChanged` for every listed matter.

use std::path::Path;

use clauseo_protocol::{
    ErrorCode, HostEvent, NotProtectedCause, Protection, ProtectionTier, ProtectionView,
    ProtectionWork, Timestamp, UnsupportedWhy,
};
use clauseo_sandbox::{Blocked, Detection, NoSandbox, SET_UP_BY_OTHER_USER, SetupState, Tier};

use super::Actor;
use crate::config::Sandbox;
use crate::error::Error;

pub(super) mod forget;
mod set_up_at;
pub(super) mod work;

/// What one detection found: the computer's protection, whether a retry would run the set-up
/// (`retry_asks_admin`), and when a finished set-up finished (`set_up_at`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Observed {
    pub(crate) computer: Protection,
    pub(crate) asks_admin: bool,
    pub(crate) set_up_at: Option<Timestamp>,
}

/// The protection state the actor holds.
pub(crate) struct ProtectionHost {
    /// The seam (`HostConfig::with_sandbox`), if any.
    sandbox: Option<Sandbox>,
    /// The last detection.
    observed: Observed,
    /// The set-up, removal or check running, if any (`work.rs`).
    running: Option<work::Running>,
    /// The id the next work takes, so a late result of an earlier one is told apart.
    next_work: u64,
    /// The view last sent to app subscribers (at start, the start's view).
    sent: Option<ProtectionView>,
}

impl ProtectionHost {
    /// The state at start: the seam and its first detection.
    pub(crate) fn new(sandbox: Option<Sandbox>, first: Observed) -> Self {
        Self {
            sandbox,
            observed: first,
            running: None,
            next_work: 0,
            sent: None,
        }
    }

    /// The launchers seam (R17 PR-B2) and the options its launches start with, when set: every
    /// agent launch goes through it. `None`: the host's own launcher, as before.
    pub(crate) fn launchers(
        &self,
    ) -> Option<(
        std::sync::Arc<dyn crate::config::SandboxLaunchers>,
        clauseo_process::SpawnOptions,
    )> {
        let sandbox = self.sandbox.as_ref()?;
        let launchers = sandbox.launchers.clone()?;
        Some((launchers, sandbox.launch_opts.clone()))
    }

    /// The computer's protection now.
    pub(crate) fn computer(&self) -> &Protection {
        &self.observed.computer
    }

    /// Whether the set-up's removal runs.
    fn removing(&self) -> bool {
        self.running
            .as_ref()
            .is_some_and(|running| running.work() == ProtectionWork::Remove)
    }

    /// The view under the settings `required` and `auto` (`sandbox.allow_unprotected_auto`).
    fn view(&self, required: bool, auto: bool) -> ProtectionView {
        let mut view = ProtectionView::new(self.observed.computer.clone(), required, auto);
        view.in_progress = self.running.as_ref().map(work::Running::work);
        view.retry_asks_admin = self.observed.asks_admin;
        view.set_up_at = self.observed.set_up_at;
        view
    }

    /// The view under these settings when it differs from the one last sent, which it then
    /// becomes; `None` when nothing changed.
    fn take_changed(&mut self, required: bool, auto: bool) -> Option<ProtectionView> {
        let view = self.view(required, auto);
        if self.sent.as_ref() == Some(&view) {
            return None;
        }
        self.sent = Some(view.clone());
        Some(view)
    }

    /// Whether the settings moved from the view last sent.
    fn settings_moved(&self, required: bool, auto: bool) -> bool {
        self.sent
            .as_ref()
            .is_none_or(|sent| sent.required != required || sent.unprotected_auto_allowed != auto)
    }
}

/// Detects this computer's protection through `sandbox`, on the calling thread: never the
/// actor's. No seam: `Unsupported { why: not_built }`, with no retry to offer. A set-up that is
/// done gives its time from its marker in `data_dir` (`set_up_at`).
pub(crate) fn observe(sandbox: Option<&Sandbox>, data_dir: &Path) -> Observed {
    let Some(sandbox) = sandbox else {
        return Observed {
            computer: Protection::NotProtected {
                cause: NotProtectedCause::Unsupported {
                    why: UnsupportedWhy::NotBuilt,
                },
            },
            asks_admin: false,
            set_up_at: None,
        };
    };
    let detection = (sandbox.detect)();
    let state = sandbox.setup.as_ref().map(|setup| setup.state());
    let set_up_at = match state {
        Some(SetupState::Ready) => set_up_at::read(data_dir),
        _ => None,
    };
    Observed {
        computer: protection_of(&detection),
        asks_admin: state.as_ref().is_some_and(asks_admin),
        set_up_at,
    }
}

/// Whether a set-up in `state` is one `RetryProtection` runs, with its administrator prompt:
/// needed, refused, failed or outdated. Done, owned by another Windows user, or blocked by a
/// setting only whoever looks after the computer can change (R17 PR-E: no "Try again", the
/// remedy is IT's or the installer's), it is not: a set-up cannot fix those.
pub(crate) fn asks_admin(state: &SetupState) -> bool {
    match state {
        SetupState::Needed
        | SetupState::Refused
        | SetupState::Failed(_)
        | SetupState::Outdated { .. } => true,
        SetupState::Ready | SetupState::SetUpByOtherUser | SetupState::Blocked(_) => false,
    }
}

/// The wire's protection for a detection. A failure's text is logged by its kind only, never
/// sent (0047: it may name a path).
pub(crate) fn protection_of(detection: &Detection) -> Protection {
    let tier = match detection.tier {
        Tier::A => Some(ProtectionTier::A),
        Tier::B => Some(ProtectionTier::B),
        Tier::C => None,
    };
    if let Some(tier) = tier {
        return Protection::Protected { tier };
    }
    let cause = match &detection.reason {
        Some(NoSandbox::UnsupportedOs) => NotProtectedCause::Unsupported {
            why: if cfg!(all(windows, target_arch = "aarch64")) {
                UnsupportedWhy::WindowsArm
            } else {
                UnsupportedWhy::Os
            },
        },
        Some(NoSandbox::NotBuilt) => NotProtectedCause::Unsupported {
            why: UnsupportedWhy::NotBuilt,
        },
        Some(NoSandbox::NeedsSetUp) => NotProtectedCause::NeedsSetUp,
        Some(NoSandbox::SetUpRefused) => NotProtectedCause::SetUpRefused,
        Some(NoSandbox::ProbeFailed(_)) => failed("probe_failed"),
        Some(NoSandbox::SetUpFailed(why)) if why == SET_UP_BY_OTHER_USER => {
            NotProtectedCause::OtherAccount
        }
        Some(NoSandbox::SetUpFailed(_)) => failed("set_up_failed"),
        Some(NoSandbox::SetUpBlocked(Blocked::OtherAccount)) => NotProtectedCause::OtherAccount,
        // The sandbox's name, passed through: an open set the app reads (logon_right, policy,
        // admin_copy_*, tool_missing, and any later one).
        Some(NoSandbox::SetUpBlocked(blocked)) => NotProtectedCause::Blocked {
            reason: blocked.name().to_owned(),
        },
        None => failed("no_reason"),
        // `NoSandbox` is non-exhaustive: a reason this host does not know fails closed.
        Some(_) => failed("unknown"),
    };
    Protection::NotProtected { cause }
}

/// `SetUpFailed`, with a log line naming `kind` only.
fn failed(kind: &'static str) -> NotProtectedCause {
    tracing::warn!(kind, "the sandbox check found no working sandbox");
    NotProtectedCause::SetUpFailed
}

/// The settings `sandbox.required` and `sandbox.allow_unprotected_auto`.
fn settings(actor: &Actor) -> (bool, bool) {
    let sandbox = &actor.effective.settings().sandbox;
    (sandbox.required, sandbox.allow_unprotected_auto)
}

/// The view `HostQuery::Protection` answers.
pub(crate) fn view(actor: &Actor) -> ProtectionView {
    let (required, auto) = settings(actor);
    actor.protection.view(required, auto)
}

/// The start's view, as the one sent: a later change is what goes out.
pub(super) fn start(actor: &mut Actor) {
    let (required, auto) = settings(actor);
    let _first = actor.protection.take_changed(required, auto);
}

/// Sends `ProtectionViewChanged` to every app subscriber when the view changed.
pub(super) fn publish_view(actor: &mut Actor) {
    let (required, auto) = settings(actor);
    if let Some(view) = actor.protection.take_changed(required, auto) {
        super::app::publish(actor, &HostEvent::ProtectionViewChanged { view });
    }
}

/// After every input: when either protection setting changed, the new view goes out.
pub(super) fn tick(actor: &mut Actor) {
    let (required, auto) = settings(actor);
    if actor.protection.settings_moved(required, auto) {
        publish_view(actor);
    }
}

/// The computer's protection changed: `ProtectionChanged` for every listed matter (B1: each
/// matter's is the computer's), and every row compared at the next flush.
fn matters_changed(actor: &mut Actor) {
    let protection = actor.protection.computer().clone();
    let matters: Vec<_> = actor.matters.keys().copied().collect();
    for matter in matters {
        let event = HostEvent::ProtectionChanged {
            matter,
            protection: protection.clone(),
        };
        super::app::publish(actor, &event);
    }
    actor.lists.touch_all();
}

/// The refusal of `CreateThread` and of a turn's launch by this computer's protection: none
/// when it is protected. A computer that has the sandbox but can't use it now refuses whatever
/// `sandbox.required` or `sandbox.allow_unprotected_auto` say (R17 PR-E; that setting covers
/// only a computer that can't have the sandbox): `Blocked` and `OtherAccount` with
/// `sandbox.blocked`, `PoolFull` with `sandbox.pool_full`, `PoolOwned` with
/// `sandbox.pool_owned`. Every other cause refuses only under `sandbox.required`:
/// `sandbox.setup_required` when the set-up would fix it (`NeedsSetUp`, `SetUpRefused`),
/// `sandbox.required` otherwise.
///
/// While the sandbox's set-up is being removed (`RemoveSandboxSetup`, the view's `in_progress:
/// remove`), every launch and `CreateThread` is refused with `sandbox.removing` (retryable)
/// first: the cache still says what the computer had before, and the removal ends what runs as
/// a pool user.
pub(crate) fn refusal(actor: &Actor) -> Option<Error> {
    if actor.protection.removing() {
        return Some(Error::refused(
            ErrorCode::SandboxRemoving,
            "the sandbox's set-up is being removed; try again when it ends",
        ));
    }
    let Protection::NotProtected { cause } = actor.protection.computer() else {
        return None;
    };
    refusal_of(cause, actor.effective.settings().sandbox.required)
}

/// [`refusal`] for `cause`, under `sandbox.required` = `required`.
fn refusal_of(cause: &NotProtectedCause, required: bool) -> Option<Error> {
    match cause {
        NotProtectedCause::Blocked { .. } | NotProtectedCause::OtherAccount => {
            return Some(Error::refused(
                ErrorCode::SandboxBlocked,
                "a setting only whoever looks after this computer can change blocks the sandbox",
            ));
        }
        // TODO(stage C's slot-groups PR): the sandbox's `Error::PoolFull { why }` gives this
        // cause at a tier-B launch, mapped one-to-one into `why`; until then no host builds it.
        NotProtectedCause::PoolFull { .. } => {
            return Some(Error::refused(
                ErrorCode::SandboxPoolFull,
                "every agent user of the sandbox's pool is held by another matter",
            ));
        }
        NotProtectedCause::PoolOwned => {
            return Some(Error::refused(
                ErrorCode::SandboxPoolOwned,
                "a sandbox user still owns an item in this matter's folder",
            ));
        }
        NotProtectedCause::Unsupported { .. }
        | NotProtectedCause::NeedsSetUp
        | NotProtectedCause::SetUpRefused
        | NotProtectedCause::SetUpFailed
        | NotProtectedCause::SwitchedOff { .. }
        | NotProtectedCause::Unknown { .. } => {}
    }
    if !required {
        return None;
    }
    Some(match cause {
        NotProtectedCause::NeedsSetUp | NotProtectedCause::SetUpRefused => Error::refused(
            ErrorCode::SandboxSetupRequired,
            "the firm requires the sandbox, and its set-up is not done",
        ),
        NotProtectedCause::Unsupported { .. }
        | NotProtectedCause::SetUpFailed
        | NotProtectedCause::PoolFull { .. }
        | NotProtectedCause::Blocked { .. }
        | NotProtectedCause::OtherAccount
        | NotProtectedCause::PoolOwned
        | NotProtectedCause::SwitchedOff { .. }
        | NotProtectedCause::Unknown { .. } => Error::refused(
            ErrorCode::SandboxRequired,
            "the firm requires the sandbox, and this computer has none working",
        ),
    })
}

#[cfg(test)]
mod tests;
