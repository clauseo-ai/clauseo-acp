//! What a launch's 1385 is (`RunExit::LogonRight` or `RunExit::LogonPolicy`), from the
//! record's `LogonPolicy` and the cached policy read (0049, "Logon rights").

use super::RunExit;
use crate::windows::setup::rights_policy::{PolicyState, RightPolicy};

/// What a launch's 1385 is. First what the record's `LogonPolicy` says (`recorded`; set-up
/// and the SYSTEM refresh write it, since the desktop user can't read the policy): a policy that
/// defines "Allow log on locally" without the pool is [`RunExit::LogonPolicy`], with
/// [`Blocked::LogonPolicy`](crate::Blocked::LogonPolicy)'s words; none, or with the pool, is
/// [`RunExit::LogonRight`] (setting up again grants it). When the record says nothing or
/// `unknown`, `cached` (`setup::rights_source::cached`, readable on some machines) decides the
/// same way, and anything it can't read stays [`RunExit::LogonRight`].
pub fn logon_refusal(
    recorded: Option<PolicyState>,
    cached: impl FnOnce() -> RightPolicy,
) -> (RunExit, crate::Blocked) {
    let state = match recorded {
        Some(PolicyState::Unknown) | None => PolicyState::of(cached()),
        Some(state) => state,
    };
    match state {
        PolicyState::DefinedWithoutPool => (RunExit::LogonPolicy, crate::Blocked::LogonPolicy),
        PolicyState::DefinedWithPool | PolicyState::NotDefined | PolicyState::Unknown => {
            (RunExit::LogonRight, crate::Blocked::LogonRight)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_1385_names_the_policy_from_the_recorded_state() {
        let without = || RightPolicy::Defined {
            includes_pool: false,
        };
        let unread = || RightPolicy::Unknown;
        let policy = (RunExit::LogonPolicy, crate::Blocked::LogonPolicy);
        let local = (RunExit::LogonRight, crate::Blocked::LogonRight);
        // The record decides: without the pool is the IT-policy words, whatever the cache says.
        assert_eq!(
            logon_refusal(Some(PolicyState::DefinedWithoutPool), unread),
            policy
        );
        let words = policy.1.to_string();
        assert!(words.starts_with("This computer's settings don't let Clauseo's sandbox accounts"));
        assert!(
            !words.contains("set up again") && !words.contains("Setting up"),
            "{words}"
        );
        // Not defined: set-up grants it, so the words say setting up again (the cache isn't asked).
        assert_eq!(logon_refusal(Some(PolicyState::NotDefined), without), local);
        assert_eq!(
            logon_refusal(Some(PolicyState::DefinedWithPool), without),
            local
        );
        // Unknown, or no value: the cache decides, and an unreadable cache keeps today's words.
        for recorded in [Some(PolicyState::Unknown), None] {
            assert_eq!(logon_refusal(recorded, unread), local, "{recorded:?}");
            assert_eq!(logon_refusal(recorded, without), policy, "{recorded:?}");
            assert_eq!(logon_refusal(recorded, || RightPolicy::NotDefined), local);
        }
    }
}
