//! How set-up reads Windows' answers for the preflight, the logon and the profile (S15 `PR4f`),
//! apart from the calls, so the reading is tested on every OS. The calls are in `preflight.rs`,
//! `logon.rs` and `profile.rs` (Windows only).

use std::path::Path;

use super::machine::{LogonRight, MachineError, ServiceStart};
use crate::windows::sids::Sid;

/// `SERVICE_BOOT_START`: a driver the boot loader starts.
pub const SERVICE_BOOT_START: u32 = 0;
/// `SERVICE_SYSTEM_START`: a driver `IoInitSystem` starts.
pub const SERVICE_SYSTEM_START: u32 = 1;
/// `SERVICE_AUTO_START`: started with Windows.
pub const SERVICE_AUTO_START: u32 = 2;
/// `SERVICE_DEMAND_START`: started when asked.
pub const SERVICE_DEMAND_START: u32 = 3;
/// `SERVICE_DISABLED`: never started.
pub const SERVICE_DISABLED: u32 = 4;

/// The `ProfileList\<sid>` `State` flag `PROFILE_TEMP_ASSIGNED`: Windows gave the logon a
/// temporary profile, which it deletes at sign-out.
pub const PROFILE_TEMP_ASSIGNED: u32 = 0x0800;

/// The SIDs whose entry in "Deny log on locally" (`SeDenyInteractiveLogonRight`) refuses every
/// pool user, since each pool user's interactive token holds them all: Everyone, INTERACTIVE,
/// Authenticated Users, Local account, and Users.
pub const DENIED_FOR_POOL: [&str; 5] = [
    "S-1-1-0",
    "S-1-5-4",
    "S-1-5-11",
    "S-1-5-113",
    "S-1-5-32-545",
];

/// How a service with start type `start_type` (`QUERY_SERVICE_CONFIGW::dwStartType`) starts;
/// `None` for a value Windows doesn't define.
pub fn service_start(start_type: u32) -> Option<ServiceStart> {
    match start_type {
        SERVICE_BOOT_START | SERVICE_SYSTEM_START | SERVICE_AUTO_START => {
            Some(ServiceStart::Automatic)
        }
        SERVICE_DEMAND_START => Some(ServiceStart::Manual),
        SERVICE_DISABLED => Some(ServiceStart::Disabled),
        _ => None,
    }
}

/// Whether "Deny log on locally", whose holders are `denied`, refuses the pool users: it does
/// when it names any SID of [`DENIED_FOR_POOL`]. SIDs are compared exactly.
pub fn logon_right_of(denied: &[Sid]) -> LogonRight {
    let refused = denied
        .iter()
        .any(|sid| DENIED_FOR_POOL.contains(&sid.as_str()));
    if refused {
        LogonRight::Refused
    } else {
        LogonRight::Allowed
    }
}

/// Whether a profile is temporary: its `ProfileList` `State` has [`PROFILE_TEMP_ASSIGNED`],
/// or its folder is `TEMP` or `TEMP.<anything>` (Windows' temporary profile folders, such as
/// `C:\Users\TEMP` and `C:\Users\TEMP.PC.000`), compared without case.
pub fn is_temporary(state: Option<u32>, path: &Path) -> bool {
    if state.is_some_and(|state| state & PROFILE_TEMP_ASSIGNED != 0) {
        return true;
    }
    // Split by hand on both separators, so a Windows path reads the same on every OS.
    let text = path.to_string_lossy();
    let Some(leaf) = text.split(['\\', '/']).rfind(|part| !part.is_empty()) else {
        return false;
    };
    let leaf = leaf.to_ascii_uppercase();
    leaf == "TEMP" || leaf.starts_with("TEMP.")
}

/// The error a failed `LogonUserW` gives, `code` being its `GetLastError`. The engine names
/// `ERROR_LOGON_TYPE_NOT_GRANTED` (1385) as "Allow log on locally doesn't include the sandbox
/// users" (D-B4); the error carries only the call and the code, never the password.
pub fn logon_error(code: u32) -> MachineError {
    MachineError::Os {
        call: "LogonUserW",
        code,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::windows::setup::outcome::cause_of;
    use crate::windows::setup::{ERROR_LOGON_TYPE_NOT_GRANTED, LOGON_NOT_GRANTED};

    fn sid(text: &str) -> Sid {
        Sid::parse(text).unwrap()
    }

    #[test]
    fn start_types_map_to_how_the_service_starts() {
        assert_eq!(service_start(0), Some(ServiceStart::Automatic));
        assert_eq!(service_start(1), Some(ServiceStart::Automatic));
        assert_eq!(service_start(2), Some(ServiceStart::Automatic));
        assert_eq!(service_start(3), Some(ServiceStart::Manual));
        assert_eq!(service_start(4), Some(ServiceStart::Disabled));
        assert_eq!(service_start(5), None);
        assert_eq!(service_start(u32::MAX), None);
    }

    #[test]
    fn deny_log_on_locally_refuses_the_pool_only_for_groups_it_holds() {
        assert_eq!(logon_right_of(&[]), LogonRight::Allowed);
        // Guests and a domain user are not in a pool user's token.
        let others = [sid("S-1-5-32-546"), sid("S-1-5-21-1-2-3-1001")];
        assert_eq!(logon_right_of(&others), LogonRight::Allowed);
        for text in DENIED_FOR_POOL {
            assert_eq!(logon_right_of(&[sid(text)]), LogonRight::Refused, "{text}");
        }
        // Exact comparison: a SID that only starts like Users is not Users.
        assert_eq!(logon_right_of(&[sid("S-1-5-32-5450")]), LogonRight::Allowed);
        assert_eq!(
            logon_right_of(&[others[0].clone(), sid("S-1-5-32-545")]),
            LogonRight::Refused
        );
    }

    #[test]
    fn a_temporary_profile_is_read_from_its_state_or_its_folder() {
        let own = PathBuf::from(r"C:\Users\ClauseoSb01");
        assert!(!is_temporary(None, &own));
        assert!(!is_temporary(Some(0x0004), &own));
        assert!(is_temporary(Some(PROFILE_TEMP_ASSIGNED), &own));
        assert!(is_temporary(Some(0x0804), &own));
        assert!(is_temporary(None, &PathBuf::from(r"C:\Users\TEMP")));
        assert!(is_temporary(None, &PathBuf::from(r"C:\Users\temp\")));
        assert!(is_temporary(None, &PathBuf::from(r"C:\Users\TEMP.PC.000")));
        assert!(is_temporary(None, &PathBuf::from("/Users/TEMP")));
        assert!(!is_temporary(None, &PathBuf::from(r"C:\Users\Temperance")));
        assert!(!is_temporary(None, &PathBuf::from(r"C:\Users\TEMPLATE")));
        assert!(!is_temporary(None, &PathBuf::from(r"C:\TEMP\ClauseoSb01")));
        assert!(!is_temporary(None, &PathBuf::new()));
    }

    #[test]
    fn logon_error_1385_names_allow_log_on_locally() {
        let error = logon_error(ERROR_LOGON_TYPE_NOT_GRANTED);
        assert_eq!(cause_of(&error), LOGON_NOT_GRANTED);
        assert!(LOGON_NOT_GRANTED.contains("Allow log on locally"));
        // Any other code names the call and the code, and nothing else.
        let other = logon_error(1326);
        assert_eq!(cause_of(&other), "LogonUserW failed (error 1326)");
        assert_eq!(
            format!("{other:?}"),
            r#"Os { call: "LogonUserW", code: 1326 }"#
        );
    }
}
