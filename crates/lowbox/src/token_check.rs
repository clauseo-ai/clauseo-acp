//! What the guard's own token and the new `LowBox` token must be, as plain facts the Windows
//! side reads from each token (`windows::token`), so the rules are tested on every OS.

use crate::error::Error;

/// `SE_CHANGE_NOTIFY_PRIVILEGE`'s LUID: the one privilege the `LowBox` token keeps.
pub(crate) const CHANGE_NOTIFY: u64 = 23;
/// The Low mandatory level's RID (`SECURITY_MANDATORY_LOW_RID`).
pub(crate) const LOW_RID: u32 = 0x1000;
/// `S-1-5-32-544`, the local Administrators group.
pub(crate) const ADMINISTRATORS: &str = "S-1-5-32-544";

/// What the checks read from a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    /// `TokenElevation`'s `TokenIsElevated`.
    pub(crate) elevated: bool,
    /// `TokenIsAppContainer`.
    pub(crate) app_container: bool,
    /// `TokenAppContainerSid`'s text, when it has one.
    pub(crate) package: Option<String>,
    /// The mandatory label's RID (`TokenIntegrityLevel`).
    pub(crate) integrity: u32,
    /// `TokenCapabilities`' group count.
    pub(crate) capabilities: usize,
    /// The LUIDs of `TokenPrivileges`.
    pub(crate) privileges: Vec<u64>,
    /// The enabled groups' SIDs (`TokenGroups` with `SE_GROUP_ENABLED`).
    pub(crate) enabled_groups: Vec<String>,
}

/// The guard's own token, before a `LowBox` token is made over it: not elevated, the
/// Administrators group not enabled, not already an `AppContainer`.
pub(crate) fn check_base(facts: &Facts) -> Result<(), Error> {
    let refuse = |reason| Err(Error::Token { reason });
    if facts.elevated {
        return refuse("the guard's token is elevated");
    }
    if facts.enabled_groups.iter().any(|g| g == ADMINISTRATORS) {
        return refuse("the guard's token has the Administrators group enabled");
    }
    if facts.app_container {
        return refuse("the guard's token is already an AppContainer token");
    }
    Ok(())
}

/// The `LowBox` token for the package whose SID text is `package`: an `AppContainer` token of
/// that package, at Low, with no capability and no privilege but `SeChangeNotifyPrivilege`.
pub(crate) fn check_lowbox(facts: &Facts, package: &str) -> Result<(), Error> {
    let refuse = |reason| Err(Error::Token { reason });
    if !facts.app_container {
        return refuse("the LowBox token is not an AppContainer token");
    }
    if facts.package.as_deref() != Some(package) {
        return refuse("the LowBox token is not this package's");
    }
    if facts.integrity != LOW_RID {
        return refuse("the LowBox token is not at the Low integrity level");
    }
    if facts.capabilities != 0 {
        return refuse("the LowBox token has a capability");
    }
    if facts.privileges.iter().any(|p| *p != CHANGE_NOTIFY) {
        return refuse("the LowBox token keeps a privilege other than SeChangeNotifyPrivilege");
    }
    if facts.elevated {
        return refuse("the LowBox token is elevated");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE: &str = "S-1-15-2-1-2-3-4-5-6-7";

    fn base() -> Facts {
        Facts {
            elevated: false,
            app_container: false,
            package: None,
            integrity: 0x2000,
            capabilities: 0,
            // SeShutdown, SeChangeNotify, SeUndock, SeIncreaseWorkingSet, SeTimeZone.
            privileges: vec![19, 23, 25, 33, 34],
            enabled_groups: vec!["S-1-1-0".to_owned(), "S-1-5-11".to_owned()],
        }
    }

    fn lowbox() -> Facts {
        Facts {
            app_container: true,
            package: Some(PACKAGE.to_owned()),
            integrity: LOW_RID,
            privileges: vec![CHANGE_NOTIFY],
            ..base()
        }
    }

    #[test]
    fn a_standard_users_token_is_a_good_base() {
        assert_eq!(check_base(&base()), Ok(()));
    }

    #[test]
    fn an_elevated_admin_or_appcontainer_base_is_refused() {
        let elevated = Facts {
            elevated: true,
            ..base()
        };
        let mut admin = base();
        admin.enabled_groups.push(ADMINISTRATORS.to_owned());
        let boxed = Facts {
            app_container: true,
            ..base()
        };
        for facts in [elevated, admin, boxed] {
            assert!(
                matches!(check_base(&facts), Err(Error::Token { .. })),
                "{facts:?}"
            );
        }
    }

    #[test]
    fn the_lowbox_token_passes_with_change_notify_or_nothing() {
        assert_eq!(check_lowbox(&lowbox(), PACKAGE), Ok(()));
        let bare = Facts {
            privileges: Vec::new(),
            ..lowbox()
        };
        assert_eq!(check_lowbox(&bare, PACKAGE), Ok(()));
    }

    #[test]
    fn each_wrong_fact_of_the_lowbox_token_is_refused() {
        let wrong = [
            Facts {
                app_container: false,
                ..lowbox()
            },
            Facts {
                package: Some("S-1-15-2-1-2-3-4-5-6-8".to_owned()),
                ..lowbox()
            },
            Facts {
                package: None,
                ..lowbox()
            },
            Facts {
                integrity: 0x2000,
                ..lowbox()
            },
            Facts {
                integrity: 0,
                ..lowbox()
            },
            Facts {
                capabilities: 1,
                ..lowbox()
            },
            Facts {
                privileges: vec![CHANGE_NOTIFY, 19],
                ..lowbox()
            },
            Facts {
                elevated: true,
                ..lowbox()
            },
        ];
        for facts in wrong {
            assert!(
                matches!(check_lowbox(&facts, PACKAGE), Err(Error::Token { .. })),
                "{facts:?}"
            );
        }
    }
}
