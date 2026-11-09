//! The guard's state as the app reads it, for Settings: the record's `MachineLaunchDeny`
//! ([`super::STATE_VALUE`], readable by Users) typed. Set-up never fails on a degraded guard
//! and launches go ahead; this is how the lawyer and firm IT see it. The SYSTEM refresh writes
//! the value again at each run, after trying the write and the read-back again.
//! `crate::windows::sandbox_launch_deny` reads it on Windows; [`LaunchDenyState::from_record`]
//! is pure and tested on every OS.

use super::Guard;

/// The COM launch deny's state, as the record holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchDenyState {
    /// No state recorded: no set-up has applied the guard, or a removal took it off.
    NotSet,
    /// The deny holds: in the `Ole` value (`applied`), or in a Group Policy value whose SDDL
    /// holds it (`policy`).
    Applied,
    /// The deny doesn't hold; why.
    Degraded {
        /// Why, as the record says.
        reason: DegradedReason,
    },
}

/// Why the COM launch deny doesn't hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradedReason {
    /// A Group Policy value overrides it without the deny, or a Group Policy access value is
    /// set, so nothing was copied (`degraded: group policy`).
    GroupPolicy,
    /// The value isn't one Clauseo can safely change (`degraded: unparsable`).
    Unparsable,
    /// A registry read or write failed (`degraded: failed`).
    Failed,
    /// DCOM moved the value after the write (`degraded: dcom moved the value`).
    Moved,
    /// No access value, and COM's own couldn't be read (`degraded: machine access unknown`).
    AccessUnknown,
    /// A state this build doesn't know (a newer Clauseo's, or an edit). The text is logged where
    /// it is read, never carried.
    Other,
    /// No launch value, and COM's own couldn't be read (`degraded: machine launch unknown`).
    LaunchUnknown,
}

impl LaunchDenyState {
    /// The state `text` (the record's `MachineLaunchDeny`; `None` when missing) stands for.
    pub fn from_record(text: Option<&str>) -> LaunchDenyState {
        let Some(text) = text else {
            return LaunchDenyState::NotSet;
        };
        let reason = match Guard::ALL.iter().find(|guard| guard.text() == text) {
            Some(Guard::Applied | Guard::ByPolicy) => return LaunchDenyState::Applied,
            Some(Guard::PolicyOverrides) => DegradedReason::GroupPolicy,
            Some(Guard::Unparsable) => DegradedReason::Unparsable,
            Some(Guard::Failed) => DegradedReason::Failed,
            Some(Guard::Moved) => DegradedReason::Moved,
            Some(Guard::AccessUnknown) => DegradedReason::AccessUnknown,
            Some(Guard::LaunchUnknown) => DegradedReason::LaunchUnknown,
            None => {
                tracing::warn!(
                    state = text,
                    "the COM launch guard's recorded state is unknown"
                );
                DegradedReason::Other
            }
        };
        LaunchDenyState::Degraded { reason }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Guard;
    use super::{DegradedReason, LaunchDenyState};

    #[test]
    fn every_recorded_state_reads_back_typed() {
        assert_eq!(LaunchDenyState::from_record(None), LaunchDenyState::NotSet);
        for guard in Guard::ALL {
            let state = LaunchDenyState::from_record(Some(guard.text()));
            assert_eq!(
                matches!(state, LaunchDenyState::Degraded { .. }),
                guard.degraded(),
                "{guard:?}"
            );
        }
        let degraded = |reason| LaunchDenyState::Degraded { reason };
        let cases = [
            ("applied", LaunchDenyState::Applied),
            ("policy", LaunchDenyState::Applied),
            (
                "degraded: group policy",
                degraded(DegradedReason::GroupPolicy),
            ),
            ("degraded: unparsable", degraded(DegradedReason::Unparsable)),
            ("degraded: failed", degraded(DegradedReason::Failed)),
            (
                "degraded: dcom moved the value",
                degraded(DegradedReason::Moved),
            ),
            (
                "degraded: machine access unknown",
                degraded(DegradedReason::AccessUnknown),
            ),
            (
                "degraded: machine launch unknown",
                degraded(DegradedReason::LaunchUnknown),
            ),
            ("degraded: something new", degraded(DegradedReason::Other)),
        ];
        for (text, state) in cases {
            assert_eq!(LaunchDenyState::from_record(Some(text)), state, "{text}");
        }
    }
}
