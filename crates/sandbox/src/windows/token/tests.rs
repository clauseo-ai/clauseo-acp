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
fn a_service_or_malformed_desktop_user_is_refused() {
    // Well-known, service, virtual-account, logon and AppContainer SIDs, and an Entra SID with
    // the wrong part count (`S-1-12-1-a-b-c-d` is the only Entra user form).
    for not_user in [
        "S-1-5-7",
        "S-1-5-18",
        "S-1-5-82-3876422241-1344743576-1731187256-2652481255-2240555440",
        "S-1-5-83-1-1-2-3-4",
        "S-1-15-2-1-2-3-4-5-6-7",
        "S-1-15-3-1024-1-2-3-4-5-6-7",
        "S-1-12-1-1-2-3",
        "S-1-12-1-1-2-3-4-5",
    ] {
        let refused = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(not_user), None);
        assert!(
            matches!(
                refused,
                Err(TokenSpecError::NotAnAccount {
                    role: "the desktop user",
                    ..
                })
            ),
            "{not_user}: {refused:?}"
        );
    }
}

#[test]
fn the_built_in_administrator_is_an_accepted_desktop_user() {
    // RID 500 is the built-in Administrator, a real interactive user when UAC is off, so the
    // allow-list takes it. There is no RID >= 1000 rule; group RIDs such as 512 are not refused
    // here either (see `Sid::is_user_account`), since a group can't be a token's user.
    let administrator = "S-1-5-21-1111111111-2222222222-3333333333-500";
    let spec = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(administrator), None).unwrap();
    assert!(
        spec.default_dacl.contains(administrator),
        "{}",
        spec.default_dacl
    );
}

#[test]
fn an_entra_desktop_user_is_an_account_and_an_entra_pool_user_is_not() {
    let entra = "S-1-12-1-3263492174-1177263218-2391161522-1513380577";
    let spec = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(entra), None).unwrap();
    assert_eq!(
        spec.default_dacl,
        format!("D:(A;;GA;;;{POOL})(A;;GA;;;SY)(A;;0x101001;;;{entra})")
    );
    assert!(spec.token_dacl.contains(entra), "{}", spec.token_dacl);
    let refused = TokenSpec::for_pool(&sid(entra), &sid(LOGON), &sid(DESKTOP), None);
    assert!(
        matches!(
            refused,
            Err(TokenSpecError::NotAnAccount {
                role: "the pool user",
                ..
            })
        ),
        "{refused:?}"
    );
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
fn display_names_every_part() {
    let spec = TokenSpec::for_pool(&sid(POOL), &sid(LOGON), &sid(DESKTOP), None).unwrap();
    let text = spec.to_string();
    for part in [POOL, LOGON, "S-1-16-4096", "0x101001"] {
        assert!(text.contains(part), "{text}");
    }
}

#[test]
fn the_inner_runners_own_objects_name_no_pool_user() {
    let desktop = sid(DESKTOP);
    for sddl in [
        runner_process_sddl(&desktop),
        runner_thread_sddl(&desktop),
        runner_token_sddl(&desktop),
        runner_default_dacl(&desktop),
    ] {
        assert!(sddl.contains(DESKTOP) && sddl.contains(";;;SY)"), "{sddl}");
        for absent in [POOL, ";WD)", ";BU)", ";RC)"] {
            assert!(!sddl.contains(absent), "{absent} in {sddl}");
        }
    }
    // None names a logon SID: each runner's is its own logon's.
    for sddl in [
        runner_process_sddl(&desktop),
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
    // Before the launch arrives the inner's token adds to SYSTEM's entries TOKEN_QUERY (0x8) for
    // exactly the desktop user, nothing broader.
    assert_eq!(RUNNER_TOKEN_FIRST_SDDL, "D:P(A;;GA;;;SY)(A;;RC;;;OW)");
    assert_eq!(
        runner_token_first_sddl(&desktop),
        format!("D:P(A;;GA;;;SY)(A;;RC;;;OW)(A;;0x8;;;{desktop})")
    );
}

/// Every descriptor the outer or the inner sets on the inner runner (the one runner the outer
/// starts as the pool user), pinned whole.
#[test]
fn every_runner_descriptor_is_pinned() {
    let desktop = sid(DESKTOP);
    let pinned = [
        (
            runner_process_sddl(&desktop),
            format!("D:P(A;;GA;;;SY)(A;;0x101001;;;{DESKTOP})(A;;RC;;;OW)S:(ML;;NWNRNX;;;ME)"),
        ),
        (
            runner_thread_sddl(&desktop),
            format!("D:P(A;;GA;;;SY)(A;;0x100000;;;{DESKTOP})(A;;RC;;;OW)"),
        ),
        (
            runner_token_first_sddl(&desktop),
            format!("D:P(A;;GA;;;SY)(A;;RC;;;OW)(A;;0x8;;;{DESKTOP})"),
        ),
        (
            runner_token_sddl(&desktop),
            format!("D:P(A;;GA;;;SY)(A;;0x8;;;{DESKTOP})(A;;RC;;;OW)"),
        ),
        (
            runner_default_dacl(&desktop),
            format!("D:(A;;GA;;;SY)(A;;0x100000;;;{DESKTOP})(A;;RC;;;OW)"),
        ),
    ];
    for (sddl, want) in pinned {
        assert_eq!(sddl, want);
    }
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
fn the_runners_process_grants_the_desktop_user_alone_and_carries_a_medium_label() {
    // No pool user (the agent, and any other process of the pool user's, opens nothing of a
    // runner, and no runner opens another) and no logon SID (each runner's is its own logon's);
    // the owner held to READ_CONTROL; a Medium label refusing Low callers write, read and
    // execute.
    let sddl = runner_process_sddl(&sid(DESKTOP));
    assert_eq!(
        sddl,
        format!("D:P(A;;GA;;;SY)(A;;0x101001;;;{DESKTOP})(A;;RC;;;OW)S:(ML;;NWNRNX;;;ME)")
    );
    for absent in [POOL, LOGON, ";WD)", ";BU)", ";AU)"] {
        assert!(!sddl.contains(absent), "{absent} in {sddl}");
    }
}
