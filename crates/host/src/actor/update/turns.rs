//! What the machine hears about turns after an input: `TurnsRunning` when "any turn running"
//! flipped, and `TurnsCancelled` once a `CancelTurns` has no turn left without its `TurnEnded`,
//! whether or not any turn was running when it was asked.

/// Where a `CancelTurns` stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cancel {
    /// None waits for its answer.
    NotAsked,
    /// One waits, and a turn has not ended durably (`turn::any_active`).
    TurnsLeft,
    /// One waits, and every turn has ended.
    AllEnded,
}

/// What the actor sees after an input.
#[derive(Debug, Clone, Copy)]
pub(super) struct Seen {
    /// The last `TurnsRunning` the machine heard.
    pub(super) heard_running: bool,
    /// Whether any turn runs now (`turn::any_running`).
    pub(super) running: bool,
    /// Where a `CancelTurns` stands.
    pub(super) cancel: Cancel,
}

/// What the machine is to hear, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Heard {
    /// `TurnsRunning(running)`, when it flipped.
    pub(super) running: Option<bool>,
    /// `TurnsCancelled`.
    pub(super) cancelled: bool,
}

/// What the machine hears given what the actor sees.
pub(super) fn to_hear(seen: Seen) -> Heard {
    Heard {
        running: (seen.running != seen.heard_running).then_some(seen.running),
        cancelled: seen.cancel == Cancel::AllEnded,
    }
}

#[cfg(test)]
mod tests {
    use super::{Cancel, Heard, Seen, to_hear};

    fn seen(heard_running: bool, running: bool, cancel: Cancel) -> Seen {
        Seen {
            heard_running,
            running,
            cancel,
        }
    }

    #[test]
    fn running_is_heard_only_when_it_flips() {
        let none = Heard {
            running: None,
            cancelled: false,
        };
        assert_eq!(to_hear(seen(false, false, Cancel::NotAsked)), none);
        assert_eq!(to_hear(seen(true, true, Cancel::NotAsked)), none);
        assert_eq!(
            to_hear(seen(false, true, Cancel::NotAsked)).running,
            Some(true)
        );
        assert_eq!(
            to_hear(seen(true, false, Cancel::NotAsked)).running,
            Some(false)
        );
    }

    #[test]
    fn a_cancel_with_no_turn_running_is_answered_at_once() {
        // Host thread, #434: `CancelTurns` is always answered, else a `now` install would hold
        // new sends for ever.
        assert_eq!(
            to_hear(seen(false, false, Cancel::AllEnded)),
            Heard {
                running: None,
                cancelled: true,
            }
        );
    }

    #[test]
    fn a_cancel_is_answered_once_the_last_turn_has_ended() {
        assert!(!to_hear(seen(true, true, Cancel::TurnsLeft)).cancelled);
        assert_eq!(
            to_hear(seen(true, false, Cancel::AllEnded)),
            Heard {
                running: Some(false),
                cancelled: true,
            }
        );
    }
}
