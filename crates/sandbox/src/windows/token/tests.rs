//! The (B) token spec built from its inputs.

use super::*;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

const POOL: &str = "S-1-5-21-1111111111-2222222222-3333333333-1005";
const DESKTOP: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
const LOGON: &str = "S-1-5-5-0-123456";
const MATTER: &str = "S-1-5-21-444444444-555555555-666666666-1234";

#[test]
fn the_spec_is_design_b() {
    let spec = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(DESKTOP), None).unwrap();
    let restricting: Vec<&str> = spec.restricting.iter().map(Sid::as_str).collect();
    assert_eq!(restricting, [POOL, "S-1-5-12", "S-1-5-32-545", "S-1-1-0"]);
    assert_eq!(spec.deny_only, [sid(LOGON)]);
    assert_eq!(spec.integrity.as_str(), "S-1-16-4096");
    assert_eq!(
        spec.default_dacl,
        format!("D:(A;;GA;;;{POOL})(A;;GA;;;SY)(A;;0x101001;;;{DESKTOP})")
    );
}

#[test]
fn the_default_dacl_denies_the_pool_user_nothing() {
    // A console process with no console makes \Device\ConDrv\Server with GENERIC_ALL under this
    // DACL: a deny entry, or an OWNER RIGHTS entry, ends every agent at start with
    // STATUS_DLL_INIT_FAILED (0xC0000142).
    let spec = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(DESKTOP), None).unwrap();
    assert!(!spec.default_dacl.contains("(D;"), "{}", spec.default_dacl);
    assert!(
        !spec.default_dacl.contains(";;;OW)"),
        "{}",
        spec.default_dacl
    );
    assert!(
        spec.default_dacl
            .starts_with(&format!("D:(A;;GA;;;{POOL})"))
    );
}

#[test]
fn the_default_dacl_names_no_logon_sid_and_no_broad_group() {
    let spec = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(DESKTOP), None).unwrap();
    for absent in [
        LOGON,
        "S-1-1-0",
        "S-1-5-32-545",
        "S-1-5-12",
        ";WD)",
        ";BU)",
        ";AU)",
    ] {
        assert!(
            !spec.default_dacl.contains(absent),
            "{absent} in {}",
            spec.default_dacl
        );
    }
}

#[test]
fn the_desktop_user_gets_wait_query_and_terminate_only() {
    // SYNCHRONIZE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE.
    assert_eq!(DESKTOP_USER_RIGHTS, 0x0010_0000 | 0x1000 | 0x1);
}

#[test]
fn a_matter_sid_is_appended_last_when_asked() {
    let spec =
        TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(DESKTOP), Some(&sid(MATTER))).unwrap();
    assert_eq!(spec.restricting.len(), 5);
    assert_eq!(spec.restricting[4], sid(MATTER));
    assert!(!spec.default_dacl.contains(MATTER));
}

#[test]
fn the_pool_user_must_not_be_the_desktop_user() {
    assert_eq!(
        TokenSpec::for_pool(&sid(DESKTOP), &sid(LOGON), &sid(DESKTOP), None),
        Err(TokenSpecError::PoolIsDesktopUser)
    );
}

#[test]
fn well_known_sids_are_not_accounts() {
    for broad in ["S-1-1-0", "S-1-5-32-545", "S-1-5-18", "S-1-5-11", LOGON] {
        let refused = TokenSpec::for_pool(&sid(broad), &sid(LOGON), &sid(DESKTOP), None);
        assert!(
            matches!(
                refused,
                Err(TokenSpecError::NotAnAccount {
                    role: "the pool user",
                    ..
                })
            ),
            "{broad}: {refused:?}"
        );
        let refused = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(broad), None);
        assert!(
            matches!(
                refused,
                Err(TokenSpecError::NotAnAccount {
                    role: "the desktop user",
                    ..
                })
            ),
            "{broad}: {refused:?}"
        );
    }
}

#[test]
fn the_deny_only_sid_must_be_a_logon_sid() {
    for not_logon in [POOL, "S-1-5-5-1", "S-1-5-5-1-2-3", "S-1-5-18"] {
        assert_eq!(
            TokenSpec::for_pool(&sid(POOL), &sid(not_logon), &sid(DESKTOP), None),
            Err(TokenSpecError::NotALogonSid(sid(not_logon))),
        );
    }
    assert!(is_logon_sid(&sid("S-1-5-5-0-999")));
}

#[test]
fn the_private_desktop_is_low_and_shut_to_the_logon_sid() {
    let sddl = desktop_sddl(&sid(POOL), None);
    assert_eq!(
        sddl,
        format!("D:(A;;GA;;;SY)(A;;GA;;;{POOL})S:(ML;;NW;;;LW)")
    );
    assert!(!sddl.contains(DESKTOP));
    let with_matter = desktop_sddl(&sid(POOL), Some(&sid(MATTER)));
    assert!(
        with_matter.contains(&format!("(A;;GA;;;{MATTER})S:")),
        "{with_matter}"
    );
}

#[test]
fn display_names_every_part() {
    let spec = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(DESKTOP), None).unwrap();
    let text = spec.to_string();
    for part in [POOL, LOGON, "S-1-16-4096", "0x101001"] {
        assert!(text.contains(part), "{text}");
    }
}

#[test]
fn the_agents_window_station_names_only_system_and_the_pool_user_at_low() {
    let sddl = station_sddl(&sid(POOL), None);
    assert_eq!(
        sddl,
        format!("D:(A;;GA;;;SY)(A;;GA;;;{POOL})S:(ML;;NW;;;LW)")
    );
    for absent in [DESKTOP, LOGON, ";WD)", ";BU)"] {
        assert!(!sddl.contains(absent), "{absent} in {sddl}");
    }
}

#[test]
fn the_inner_runners_own_objects_name_no_pool_user() {
    let (desktop, logon) = (sid(DESKTOP), sid(LOGON));
    for sddl in [
        runner_process_sddl(&desktop, &logon),
        runner_thread_sddl(&desktop),
        runner_token_sddl(&desktop),
        runner_default_dacl(&desktop),
    ] {
        assert!(sddl.contains(DESKTOP) && sddl.contains(";;;SY)"), "{sddl}");
        for absent in [POOL, ";WD)", ";BU)", ";RC)"] {
            assert!(!sddl.contains(absent), "{absent} in {sddl}");
        }
    }
    // Only the process names the logon SID; the threads and the token don't.
    for sddl in [
        runner_thread_sddl(&desktop),
        runner_token_sddl(&desktop),
        runner_default_dacl(&desktop),
    ] {
        assert!(!sddl.contains(LOGON), "{sddl}");
    }
    assert!(runner_token_sddl(&desktop).ends_with("(A;;RC;;;OW)"));
    // Later threads get the threads' own entries: the owner held to READ_CONTROL, the desktop
    // user SYNCHRONIZE only.
    assert_eq!(
        runner_default_dacl(&desktop),
        runner_thread_sddl(&desktop).replacen("D:P", "D:", 1)
    );
    // Before the launch arrives the token names SYSTEM alone.
    assert_eq!(RUNNER_TOKEN_FIRST_SDDL, "D:P(A;;GA;;;SY)(A;;RC;;;OW)");
}

#[test]
fn the_end_sweepers_threads_and_later_objects_name_system_alone() {
    // No pool user and no desktop user (the sweeper doesn't know one); the owner held to
    // READ_CONTROL, so the pool user keeps no implied WRITE_DAC.
    assert_eq!(SWEEPER_THREAD_SDDL, "D:P(A;;GA;;;SY)(A;;RC;;;OW)");
    assert_eq!(
        SWEEPER_DEFAULT_DACL,
        SWEEPER_THREAD_SDDL.replacen("D:P", "D:", 1)
    );
}

#[test]
fn the_agents_own_token_names_the_pool_user() {
    let spec = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(DESKTOP), None).unwrap();
    assert_eq!(
        spec.token_dacl,
        format!("D:P(A;;0x20009;;;{POOL})(A;;GA;;;SY)(A;;0x8;;;{DESKTOP})(A;;RC;;;OW)")
    );
}

#[test]
fn the_pool_user_cant_change_the_agents_token() {
    // TOKEN_ADJUST_DEFAULT (0x80), TOKEN_ADJUST_GROUPS (0x40), TOKEN_ADJUST_PRIVILEGES (0x20),
    // TOKEN_DUPLICATE (0x2), TOKEN_IMPERSONATE (0x4), WRITE_DAC and WRITE_OWNER.
    for right in [0x80, 0x40, 0x20, 0x2, 0x4, 0x0004_0000, 0x0008_0000] {
        assert_eq!(AGENT_TOKEN_POOL_RIGHTS & right, 0, "0x{right:x}");
    }
    assert_eq!(AGENT_TOKEN_POOL_RIGHTS, 0x0002_0000 | 0x8 | 0x1);
}

#[test]
fn the_runners_process_grants_the_logon_sid_wait_query_and_end_and_the_pool_user_nothing() {
    // The pool user's sweeps reach the runners (the inner, the end sweeper) through the desktop
    // user's logon SID, which the agent holds deny-only.
    let (desktop, logon) = (sid(DESKTOP), sid(LOGON));
    let sddl = runner_process_sddl(&desktop, &logon);
    assert_eq!(
        sddl,
        format!("D:P(A;;GA;;;SY)(A;;0x101001;;;{DESKTOP})(A;;0x101001;;;{LOGON})(A;;RC;;;OW)")
    );
    // Every entry, SID by SID: no other SID gains anything, and the pool user isn't one.
    let entries: Vec<&str> = sddl
        .trim_start_matches("D:P(")
        .trim_end_matches(')')
        .split(")(")
        .collect();
    let want = [
        "A;;GA;;;SY".to_owned(),
        format!("A;;0x101001;;;{DESKTOP}"),
        format!("A;;0x{DESKTOP_USER_RIGHTS:x};;;{LOGON}"),
        "A;;RC;;;OW".to_owned(),
    ];
    assert_eq!(entries, want);
    assert!(!sddl.contains(POOL));
    // The agent's token holds that logon SID deny-only, so the allow never matches it.
    let spec = TokenSpec::for_pool(&sid(POOL), &logon, &desktop, None).unwrap();
    assert_eq!(spec.deny_only, [logon]);
}

#[test]
fn the_sweepers_com_token_dacl_adds_query_duplicate_and_impersonate_for_its_user() {
    assert_eq!(
        sweeper_com_token_sddl(&sid(POOL)),
        format!("D:P(A;;GA;;;SY)(A;;0xe;;;{POOL})(A;;RC;;;OW)")
    );
    // TOKEN_QUERY, TOKEN_DUPLICATE and TOKEN_IMPERSONATE alone: no TOKEN_ASSIGN_PRIMARY (0x1),
    // TOKEN_ADJUST_* (0x20, 0x40, 0x80, 0x100), READ_CONTROL or WRITE_DAC/WRITE_OWNER.
    assert_eq!(SWEEPER_COM_TOKEN_RIGHTS, 0x8 | 0x2 | 0x4);
}
