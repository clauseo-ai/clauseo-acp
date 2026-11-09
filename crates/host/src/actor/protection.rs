//! This computer's protection on the actor (decision record 0051, R17 PR-B1): the cache of the
//! sandbox's detection, the view `HostQuery::Protection` answers and
//! `HostEvent::ProtectionViewChanged` carries, each matter's protection (the computer's, until
//! R18's switch and PR-E's pool), and `sandbox.required`'s refusals. The two commands that
//! change it run in `protection/work.rs`.
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

use clauseo_protocol::{
    ErrorCode, HostEvent, NotProtectedCause, Protection, ProtectionTier, ProtectionView,
    UnsupportedWhy,
};
use clauseo_sandbox::{Detection, NoSandbox, SetupState, Tier};

use super::Actor;
use crate::config::Sandbox;
use crate::error::Error;

pub(super) mod work;

/// What one detection found: the computer's protection, and whether a retry would run the
/// set-up (`retry_asks_admin`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Observed {
    pub(crate) computer: Protection,
    pub(crate) asks_admin: bool,
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

    /// The computer's protection now.
    pub(crate) fn computer(&self) -> &Protection {
        &self.observed.computer
    }

    /// The view under the settings `required` and `auto` (`sandbox.allow_unprotected_auto`).
    fn view(&self, required: bool, auto: bool) -> ProtectionView {
        let mut view = ProtectionView::new(self.observed.computer.clone(), required, auto);
        view.in_progress = self.running.as_ref().map(work::Running::work);
        view.retry_asks_admin = self.observed.asks_admin;
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
/// actor's. No seam: `Unsupported { why: not_built }`, with no retry to offer.
pub(crate) fn observe(sandbox: Option<&Sandbox>) -> Observed {
    let Some(sandbox) = sandbox else {
        return Observed {
            computer: Protection::NotProtected {
                cause: NotProtectedCause::Unsupported {
                    why: UnsupportedWhy::NotBuilt,
                },
            },
            asks_admin: false,
        };
    };
    let detection = (sandbox.detect)();
    let asks_admin = sandbox
        .setup
        .as_ref()
        .is_some_and(|setup| asks_admin(&setup.state()));
    Observed {
        computer: protection_of(&detection),
        asks_admin,
    }
}

/// Whether a set-up in `state` is one `RetryProtection` runs, with its administrator prompt:
/// needed, refused, failed, blocked by a Windows policy (stage C's D-B4: trying again asks the
/// administrator, R17 PR-E names it), outdated, or working with a repair to offer. Done, or
/// owned by another Windows user, it is not: a set-up cannot fix those.
pub(crate) fn asks_admin(state: &SetupState) -> bool {
    match state {
        SetupState::Needed
        | SetupState::Refused
        | SetupState::Failed(_)
        | SetupState::Blocked(_)
        | SetupState::Outdated { .. }
        | SetupState::RepairNeeded(_) => true,
        SetupState::Ready | SetupState::SetUpByOtherUser => false,
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
        Some(NoSandbox::SetUpFailed(_)) => failed("set_up_failed"),
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

/// `sandbox.required`'s refusal of `CreateThread` and of a turn's launch: none when the setting
/// is off or the computer is protected; `sandbox.setup_required` when the set-up would fix it
/// (`NeedsSetUp`, `SetUpRefused`); `sandbox.required` for every other cause.
pub(crate) fn refusal(actor: &Actor) -> Option<Error> {
    if !actor.effective.settings().sandbox.required {
        return None;
    }
    let Protection::NotProtected { cause } = actor.protection.computer() else {
        return None;
    };
    Some(match cause {
        NotProtectedCause::NeedsSetUp | NotProtectedCause::SetUpRefused => Error::refused(
            ErrorCode::SandboxSetupRequired,
            "the firm requires the sandbox, and its set-up is not done",
        ),
        NotProtectedCause::Unsupported { .. }
        | NotProtectedCause::SetUpFailed
        | NotProtectedCause::PoolFull { .. }
        | NotProtectedCause::SwitchedOff { .. }
        | NotProtectedCause::Unknown { .. } => Error::refused(
            ErrorCode::SandboxRequired,
            "the firm requires the sandbox, and this computer has none working",
        ),
    })
}

#[cfg(test)]
mod tests;
