//! When this computer's sandbox set-up finished (`ProtectionView.set_up_at`, R17 PR-E): the
//! `set_up_at` of the set-up's marker, `<data_dir>\sandbox\setup.json` (the sandbox's
//! `windows::marker`), read only for a set-up that is done, off the actor where the
//! protection is detected (`observe`).
//!
//! The marker names the user who set up; a done set-up (`SetupState::Ready`) is this user's, so
//! the record's owner is the user it is read for. Anything that can't be read gives no time:
//! the view just leaves it out. Off Windows there is no set-up and no marker.

use std::path::Path;

use clauseo_protocol::Timestamp;
#[cfg(any(windows, test))]
use clauseo_sandbox::windows::marker::MarkerState;

/// The time the set-up's marker in `data_dir` holds, if a current one can be read.
#[cfg(windows)]
pub(super) fn read(data_dir: &Path) -> Option<Timestamp> {
    use clauseo_sandbox::windows::sids::Sid;
    let owner = clauseo_sandbox::windows::sandbox_owner_sid().ok()??;
    let owner = Sid::parse(&owner).ok()?;
    of(clauseo_sandbox::windows::marker::read(data_dir, &owner))
}

/// Off Windows no set-up writes a marker.
#[cfg(not(windows))]
pub(super) const fn read(_data_dir: &Path) -> Option<Timestamp> {
    None
}

/// The marker's time (seconds since the Unix epoch) as the wire's milliseconds: a current
/// marker's only.
#[cfg(any(windows, test))]
pub(super) fn of(marker: MarkerState) -> Option<Timestamp> {
    let MarkerState::Current(marker) = marker else {
        return None;
    };
    let millis = i64::try_from(marker.set_up_at).ok()?.checked_mul(1000)?;
    Timestamp::new(millis).ok()
}

#[cfg(test)]
mod tests {
    use clauseo_sandbox::windows::marker::{self, Marker, MarkerState};
    use clauseo_sandbox::windows::sids::Sid;

    use super::of;

    fn sid(text: &str) -> Sid {
        Sid::parse(text).unwrap()
    }

    /// A marker the set-up wrote, as `marker::read` reads it back, gives its time in ms; any
    /// other state, none.
    #[test]
    fn a_current_marker_gives_its_time_and_nothing_else_does() {
        let dir = crate::pipeline::tests::support::TempDir::new("set-up-at");
        let user = sid("S-1-5-21-1-2-3-1001");
        let written = Marker {
            version: marker::SETUP_VERSION,
            user_sid: user.clone(),
            group_sid: sid("S-1-5-21-1-2-3-1100"),
            pool: vec![sid("S-1-5-21-1-2-3-1002")],
            set_up_at: 1_790_000_000,
        };
        marker::write(dir.path(), &written).unwrap();
        let read = marker::read(dir.path(), &user);
        assert_eq!(
            of(read).map(clauseo_protocol::Timestamp::as_millis),
            Some(1_790_000_000_000)
        );
        assert_eq!(of(MarkerState::Missing), None);
        assert_eq!(of(MarkerState::Older { found: 1 }), None);
        let other = marker::read(dir.path(), &sid("S-1-5-21-1-2-3-1009"));
        assert!(matches!(other, MarkerState::OtherUser { .. }), "{other:?}");
        assert_eq!(of(other), None);
        let mut far = written;
        far.set_up_at = u64::MAX;
        assert_eq!(of(MarkerState::Current(far)), None, "out of range");
    }
}
