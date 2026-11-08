//! The gate on who asked ([`identify`]), each rule refusing on its own; the pipe's DACL
//! ([`pipe_sddl`]); and the app's check of who answers ([`check_server`]).

use std::path::Path;

use super::*;

/// A change to a client's facts or the record.
type Change = fn(&mut ClientFacts, &mut Gate);

/// The lawyer's facts, changed by `change`, through a per-machine gate with the lawyer signed in.
fn gated(change: impl FnOnce(&mut ClientFacts, &mut Gate)) -> Result<Sid, Refusal> {
    let mut facts = client(LAWYER);
    let mut gate = machine_gate();
    change(&mut facts, &mut gate);
    identify(&facts, Some(&sid(LAWYER)), &gate)
}

#[test]
fn the_session_user_passes_and_each_rule_refuses_on_its_own() {
    assert_eq!(gated(|_, _| {}), Ok(sid(LAWYER)));
    // High integrity (an elevated app) passes: its owner is lowered after.
    assert_eq!(gated(|facts, _| facts.integrity = 0x3000), Ok(sid(LAWYER)));
    let cases: [(&str, Change, Refusal); 8] = [
        (
            "no admin copy",
            |_, gate| gate.copy_owner = None,
            Refusal::NoAdminCopy,
        ),
        (
            "pool SID",
            |_, gate| gate.pool.push(sid(LAWYER)),
            Refusal::PoolUser,
        ),
        (
            "pool group",
            |facts, _| facts.groups.push(sid(GROUP)),
            Refusal::PoolGroup,
        ),
        (
            "session 0",
            |facts, _| facts.session = 0,
            Refusal::SessionZero,
        ),
        (
            "restricted",
            |facts, _| facts.restricted = true,
            Refusal::Restricted,
        ),
        (
            "Low",
            |facts, _| facts.integrity = 0x1000,
            Refusal::LowIntegrity,
        ),
        (
            "untrusted",
            |facts, _| facts.integrity = 0,
            Refusal::LowIntegrity,
        ),
        (
            "per user, another's install",
            |_, gate| gate.copy_owner = Some(CopyOwner::User(sid(OTHER))),
            Refusal::NotTheOwner,
        ),
    ];
    for (case, change, refusal) in cases {
        assert_eq!(gated(change), Err(refusal), "{case}");
    }
    // Per user, the owner itself passes.
    assert_eq!(
        gated(|_, gate| gate.copy_owner = Some(CopyOwner::User(sid(LAWYER)))),
        Ok(sid(LAWYER))
    );
}

#[test]
fn a_client_that_isnt_its_sessions_user_is_refused() {
    let gate = machine_gate();
    let facts = client(OTHER);
    assert_eq!(
        identify(&facts, Some(&sid(LAWYER)), &gate),
        Err(Refusal::NotSessionUser)
    );
    assert_eq!(identify(&facts, None, &gate), Err(Refusal::NoSessionUser));
}

#[test]
fn service_system_and_virtual_accounts_are_refused() {
    for user in [
        "S-1-5-18",
        "S-1-5-19",
        "S-1-5-20",
        "S-1-5-80-1-2-3-4-5",
        "S-1-5-90-0-2",
        "S-1-5-96-0-1",
        "S-1-5-32-544",
        "S-1-5-21",
    ] {
        let facts = client(user);
        let refused = identify(&facts, Some(&sid(user)), &machine_gate());
        assert_eq!(refused, Err(Refusal::NotAnAccount), "{user}");
    }
}

#[test]
fn the_pool_rules_hold_whatever_else_is_true() {
    // No descriptor or DACL is an input: a pool user that is its own session's user, with every
    // other fact a lawyer's, on a per-machine install, is refused; so is the group itself as a
    // user, a group SID the record has but PoolSids doesn't, and a deny-only membership.
    let gate = machine_gate();
    let pool = client(POOL);
    assert_eq!(
        identify(&pool, Some(&sid(POOL)), &gate),
        Err(Refusal::PoolUser)
    );
    let as_group = client(GROUP);
    assert_eq!(
        identify(&as_group, Some(&sid(GROUP)), &gate),
        Err(Refusal::PoolGroup)
    );
    let mut named = machine_gate();
    named.groups.push(sid("S-1-5-21-1-2-3-1060"));
    let mut member = client(LAWYER);
    member.groups.push(sid("S-1-5-21-1-2-3-1060"));
    assert_eq!(
        identify(&member, Some(&sid(LAWYER)), &named),
        Err(Refusal::PoolGroup)
    );
    // The pool rules come first: a pool user in session 0, or not its session's user, is still
    // refused as a pool user, so a log line never hides one.
    let mut session_zero = client(POOL);
    session_zero.session = 0;
    assert_eq!(identify(&session_zero, None, &gate), Err(Refusal::PoolUser));
    assert_eq!(
        identify(&pool, Some(&sid(LAWYER)), &gate),
        Err(Refusal::PoolUser)
    );
    for refusal in [
        Refusal::PoolUser,
        Refusal::PoolGroup,
        Refusal::NotSessionUser,
    ] {
        assert!(!refusal.to_string().contains("S-1-"), "{refusal}");
    }
}

#[test]
fn the_pipe_names_the_owner_or_interactive_users_and_refuses_the_pool_first() {
    let per_machine = pipe_sddl(&machine_gate()).unwrap();
    assert_eq!(
        per_machine,
        format!("D:P(D;;GA;;;{GROUP})(A;;GA;;;SY)(A;;0x12019b;;;IU)S:(ML;;NWNR;;;ME)")
    );
    let mut per_user = machine_gate();
    per_user.copy_owner = Some(CopyOwner::User(sid(LAWYER)));
    per_user.groups.clear();
    assert_eq!(
        pipe_sddl(&per_user).unwrap(),
        format!("D:P(A;;GA;;;SY)(A;;0x12019b;;;{LAWYER})S:(ML;;NWNR;;;ME)")
    );
    // A client's rights never include a new instance (FILE_CREATE_PIPE_INSTANCE, 0x4).
    assert_eq!(crate::windows::pipe::PIPE_RIGHTS & 0x4, 0);
    assert_eq!(pipe_sddl(&Gate::default()), None);
}

#[test]
fn the_app_talks_only_to_system_in_session_0_running_the_admin_copy() {
    let exe = Path::new(ADMIN_EXE);
    let device = r"\Device\HarddiskVolume3";
    let real = ServerFacts {
        session: 0,
        user: Some(sid("S-1-5-18")),
        image: Some(
            r"\Device\HarddiskVolume3\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe"
                .to_owned(),
        ),
    };
    assert_eq!(check_server(&real, exe, device), Ok(()));
    let upper = ServerFacts {
        image: real.image.as_ref().map(|image| image.to_uppercase()),
        ..real.clone()
    };
    assert_eq!(check_server(&upper, exe, device), Ok(()), "case folded");
    let squats = [
        // A pool user's process in its own session.
        ServerFacts {
            session: 1,
            user: Some(sid(POOL)),
            ..real.clone()
        },
        // A pool user's scheduled task reaches session 0, but isn't SYSTEM.
        ServerFacts {
            user: Some(sid(POOL)),
            ..real.clone()
        },
        ServerFacts {
            user: None,
            ..real.clone()
        },
        // SYSTEM running anything else, or on another volume.
        ServerFacts {
            image: Some(r"\Device\HarddiskVolume3\Windows\System32\cmd.exe".to_owned()),
            ..real.clone()
        },
        ServerFacts {
            image: Some(
                r"\Device\HarddiskVolume4\Program Files\Clauseo\Sandbox\clauseo-sandbox-setup.exe"
                    .to_owned(),
            ),
            ..real.clone()
        },
        ServerFacts {
            image: None,
            ..real
        },
    ];
    for squat in squats {
        assert!(check_server(&squat, exe, device).is_err(), "{squat:?}");
    }
}
