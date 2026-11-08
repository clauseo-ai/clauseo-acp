//! What the set-up and the refresh record of the targets in the machine record, and the `AppID`
//! GUIDs they name. Pure; tested on every OS.
//!
//! Two `REG_MULTI_SZ` values, appended to the set-up's (`record_values::SETUP_VALUES`), so a
//! removal or a rollback deletes them with the rest: `WslDenyAppIds`, every `AppID` a deny was
//! written to or found on, and `WslDenyAbsent`, those of them whose `LaunchPermission` we made
//! because it was missing. Each is braced and upper-case (`{370121D2-AA7E-...}`); a list that
//! holds anything else is refused whole, so no text from the record ever becomes a key path
//! that isn't an `AppID`'s.

use super::seam::WslError;

/// An `AppID`: a GUID in braces, upper-case, checked digit by digit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppId(String);

impl AppId {
    /// `text` as an `AppID`: `{8-4-4-4-12}` hex digits, any case, braces required; `None` for
    /// anything else.
    pub fn parse(text: &str) -> Option<AppId> {
        let inner = text.strip_prefix('{')?.strip_suffix('}')?;
        let groups: Vec<&str> = inner.split('-').collect();
        let lengths: Vec<usize> = groups.iter().map(|group| group.len()).collect();
        let hex = groups
            .iter()
            .all(|group| group.bytes().all(|b| b.is_ascii_hexdigit()));
        (lengths == [8, 4, 4, 4, 12] && hex).then(|| AppId(text.to_ascii_uppercase()))
    }

    /// Its text: `{...}`, upper-case.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AppId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `WslDenyAppIds`.
pub const RECORD_APP_IDS: &str = "WslDenyAppIds";
/// `WslDenyAbsent`.
pub const RECORD_ABSENT: &str = "WslDenyAbsent";

/// The targets as recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WslRecord {
    /// Every `AppID` a deny was written to or found on, in the order first recorded.
    pub app_ids: Vec<AppId>,
    /// Those whose `LaunchPermission` we made because it was missing.
    pub absent: Vec<AppId>,
}

impl WslRecord {
    /// Adds `app_id`, and to [`WslRecord::absent`] when `absent`, each where it is missing;
    /// whether anything changed. Nothing is ever taken off but by a removal.
    pub fn add(&mut self, app_id: &AppId, absent: bool) -> bool {
        let mut changed = false;
        if !self.app_ids.contains(app_id) {
            self.app_ids.push(app_id.clone());
            changed = true;
        }
        if absent && !self.absent.contains(app_id) {
            self.absent.push(app_id.clone());
            changed = true;
        }
        changed
    }

    /// The record `app_ids` and `absent` hold, as read (each `REG_MULTI_SZ`'s units; `None`
    /// when missing). Anything but braced GUIDs refuses the whole record.
    pub fn decode(app_ids: Option<&[u16]>, absent: Option<&[u16]>) -> Result<WslRecord, WslError> {
        Ok(WslRecord {
            app_ids: list(app_ids, RECORD_APP_IDS)?,
            absent: list(absent, RECORD_ABSENT)?,
        })
    }

    /// The two values' units, `WslDenyAppIds` then `WslDenyAbsent`, as `REG_MULTI_SZ` data.
    pub fn encode(&self) -> (Vec<u16>, Vec<u16>) {
        (units(&self.app_ids), units(&self.absent))
    }
}

/// `ids` as `REG_MULTI_SZ` data: each one and a NUL, then one more NUL.
fn units(ids: &[AppId]) -> Vec<u16> {
    let mut out: Vec<u16> = ids
        .iter()
        .flat_map(|id| id.as_str().encode_utf16().chain([0]))
        .collect();
    out.push(0);
    out
}

/// The `AppID`s a `REG_MULTI_SZ`'s `units` list, for the value `name`.
fn list(units: Option<&[u16]>, name: &str) -> Result<Vec<AppId>, WslError> {
    let Some(units) = units else {
        return Ok(Vec::new());
    };
    let text = String::from_utf16(units)
        .map_err(|_not_text| WslError(format!("the record's {name} isn't text")))?;
    let mut ids = Vec::new();
    for part in text.split('\0').filter(|part| !part.is_empty()) {
        let id = AppId::parse(part)
            .ok_or_else(|| WslError(format!("the record's {name} holds something not an AppID")))?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_id_is_a_braced_guid_kept_upper_case() {
        let id = AppId::parse("{370121d2-aa7e-4608-a86d-0bbab9da1a60}").unwrap();
        assert_eq!(id.as_str(), "{370121D2-AA7E-4608-A86D-0BBAB9DA1A60}");
        for bad in [
            "370121D2-AA7E-4608-A86D-0BBAB9DA1A60",
            "{370121D2-AA7E-4608-A86D-0BBAB9DA1A6}",
            "{370121D2-AA7E-4608-A86D-0BBAB9DA1A6G}",
            "{370121D2-AA7E-4608-A86D-0BBAB9DA1A60}\\..",
            "{..\\..\\Ole}",
            "wsl.exe",
            "",
        ] {
            assert_eq!(AppId::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_record_round_trips_and_refuses_anything_else_whole() {
        let a = AppId::parse("{370121D2-AA7E-4608-A86D-0BBAB9DA1A60}").unwrap();
        let b = AppId::parse("{E9B79997-57E3-4201-AECC-6A464E530DD2}").unwrap();
        let mut record = WslRecord::default();
        assert!(record.add(&a, false));
        assert!(record.add(&b, true));
        assert!(!record.add(&b, true), "added once");
        assert!(!record.add(&a, false));
        assert!(record.add(&a, true), "absent is added on its own");
        let (ids, absent) = record.encode();
        assert_eq!(ids.last(), Some(&0));
        assert_eq!(WslRecord::decode(Some(&ids), Some(&absent)), Ok(record));
        assert_eq!(WslRecord::decode(None, None), Ok(WslRecord::default()));
        assert_eq!(
            WslRecord::decode(Some(&[0]), None),
            Ok(WslRecord::default())
        );
        let odd: Vec<u16> = "{370121D2-AA7E-4608-A86D-0BBAB9DA1A60}\0..\\Ole\0\0"
            .encode_utf16()
            .collect();
        assert!(WslRecord::decode(Some(&odd), None).is_err());
        assert_eq!(
            (RECORD_APP_IDS, RECORD_ABSENT),
            ("WslDenyAppIds", "WslDenyAbsent")
        );
    }
}
