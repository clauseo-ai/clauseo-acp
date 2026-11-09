//! What a launch's 1385 is (`RunExit::LogonRight` or `RunExit::LogonPolicy`), from the
//! record's `LogonPolicy` and the cached policy read (0049, "Logon rights").

use super::RunExit;
use crate::windows::setup::rights_policy::{PolicyState, RightPolicy};

/// The record's `LogonPolicy` as `read` gave it (`record::sandbox_logon_policy`): its state, or
/// `None` when the value is missing, can't be read, or isn't a state's text.
pub fn recorded_state<E>(read: Result<Option<String>, E>) -> Option<PolicyState> {
    read.ok()
        .flatten()
        .and_then(|text| PolicyState::parse(&text))
}

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

    /// The record's value as the registry read gives it, through [`recorded_state`].
    fn from_record(
        read: Result<Option<&str>, &str>,
        cached: RightPolicy,
    ) -> (RunExit, crate::Blocked) {
        let read = read.map(|text| text.map(str::to_owned));
        logon_refusal(recorded_state(read), || cached)
    }

    #[test]
    fn a_launch_reads_each_recorded_state_from_the_record() {
        let policy = (RunExit::LogonPolicy, crate::Blocked::LogonPolicy);
        let local = (RunExit::LogonRight, crate::Blocked::LogonRight);
        let unread = RightPolicy::Unknown;
        let without = RightPolicy::Defined {
            includes_pool: false,
        };
        assert_eq!(
            from_record(Ok(Some("defined_without_pool")), unread),
            policy
        );
        assert_eq!(from_record(Ok(Some("not_defined")), without), local);
        assert_eq!(from_record(Ok(Some("defined_with_pool")), without), local);
        // Unknown, missing, unreadable or malformed: the cache decides.
        for read in [
            Ok(Some("unknown")),
            Ok(None),
            Err("access denied"),
            Ok(Some("Defined")),
        ] {
            assert_eq!(from_record(read, unread), local, "{read:?}");
            assert_eq!(from_record(read, without), policy, "{read:?}");
        }
    }
}
