//! The sandbox as the window sees it (decision record 0051): the tier this computer can give,
//! each matter's protection, and why a matter runs unprotected. Never logged: the query
//! `HostQuery::Protection`, the row field `MatterSummary::protection` and the app event
//! `HostEvent::ProtectionChanged` carry these types, so a new variant is additive (0002's
//! freeze rules: the desktop app ships with the host).
//!
//! - [`ProtectionTier`] and [`Protection`] are closed: the tiers are the threat model's three
//!   (`docs/threat-model.md`), and a matter either runs in the sandbox or does not.
//! - [`NotProtectedCause`] is open: more causes will come, and a newer host's cause decodes to
//!   `Unknown` here instead of failing the whole answer.
//! - Nothing here names a Windows account or a SID: [`NotProtectedCause::PoolFull`] carries
//!   counts only. The host logs the accounts, on the host only.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::open_enum::{enum_with_names, open_data_enum};

enum_with_names! {
    /// How strongly this computer can sandbox an agent (`docs/threat-model.md`, the tiers).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "specta", derive(specta::Type))]
    pub enum ProtectionTier {
        /// The OS's own sandbox around the agent's whole process tree, with no admin needed:
        /// Seatbelt on macOS, MXC on Windows.
        A,
        /// On Windows, a sandbox user per matter from a pool set up once with an administrator's
        /// consent (decision records 0038 and 0049).
        B,
        /// No sandbox: the agent runs as the lawyer, in the adapters' ask modes; the matter shows
        /// Not protected.
        C,
    }
}

impl ProtectionTier {
    /// Every value, strongest first.
    pub const ALL: [Self; 3] = [Self::A, Self::B, Self::C];
}

enum_with_names! {
    /// Whether a matter's agents run in the sandbox, or this computer's state when no matter is
    /// named (`ProtectionView::computer`).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "specta", derive(specta::Type))]
    pub enum Protection {
        /// The agents run in the sandbox at `tier`: `a` or `b` only. The host never sends
        /// `Protected { tier: c }`; tier C is `NotProtected`.
        Protected {
            /// The sandbox's tier.
            tier: ProtectionTier,
        },
        /// No sandbox for `cause`. Under `sandbox.required` false, the agents run at tier C in
        /// the ask modes (auto only when `sandbox.allow_unprotected_auto` is true); under
        /// `sandbox.required` true, no agent starts (`NotProtectedCause` gives which code).
        NotProtected {
            /// Why.
            cause: NotProtectedCause,
        },
    }
}

open_data_enum! {
    /// Why a matter, or this computer, has no sandbox. A cause from a newer Clauseo decodes to
    /// `Unknown`. What follows depends on `sandbox.required` (decision record 0051): when it is
    /// false, any cause runs the agents at tier C, in the ask modes (auto only when
    /// `sandbox.allow_unprotected_auto` is true), with no refusal; when it is true, there is
    /// never a silent fallback: `NeedsSetUp` and `SetUpRefused`, which the set-up fixes, refuse
    /// `CreateThread` and a turn's launch with `sandbox.setup_required`, and every other cause
    /// (`Unknown` included) with `sandbox.required`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum NotProtectedCause {
        /// This computer has no sandbox Clauseo can use: Linux, Windows on ARM, or a build without
        /// the sandbox.
        Unsupported = "unsupported",
        /// The sandbox needs a one-time set-up with an administrator's consent, not done yet.
        /// Tier C when `sandbox.required` is false; `sandbox.setup_required` when it is true.
        NeedsSetUp = "needs_set_up",
        /// The set-up was asked for, and the lawyer declined the administrator prompt or Windows
        /// refused to raise it.
        SetUpRefused = "set_up_refused",
        /// The set-up, or the check that the sandbox works, failed.
        SetUpFailed = "set_up_failed",
        /// Every agent user of the set-up pool is taken, so this matter has no sandbox until one
        /// is free. Counts only, never an account name or a SID.
        PoolFull = "pool_full" {
            /// The pool's users in all.
            size: u8,
            /// Users held by another live matter's open session.
            open: u8,
            /// Users held by a matter that still has them, with no session open.
            held: u8,
            /// Users owed a permission hand-back, which clears at the next launch.
            owed: u8,
            /// Users that still own set-aside items.
            set_aside: u8,
        },
        /// The lawyer switched protection off for the matter at `at`.
        SwitchedOff = "switched_off" {
            /// When.
            at: Timestamp,
        },
        _ =>
        /// A cause from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod not_protected_cause with [
        NotProtectedCause::Unsupported,
        NotProtectedCause::NeedsSetUp,
        NotProtectedCause::SetUpRefused,
        NotProtectedCause::SetUpFailed,
        NotProtectedCause::PoolFull {
            size: 8,
            open: 3,
            held: 2,
            owed: 2,
            set_aside: 1,
        },
        NotProtectedCause::SwitchedOff {
            at: Timestamp::ZERO,
        },
    ];
}

/// The answer to `HostQuery::Protection`: this computer's sandbox and the firm's two protection
/// settings. A struct, so later fields are additive; it accepts fields it does not know. The
/// host builds it with [`ProtectionView::new`], so `tier` always agrees with `computer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ProtectionView {
    /// The best tier this computer can give now: `computer`'s tier when it is `protected`, `c`
    /// when it is not (until a needed set-up is done, say).
    pub tier: ProtectionTier,
    /// This computer's protection state: `protected` at `tier`, or `not_protected` with why.
    pub computer: Protection,
    /// `sandbox.required`: the firm requires the sandbox, so no agent starts without it.
    pub required: bool,
    /// `sandbox.allow_unprotected_auto`: an agent outside the sandbox may still run in an auto
    /// mode. When `false`, it runs only in the ask modes.
    pub unprotected_auto_allowed: bool,
}

impl ProtectionView {
    /// The view of `computer`, with `tier` taken from it: its tier when protected, C when not.
    /// `Protected { tier: C }` is a contradiction the host never builds (debug assertion).
    #[must_use]
    pub fn new(computer: Protection, required: bool, unprotected_auto_allowed: bool) -> Self {
        let tier = match &computer {
            Protection::Protected { tier } => {
                debug_assert_ne!(*tier, ProtectionTier::C, "protected at tier C");
                *tier
            }
            Protection::NotProtected { .. } => ProtectionTier::C,
        };
        Self {
            tier,
            computer,
            required,
            unprotected_auto_allowed,
        }
    }
}
