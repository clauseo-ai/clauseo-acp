//! The set-up marker, `<data_dir>\sandbox\setup.json`: which set-up version ran, for which
//! Windows user, which `ClauseoSandbox` account it made, and when. Set-up (S15 PR4) writes it
//! last; [`read`] tells missing, older, current and another user's apart, and [`state_of`] adds
//! the machine's `UserSid` record to give the [`SetupState`].
//!
//! Built on every OS so its tests run everywhere; only Windows uses it.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::sids::{SANDBOX_DIR, Sid};
use super::write;
use crate::{SetupError, SetupState};

/// The marker's file, in the sandbox folder.
pub const SETUP_FILE: &str = "setup.json";
/// The set-up version this build makes. A marker below it means set-up must run again.
pub const SETUP_VERSION: u32 = 1;

/// What `setup.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// The set-up version that wrote it.
    pub version: u32,
    /// The Windows user who ran the set-up and owns it (D5).
    pub user_sid: Sid,
    /// The `ClauseoSandbox` account the set-up made.
    pub sandbox_sid: Sid,
    /// When the set-up finished, in seconds since the Unix epoch (set-up passes the time in).
    pub set_up_at: u64,
}

/// What [`read`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerState {
    /// No `setup.json`: never set up here.
    Missing,
    /// Written by an older set-up, for this user.
    Older {
        /// Its version.
        found: u32,
    },
    /// Written by this set-up version, for this user.
    Current(Marker),
    /// Written for another Windows user, at any version.
    OtherUser {
        /// The user it names.
        user_sid: Sid,
    },
    /// Written by a newer set-up, for this user; this build can't vouch for it.
    Newer {
        /// Its version.
        found: u32,
    },
    /// Not readable, or not a marker; why.
    Unreadable(String),
}

/// The fields every version of the marker has, read before the version decides the rest.
#[derive(Deserialize)]
struct Head {
    version: u32,
    user_sid: Sid,
}

/// The marker's path under `data_dir`.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(SANDBOX_DIR).join(SETUP_FILE)
}

/// Reads `<data_dir>\sandbox\setup.json` for the Windows user `user`. The user is checked
/// before the version, so another user's marker is [`MarkerState::OtherUser`] whatever its
/// version; then a lower version is [`MarkerState::Older`], a higher one
/// [`MarkerState::Newer`], and this version must parse whole to be [`MarkerState::Current`].
pub fn read(data_dir: &Path, user: &Sid) -> MarkerState {
    let bytes = match std::fs::read(path(data_dir)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return MarkerState::Missing,
        Err(error) => return MarkerState::Unreadable(error.to_string()),
    };
    let head: Head = match serde_json::from_slice(&bytes) {
        Ok(head) => head,
        Err(error) => return MarkerState::Unreadable(error.to_string()),
    };
    if head.user_sid != *user {
        return MarkerState::OtherUser {
            user_sid: head.user_sid,
        };
    }
    match head.version {
        found if found < SETUP_VERSION => MarkerState::Older { found },
        found if found > SETUP_VERSION => MarkerState::Newer { found },
        _ => match serde_json::from_slice(&bytes) {
            Ok(marker) => MarkerState::Current(marker),
            Err(error) => MarkerState::Unreadable(error.to_string()),
        },
    }
}

/// Writes `marker` as `<data_dir>\sandbox\setup.json`, atomically, making the folder if
/// missing. Set-up's last step (S15 PR4).
pub fn write(data_dir: &Path, marker: &Marker) -> Result<(), SetupError> {
    let file = path(data_dir);
    let failed = |why: String| SetupError::Failed(format!("{}: {why}", file.display()));
    let mut bytes = serde_json::to_vec_pretty(marker).map_err(|error| failed(error.to_string()))?;
    bytes.push(b'\n');
    if let Some(folder) = file.parent() {
        write::make_dirs(folder).map_err(|error| failed(error.to_string()))?;
    }
    write::write_atomic(&file, &bytes).map_err(|error| failed(error.to_string()))
}

/// The set-up state from the marker and the machine's `UserSid` record (`record`, read only for
/// a current marker: `Ok(None)` when there is none, `Err` with why it can't be read).
///
/// - Missing: [`SetupState::Needed`]. A record without this user's marker is left to set-up
///   (S15 PR4) to judge: it may be this user's own set-up that stopped before the marker.
/// - Older: [`SetupState::Outdated`]. Another user's: [`SetupState::SetUpByOtherUser`] (D5).
/// - Newer or unreadable: [`SetupState::Failed`].
/// - Current: [`SetupState::Ready`] only when the record names the account the marker does
///   (canonical strings, so equal strings are equal SIDs). No record: the account is gone,
///   [`SetupState::Needed`]. Another account: it was made again by someone else, whose set-up
///   reset the password this user holds, so [`SetupState::SetUpByOtherUser`]. A record that
///   can't be read or is not a SID: [`SetupState::Failed`].
pub fn state_of(
    marker: MarkerState,
    record: impl FnOnce() -> Result<Option<String>, String>,
) -> SetupState {
    let marker = match marker {
        MarkerState::Missing => return SetupState::Needed,
        MarkerState::Older { found } => {
            return SetupState::Outdated {
                found,
                current: SETUP_VERSION,
            };
        }
        MarkerState::OtherUser { .. } => return SetupState::SetUpByOtherUser,
        MarkerState::Newer { found } => {
            return SetupState::Failed(format!(
                "{SETUP_FILE} is from a newer Clauseo (version {found}, this one sets up \
                 {SETUP_VERSION})"
            ));
        }
        MarkerState::Unreadable(why) => return SetupState::Failed(format!("{SETUP_FILE}: {why}")),
        MarkerState::Current(marker) => marker,
    };
    match record() {
        Ok(None) => SetupState::Needed,
        Ok(Some(text)) => match Sid::parse(&text) {
            Ok(sid) if sid == marker.sandbox_sid => SetupState::Ready,
            Ok(_) => SetupState::SetUpByOtherUser,
            Err(error) => SetupState::Failed(format!("the sandbox user record: {error}")),
        },
        Err(why) => SetupState::Failed(format!("the sandbox user record: {why}")),
    }
}

/// This Windows user's set-up state on this machine: `setup.json` under `data_dir`, read for
/// this process's user, and `HKLM\SOFTWARE\Clauseo\Sandbox\UserSid`. What S15 PR4's
/// `UserSetup::state` returns; reads only, every call.
#[cfg(windows)]
pub fn setup_state(data_dir: &Path) -> SetupState {
    let user = match super::record::current_user_sid() {
        Ok(user) => user,
        Err(error) => return SetupState::Failed(format!("this user's SID: {error}")),
    };
    state_of(read(data_dir, &user), || {
        super::record::sandbox_user_sid().map_err(|error| error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    const LAWYER: &str = "S-1-5-21-111-222-333-1001";
    const COLLEAGUE: &str = "S-1-5-21-111-222-333-1002";
    const SANDBOX: &str = "S-1-5-21-111-222-333-1003";

    fn sid(text: &str) -> Sid {
        Sid::parse(text).unwrap()
    }

    fn marker(version: u32, user: &str) -> Marker {
        Marker {
            version,
            user_sid: sid(user),
            sandbox_sid: sid(SANDBOX),
            set_up_at: 1_793_000_000,
        }
    }

    #[test]
    fn a_missing_marker_is_missing() {
        let temp = TempDir::new("marker-missing");
        assert_eq!(read(temp.path(), &sid(LAWYER)), MarkerState::Missing);
    }

    #[test]
    fn a_marker_written_then_read_is_current() {
        let temp = TempDir::new("marker-current");
        let written = marker(SETUP_VERSION, LAWYER);
        write(temp.path(), &written).unwrap();
        assert_eq!(
            read(temp.path(), &sid(LAWYER)),
            MarkerState::Current(written)
        );
        let names: Vec<String> = std::fs::read_dir(temp.path().join(SANDBOX_DIR))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [SETUP_FILE]);
    }

    #[test]
    fn an_older_marker_is_older() {
        let temp = TempDir::new("marker-older");
        let file = format!("{SANDBOX_DIR}/{SETUP_FILE}");
        // An older version need not have today's fields; only the version and user are read.
        temp.file(&file, &format!(r#"{{"version":0,"user_sid":"{LAWYER}"}}"#));
        assert_eq!(
            read(temp.path(), &sid(LAWYER)),
            MarkerState::Older { found: 0 }
        );
    }

    #[test]
    fn another_users_marker_is_other_user_at_any_version() {
        let temp = TempDir::new("marker-other");
        write(temp.path(), &marker(SETUP_VERSION, COLLEAGUE)).unwrap();
        let other = MarkerState::OtherUser {
            user_sid: sid(COLLEAGUE),
        };
        assert_eq!(read(temp.path(), &sid(LAWYER)), other);
        write(temp.path(), &marker(SETUP_VERSION + 1, COLLEAGUE)).unwrap();
        assert_eq!(read(temp.path(), &sid(LAWYER)), other);
    }

    #[test]
    fn a_newer_or_broken_marker_is_not_current() {
        let temp = TempDir::new("marker-broken");
        write(temp.path(), &marker(SETUP_VERSION + 1, LAWYER)).unwrap();
        assert_eq!(
            read(temp.path(), &sid(LAWYER)),
            MarkerState::Newer {
                found: SETUP_VERSION + 1
            }
        );
        let file = format!("{SANDBOX_DIR}/{SETUP_FILE}");
        for broken in [
            "not json".to_owned(),
            r#"{"version":1,"user_sid":"BA"}"#.to_owned(),
            format!(r#"{{"version":1,"user_sid":"{LAWYER}"}}"#),
            format!(
                r#"{{"version":1,"user_sid":"{LAWYER}","sandbox_sid":"{SANDBOX}","set_up_at":1,
                "extra":true}}"#
            ),
        ] {
            temp.file(&file, &broken);
            let found = read(temp.path(), &sid(LAWYER));
            assert!(
                matches!(found, MarkerState::Unreadable(_)),
                "{broken}: {found:?}"
            );
        }
    }

    #[test]
    fn the_state_needs_a_current_marker_and_a_matching_record() {
        let current = || MarkerState::Current(marker(SETUP_VERSION, LAWYER));
        let record = |text: &'static str| move || Ok(Some(text.to_owned()));
        assert_eq!(state_of(current(), record(SANDBOX)), SetupState::Ready);
        assert_eq!(state_of(current(), || Ok(None)), SetupState::Needed);
        assert_eq!(
            state_of(current(), record("S-1-5-21-9-9-9-1003")),
            SetupState::SetUpByOtherUser
        );
        assert!(matches!(
            state_of(current(), record("not a sid")),
            SetupState::Failed(_)
        ));
        assert!(matches!(
            state_of(current(), || Err("denied".to_owned())),
            SetupState::Failed(why) if why.contains("denied")
        ));
    }

    #[test]
    fn every_other_marker_state_ignores_the_record() {
        let unasked = || -> Result<Option<String>, String> { panic!("the record was read") };
        assert_eq!(state_of(MarkerState::Missing, unasked), SetupState::Needed);
        assert_eq!(
            state_of(MarkerState::Older { found: 0 }, unasked),
            SetupState::Outdated {
                found: 0,
                current: SETUP_VERSION
            }
        );
        let other = MarkerState::OtherUser {
            user_sid: sid(COLLEAGUE),
        };
        assert_eq!(state_of(other, unasked), SetupState::SetUpByOtherUser);
        assert!(matches!(
            state_of(MarkerState::Newer { found: 9 }, unasked),
            SetupState::Failed(_)
        ));
        assert!(matches!(
            state_of(MarkerState::Unreadable("bad".to_owned()), unasked),
            SetupState::Failed(_)
        ));
    }
}
