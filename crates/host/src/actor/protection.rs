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
//!   progress, `retry_asks_admin`, either setting, or the data fence's names-only flag (the
//!   loop's [`tick`] compares the settings and the flag after every input, so any path that
//!   changes them is covered; a fence check runs on a matter thread, whose reply is an input). Like
//!   `ProtectionChanged`, no app catch-up carries it: the query gives the current view.
//! - A change of the computer's protection sends `ProtectionChanged` for every listed matter.
//! - With no sandbox wired ([`ProtectionHost::sandbox_wired`]) the Safe workspace is parked for
//!   launch (Rohan, 2026-11-09): rows carry no protection, nothing refuses, no view change goes
//!   out, and a set `sandbox.*` key is logged once as ignored. The query still answers.

use clauseo_protocol::{
    ErrorCode, HostEvent, NotProtectedCause, Protection, ProtectionTier, ProtectionView,
    UnsupportedWhy,
};
use clauseo_sandbox::{Detection, NoSandbox, SetupState, Tier};
use clauseo_settings::{Effective, SandboxSettings};

use super::Actor;
use crate::agent::data_fence::DataFenceHealth;
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
    /// With no sandbox wired, the `sandbox.*` keys last logged as set but ignored.
    ignored: Vec<&'static str>,
    /// The data fence's health, shared with every fence check (`agent/data_fence/health.rs`):
    /// the actor only loads its flag, never touching the disk.
    data_fence: DataFenceHealth,
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
            ignored: Vec::new(),
            data_fence: DataFenceHealth::default(),
        }
    }

    /// Whether a sandbox is wired (`HostConfig::with_sandbox`). With none, the host is a plain
    /// ACP client: rows carry no protection, `sandbox.*` never refuses and no view change goes
    /// out (the Safe workspace is parked for launch, Rohan 2026-11-09; it returns by itself when
    /// desktop wires `with_sandbox`).
    pub(crate) fn sandbox_wired(&self) -> bool {
        self.sandbox.is_some()
    }

    /// The data fence's health, for each permission request's fence check.
    pub(crate) fn data_fence(&self) -> &DataFenceHealth {
        &self.data_fence
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
        view.data_fence_names_only = self.data_fence.names_only();
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

    /// With no sandbox wired, one warning naming the `sandbox.*` keys `set` when they differ
    /// from the ones last logged: once per settings load or change, never per launch. Keys only,
    /// no values or paths (0047).
    fn warn_ignored(&mut self, set: Vec<&'static str>) {
        if self.sandbox_wired() || set == self.ignored {
            return;
        }
        if !set.is_empty() {
            let verb = if set.len() == 1 { "is" } else { "are" };
            tracing::warn!(
                "{} {verb} set but ignored in this version: the Safe workspace isn't part of this \
                 release",
                set.join(" and ")
            );
        }
        self.ignored = set;
    }

    /// Whether the settings, or the data fence's names-only flag, moved from the view last
    /// sent.
    fn moved(&self, required: bool, auto: bool) -> bool {
        let names_only = self.data_fence.names_only();
        self.sent.as_ref().is_none_or(|sent| {
            sent.required != required
                || sent.unprotected_auto_allowed != auto
                || sent.data_fence_names_only != names_only
        })
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
/// administrator, R17 PR-E names it) or outdated. Done, or owned by another Windows user, it is
/// not: a set-up cannot fix those.
pub(crate) fn asks_admin(state: &SetupState) -> bool {
    match state {
        SetupState::Needed
        | SetupState::Refused
        | SetupState::Failed(_)
        | SetupState::Blocked(_)
        | SetupState::Outdated { .. } => true,
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

/// The `sandbox.*` keys set, from a policy or `settings.json`: each one a policy locks with
/// its own value, or whose value is not the default (one at its default changes nothing). A key
/// a broken policy locked off (it has a lock reason) was set by no one.
pub(crate) fn set_keys(effective: &Effective) -> Vec<&'static str> {
    let sandbox = &effective.settings().sandbox;
    let default = SandboxSettings::default();
    let reasoned = |key| effective.lock_reason(key).is_some();
    let by_policy = |key| effective.is_locked(key) && !reasoned(key);
    [
        ("sandbox.required", sandbox.required != default.required),
        (
            "sandbox.allow_unprotected_auto",
            sandbox.allow_unprotected_auto != default.allow_unprotected_auto,
        ),
    ]
    .into_iter()
    .filter(|&(key, moved)| by_policy(key) || (moved && !reasoned(key)))
    .map(|(key, _)| key)
    .collect()
}

/// The view `HostQuery::Protection` answers.
pub(crate) fn view(actor: &Actor) -> ProtectionView {
    let (required, auto) = settings(actor);
    actor.protection.view(required, auto)
}

/// The start's view, as the one sent: a later change is what goes out.
pub(super) fn start(actor: &mut Actor) {
    warn_ignored(actor);
    let (required, auto) = settings(actor);
    let _first = actor.protection.take_changed(required, auto);
}

/// Sends `ProtectionViewChanged` to every app subscriber when the view changed.
pub(super) fn publish_view(actor: &mut Actor) {
    // Safe workspace parked for launch: no view goes out until desktop wires `with_sandbox`.
    if !actor.protection.sandbox_wired() {
        return;
    }
    let (required, auto) = settings(actor);
    if let Some(view) = actor.protection.take_changed(required, auto) {
        super::app::publish(actor, &HostEvent::ProtectionViewChanged { view });
    }
}

/// After every input: when either protection setting changed, or the data fence started or
/// stopped comparing by names only (a fence check's reply is an input), the new view goes out.
pub(super) fn tick(actor: &mut Actor) {
    // Safe workspace parked for launch: only the ignored-keys warning, until desktop wires
    // `with_sandbox`.
    if !actor.protection.sandbox_wired() {
        warn_ignored(actor);
        return;
    }
    let (required, auto) = settings(actor);
    if actor.protection.moved(required, auto) {
        publish_view(actor);
    }
}

/// With no sandbox wired, the warning that the `sandbox.*` keys set are ignored, when they changed.
fn warn_ignored(actor: &mut Actor) {
    // Safe workspace parked for launch: the keys are read again once desktop wires `with_sandbox`.
    if actor.protection.sandbox_wired() {
        return;
    }
    let set = set_keys(&actor.effective);
    actor.protection.warn_ignored(set);
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

/// `sandbox.required`'s refusal of `CreateThread` and of a turn's launch: none when no sandbox
/// is wired, the setting is off or the computer is protected; `sandbox.setup_required` when the
/// set-up would fix it (`NeedsSetUp`, `SetUpRefused`); `sandbox.required` for every other cause.
pub(crate) fn refusal(actor: &Actor) -> Option<Error> {
    // Safe workspace parked for launch: never refuses until desktop wires `with_sandbox`.
    if !actor.protection.sandbox_wired() {
        return None;
    }
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
