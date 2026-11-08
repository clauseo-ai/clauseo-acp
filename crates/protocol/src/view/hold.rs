//! Why a thread's queue waits: [`HeaderEntry::queue_held_reason`](super::HeaderEntry) and the
//! same field of a `ListThreads` row. The host sets it beside `queue_held`; nothing logs it. Open,
//! so a newer host's reason decodes.

use crate::open_enum::open_data_enum;

open_data_enum! {
    /// Why a thread's queued turns wait, the most specific reason that applies. Set only while
    /// `queue_held` is true.
    ///
    /// A plain string; a reason from a newer Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum QueueHoldReason {
        /// The queue waits for its agent's sign-in: a turn needed one and went back to the
        /// queue's head. It runs once the agent starts again with the new login.
        SignIn = "sign_in",
        /// The login is switching: Clauseo stopped the agent (`stop_agents`) and starts it again
        /// once the new login is in place.
        Switching = "switching",
        /// An update is installing: queued turns wait for it, and nothing starts meanwhile.
        Update = "update",
        /// The last turn stopped at the plan's limit; the queue waits for the lawyer.
        Limit = "limit",
        /// The queue waits for the lawyer: the last turn was stopped, failed, or the app started
        /// again.
        Paused = "paused",
        _ =>
        /// A reason from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod queue_hold_reason with [
        QueueHoldReason::SignIn,
        QueueHoldReason::Switching,
        QueueHoldReason::Update,
        QueueHoldReason::Limit,
        QueueHoldReason::Paused,
    ];
}

#[cfg(test)]
mod tests {
    use super::QueueHoldReason;
    use crate::ThreadSummary;

    /// A `ListThreads` row as JSON, with `extra` fields after the required ones.
    fn row(extra: &str) -> ThreadSummary {
        let json = format!(
            r#"{{"thread":"01900000-0000-7000-8000-000000000001","matter":"01900000-0000-7000-8000-000000000002","agent":"claude-acp","status":"idle"{extra}}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn a_row_reads_its_reason_none_when_missing_and_a_newer_one_as_unknown() {
        let old = row("");
        assert_eq!(old.queue_held_reason, None);
        let written = serde_json::to_value(&old).unwrap();
        assert!(
            written.get("queue_held_reason").is_none(),
            "None is left out"
        );
        let held = row(r#","queue_held":true,"queue_held_reason":"limit""#);
        assert_eq!(held.queue_held_reason, Some(QueueHoldReason::Limit));
        let newer = row(r#","queue_held":true,"queue_held_reason":"maintenance""#);
        assert!(matches!(
            newer.queue_held_reason,
            Some(QueueHoldReason::Unknown { .. })
        ));
        let back = serde_json::to_value(&newer).unwrap();
        assert_eq!(
            back["queue_held_reason"], "maintenance",
            "written back as it came"
        );
    }
}
