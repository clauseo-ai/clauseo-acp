//! The sandbox as the window sees it (decision record 0051): the tier this computer can give,
//! each matter's protection, and why a matter runs unprotected. Never logged: the query
//! `HostQuery::Protection`, the row field `MatterSummary::protection` and the app event
//! `HostEvent::ProtectionChanged` carry these types, so a new variant is additive (0002's
//! freeze rules: the desktop app ships with the host).
//!
//! - [`ProtectionTier`] and [`Protection`] are closed: the tiers are the threat model's three
//!   (`docs/threat-model.md`), and a matter either runs in the sandbox or does not.
//! - [`NotProtectedCause`] is open: more causes will come, and a newer host's cause decodes to
//!   `Unknown` here instead of failing the whole answer. So are [`UnsupportedWhy`] and
//!   [`ProtectionWork`].
//! - `HostEvent::ProtectionViewChanged` carries the whole [`ProtectionView`] each time it
//!   changes: the detection, the work in progress, `retry_asks_admin`, or either setting.
//! - Nothing here names a Windows account or a SID: [`NotProtectedCause::PoolFull`] carries
//!   counts only. The host logs the accounts, on the host only.
//! - [`SiteTag`] is open: why the block list names a site the egress proxy refused (decision
//!   record 0053), carried by the app event `HostEvent::SiteBlocked`.

use serde::{Deserialize, Serialize};

use crate::Timestamp;
use crate::open_enum::{enum_with_names, open_data_enum, open_unit_enum};

open_unit_enum! {
    /// Why the block list names a site (decision record 0053). A tag from a newer Clauseo decodes
    /// to `Unknown`.
    pub enum SiteTag {
        /// A botnet's command server (Feodo Tracker's IP addresses).
        Malware,
        /// A phishing site.
        Phishing,
    }
    test mod site_tag;
}

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
        /// the ask modes (auto only when `sandbox.allow_unprotected_auto` is true), except for
        /// the causes that refuse whatever the settings say; under `sandbox.required` true, no
        /// agent starts (`NotProtectedCause` gives which code).
        NotProtected {
            /// Why.
            cause: NotProtectedCause,
        },
    }
}

open_data_enum! {
    /// Why a matter, or this computer, has no sandbox. A cause from a newer Clauseo decodes to
    /// `Unknown`. Six causes refuse `CreateThread` and a turn's launch whatever
    /// `sandbox.required` or `sandbox.allow_unprotected_auto` say, as a computer whose sandbox
    /// is set up but can't be used now, or is broken (R17 PR-E, fail closed): `Blocked`,
    /// `OtherAccount`, `SetUpFailed` and `Unknown` with `sandbox.blocked`, `PoolFull` with
    /// `sandbox.pool_full` and `PoolOwned` with `sandbox.pool_owned`. The rest, a sandbox not
    /// set up at all or switched off, depend on `sandbox.required` (decision record 0051): when
    /// it is false they run the agents at tier C, in the ask modes (auto only when
    /// `sandbox.allow_unprotected_auto` is true), with no refusal; when it is true, there is
    /// never a silent fallback: `NeedsSetUp` and `SetUpRefused`, which the set-up fixes, refuse
    /// with `sandbox.setup_required`, `Unsupported` and `SwitchedOff` with `sandbox.required`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum NotProtectedCause {
        /// This computer has no sandbox Clauseo can use: `why` says which case (an OS without
        /// one, Windows on ARM, or a build without the sandbox). A data variant since R17 PR-B1:
        /// `{"unsupported": {"why": "os"}}`; no released build wrote the plain string.
        Unsupported = "unsupported" {
            /// Which case.
            why: UnsupportedWhy,
        },
        /// The sandbox needs a one-time set-up with an administrator's consent, not done yet.
        /// Tier C when `sandbox.required` is false; `sandbox.setup_required` when it is true.
        NeedsSetUp = "needs_set_up",
        /// The set-up was asked for, and the lawyer declined the administrator prompt or Windows
        /// refused to raise it.
        SetUpRefused = "set_up_refused",
        /// The set-up, or the check that the sandbox works, failed: no agent starts on this
        /// computer whatever either setting says, `sandbox.blocked`, until a set-up repairs it.
        SetUpFailed = "set_up_failed",
        /// Every agent user of the set-up pool is taken, so this matter has no sandbox until one
        /// is free. Counts only, never an account name or a SID; the host passes the sandbox's
        /// numbers through unchanged. `size` and `needs_reset` count the Safe workspace's
        /// places: today a place is a pool user (at most 32); after stage C's slot groups, a
        /// 2-user slot (at most 32 slots, of 64 users).
        PoolFull = "pool_full" {
            /// The places in all.
            size: u8,
            /// Users held by another live matter's open session.
            open: u8,
            /// Users held by a matter that still has them, with no session open.
            held: u8,
            /// Users owed a permission hand-back, which clears at the next launch.
            owed: u8,
            /// Users that still own set-aside items.
            set_aside: u8,
            /// Places whose hand-back failed for good, so only a reset (an administrator's
            /// "Free up protected spaces") frees them; with `why` `no_free_slot`. Missing from
            /// an older host: 0.
            #[serde(default)]
            needs_reset: u8,
            /// Why no user could be given, when the host knows; left out when not (an older
            /// host, or one before the sandbox names it).
            #[serde(default, skip_serializing_if = "Option::is_none")]
            why: Option<PoolFullWhy>,
        },
        /// The lawyer switched protection off for the matter at `at`.
        SwitchedOff = "switched_off" {
            /// When.
            at: Timestamp,
        },
        /// A setting or tool only whoever looks after the computer can change blocks the
        /// sandbox (decision record 0049), so no agent starts on this computer, whatever
        /// `sandbox.required` or `sandbox.allow_unprotected_auto` say: `sandbox.blocked`.
        Blocked = "blocked" {
            /// Which block, as the sandbox names it (`logon_right`, `policy`,
            /// `admin_copy_missing`, `admin_copy_changed`, `admin_copy_unsafe`,
            /// `tool_missing`, ...): an open set, passed through; never `other_account`,
            /// which is `OtherAccount`.
            reason: String,
        },
        /// Another Windows account on this computer owns the sandbox's set-up (one owner per
        /// computer), so no agent starts for this one, whatever either setting says:
        /// `sandbox.blocked`.
        OtherAccount = "other_account",
        /// A sandbox user still owns an item in this matter's folder, because its hand-back
        /// failed, so no agent starts in it: `sandbox.pool_owned`.
        PoolOwned = "pool_owned",
        _ =>
        /// A cause from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod not_protected_cause with [
        NotProtectedCause::Unsupported {
            why: UnsupportedWhy::WindowsArm,
        },
        NotProtectedCause::NeedsSetUp,
        NotProtectedCause::SetUpRefused,
        NotProtectedCause::SetUpFailed,
        NotProtectedCause::PoolFull {
            size: 8,
            open: 3,
            held: 2,
            owed: 2,
            set_aside: 1,
            needs_reset: 1,
            why: Some(PoolFullWhy::NoFreeSlot),
        },
        NotProtectedCause::SwitchedOff {
            at: Timestamp::ZERO,
        },
        NotProtectedCause::Blocked {
            reason: "logon_right".to_owned(),
        },
        NotProtectedCause::OtherAccount,
        NotProtectedCause::PoolOwned,
    ];
}

open_unit_enum! {
    /// Why this computer has no sandbox Clauseo can use (`NotProtectedCause::Unsupported`). A
    /// value from a newer Clauseo decodes to `Unknown`.
    pub enum UnsupportedWhy {
        /// This OS has none in Clauseo: Linux, or a Windows build before Windows 10 22H2.
        Os,
        /// Windows on ARM, which stays without a sandbox until there is one to test on.
        WindowsArm,
        /// This build of Clauseo has no sandbox wired in.
        NotBuilt,
    }
    test mod unsupported_why;
}

open_unit_enum! {
    /// Why the sandbox's pool could give this matter no user (`NotProtectedCause::PoolFull`).
    /// A value from a newer Clauseo decodes to `Unknown`; values are only added at the end.
    pub enum PoolFullWhy {
        /// Every slot is held by a matter with an open session, so none can be reclaimed.
        /// `needs_reset` counts the users only a reset frees.
        NoFreeSlot,
        /// This matter already has two conversations running, the most a matter's slot serves.
        MatterSessionsFull,
    }
    test mod pool_full_why;
}

open_unit_enum! {
    /// What the host is doing to this computer's sandbox now (`ProtectionView::in_progress`). A
    /// value from a newer Clauseo decodes to `Unknown`.
    pub enum ProtectionWork {
        /// The one-time set-up runs (`RetryProtection`): it may wait at an administrator
        /// prompt for minutes.
        SetUp,
        /// The set-up is being removed (`RemoveSandboxSetup`).
        Remove,
        /// The host checks the sandbox again (`RetryProtection` when no set-up can help).
        Check,
    }
    test mod protection_work;
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
    /// What the host is doing to the sandbox now, if anything; left out when nothing.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub in_progress: Option<ProtectionWork>,
    /// Whether `RetryProtection` runs the set-up, which asks for an administrator's consent:
    /// a set-up exists on this computer and is needed, was refused, failed or is outdated.
    /// `false` when a retry only checks again (no set-up here, it is done, or another Windows
    /// user owns it). Missing from an older host: `false`.
    #[serde(default)]
    pub retry_asks_admin: bool,
    /// When this computer's sandbox set-up finished, when the host knows it (Windows' tier B);
    /// left out when not.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub set_up_at: Option<Timestamp>,
    /// The sandbox's own state on this computer when this build sets the sandbox up but does
    /// not start agents in it yet (R17 PR-D3a): `protected { tier }` once set up, else
    /// `not_protected` with why (`needs_set_up`, `set_up_refused`, `set_up_failed`,
    /// `blocked { reason }`, `other_account`). `computer` then says `unsupported { why:
    /// not_built }`, which is how launches go: no agent runs in the Safe workspace yet. It drives
    /// the set-up screen only: `computer` stays the one input to anything that gates or labels
    /// protection (auto modes and `unprotected_auto_allowed`, matter badges, launch refusals).
    /// `None` once agents start in the sandbox, whose state `computer` then carries (one source
    /// of truth), and when there is nothing to set up. Missing from an older host: `None`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub set_up_state: Option<Protection>,
}

impl ProtectionView {
    /// The view of `computer`, with `tier` taken from it: its tier when protected, C when not;
    /// nothing in progress and `retry_asks_admin` false, which the host sets after.
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
            in_progress: None,
            retry_asks_admin: false,
            set_up_at: None,
            set_up_state: None,
        }
    }
}
