//! Why no pool user was free, in counts: what [`Error::PoolFull`](crate::Error::PoolFull)
//! carries, for the host's `NotProtectedCause::PoolFull`. Counts only, never an account name or
//! a SID, so it can reach a log and the app.

use super::assign::{HeldBack, Why};

/// The pool's users, counted by why each is held back ([`Why`], the first reason found).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PoolFullCause {
    /// The pool's users in all.
    pub size: u8,
    /// Held by a matter that had them and has a session open ([`Why::Open`]).
    pub open: u8,
    /// Held by a matter that still has them ([`Why::Held`]).
    pub held: u8,
    /// Owed a permission hand-back, or still granted by a ledger entry ([`Why::Owed`],
    /// [`Why::Entries`]): the next launch's reconcile clears them.
    pub owed: u8,
    /// Still the owner of items set aside ([`Why::SetAside`]).
    pub set_aside: u8,
    /// Owed a profile reset, which the elevated set-up does ([`Why::NeedsReset`]). The
    /// protocol's `PoolFull` has no field for it yet: the host counts it as `held`.
    pub needs_reset: u8,
}

impl PoolFullCause {
    /// The counts of `held_back` in a pool of `size` users: `assign`'s
    /// [`Assign::PoolFull`](super::assign::Assign::PoolFull). Each count stops at 255; a pool
    /// has at most [`POOL_CAP`](super::POOL_CAP) users.
    pub fn of(size: usize, held_back: &[HeldBack]) -> PoolFullCause {
        let mut cause = PoolFullCause {
            size: u8::try_from(size).unwrap_or(u8::MAX),
            ..PoolFullCause::default()
        };
        for each in held_back {
            let count = match each.why {
                Why::Open => &mut cause.open,
                Why::Held(_) => &mut cause.held,
                Why::Owed | Why::Entries => &mut cause.owed,
                Why::SetAside => &mut cause.set_aside,
                Why::NeedsReset => &mut cause.needs_reset,
            };
            *count = count.saturating_add(1);
        }
        cause
    }
}

impl std::fmt::Display for PoolFullCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "all {} of the Safe workspace's accounts are in use ({} by an open session, {} kept \
             by a matter, {} owed a hand-back, {} owning set-aside items, {} owed a reset)",
            self.size, self.open, self.held, self.owed, self.set_aside, self.needs_reset
        )
    }
}

#[cfg(test)]
mod tests {
    use clauseo_protocol::MatterId;

    use super::*;
    use crate::windows::sids::Sid;

    fn user(n: u32) -> Sid {
        Sid::parse(&format!(
            "S-1-5-21-1111111111-2222222222-3333333333-{}",
            1000 + n
        ))
        .unwrap()
    }

    fn matter(n: u8) -> MatterId {
        format!("01890a5d-ac96-774b-8cce-b302099a80{n:02}")
            .parse()
            .unwrap()
    }

    #[test]
    fn counts_each_reason_once() {
        let whys = [
            Why::Open,
            Why::Held(matter(1)),
            Why::Held(matter(2)),
            Why::Owed,
            Why::Entries,
            Why::SetAside,
            Why::NeedsReset,
        ];
        let held_back: Vec<HeldBack> = (0u32..)
            .zip(whys)
            .map(|(n, why)| HeldBack { pool: user(n), why })
            .collect();
        let cause = PoolFullCause::of(7, &held_back);
        assert_eq!(
            cause,
            PoolFullCause {
                size: 7,
                open: 1,
                held: 2,
                owed: 2,
                set_aside: 1,
                needs_reset: 1,
            }
        );
        let text = cause.to_string();
        assert!(text.starts_with("all 7 of"), "{text}");
        assert!(!text.contains("S-1-5"), "no SID: {text}");
    }
}
