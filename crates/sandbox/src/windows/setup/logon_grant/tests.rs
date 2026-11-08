use std::path::Path;

use super::fake::FakeRights;
use super::*;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

fn group() -> Sid {
    sid("S-1-5-21-5-5-5-1000")
}

/// A machine whose local policy gives the right only to Administrators.
fn admins_only() -> FakeRights {
    FakeRights {
        holders: vec![sid("S-1-5-32-544")],
        ..FakeRights::default()
    }
}

#[test]
fn not_defined_and_not_covered_is_recorded_then_granted() {
    let mut rights = admins_only();
    let granted = grant_interactive(&mut rights, &group(), &[]);
    assert_eq!(granted, Ok(Granted::Added));
    assert_eq!(rights.recorded, Some(group()));
    assert!(rights.holders.contains(&group()));
    let group = group();
    assert_eq!(
        rights.calls,
        [
            format!("policy {INTERACTIVE_RIGHT}"),
            format!("holders {INTERACTIVE_RIGHT}"),
            format!("record {group}"),
            format!("add {group}"),
        ]
    );
}

#[test]
fn a_local_holder_that_admits_the_pool_needs_no_grant() {
    for holder in ["S-1-5-32-545", "S-1-1-0", "S-1-5-11", "S-1-5-21-5-5-5-1000"] {
        let mut rights = FakeRights {
            holders: vec![sid("S-1-5-32-544"), sid(holder)],
            ..FakeRights::default()
        };
        assert_eq!(
            grant_interactive(&mut rights, &group(), &[]),
            Ok(Granted::Covered),
            "{holder}"
        );
        assert_eq!(rights.recorded, None, "{holder}");
        assert!(!rights.calls.iter().any(|call| call.starts_with("add")));
    }
}

#[test]
fn a_policy_that_defines_the_right_is_never_granted_over() {
    let mut rights = FakeRights {
        policy: RightPolicy::Defined {
            includes_pool: true,
        },
        ..admins_only()
    };
    assert_eq!(
        grant_interactive(&mut rights, &group(), &[]),
        Ok(Granted::PolicyIncludes)
    );
    rights.policy = RightPolicy::Defined {
        includes_pool: false,
    };
    assert_eq!(
        grant_interactive(&mut rights, &group(), &[]),
        Err(GrantError::Excluded)
    );
    rights.policy = RightPolicy::Unknown;
    assert_eq!(
        grant_interactive(&mut rights, &group(), &[]),
        Ok(Granted::Unknown)
    );
    assert_eq!(rights.recorded, None);
    assert!(
        rights.calls.iter().all(|call| call.starts_with("policy")),
        "{:?}",
        rights.calls
    );
}

#[test]
fn a_failed_grant_leaves_no_record_and_a_failed_record_no_grant() {
    let mut rights = admins_only();
    rights.fail = Some(format!("add {}", group()));
    assert!(matches!(
        grant_interactive(&mut rights, &group(), &[]),
        Err(GrantError::Failed(_))
    ));
    assert_eq!(rights.recorded, None);
    assert!(!rights.holders.contains(&group()));

    let mut rights = admins_only();
    rights.fail = Some(format!("record {}", group()));
    assert!(matches!(
        grant_interactive(&mut rights, &group(), &[]),
        Err(GrantError::Failed(_))
    ));
    assert!(!rights.calls.iter().any(|call| call.starts_with("add")));
}

#[test]
fn remove_takes_off_only_what_set_up_added() {
    // Nothing recorded: a right IT granted the group stays.
    let mut rights = FakeRights {
        holders: vec![group()],
        ..admins_only()
    };
    assert_eq!(remove_grant(&mut rights), Ok(false));
    assert_eq!(rights.holders, [group()]);
    assert_eq!(rights.calls, ["recorded"]);

    // Recorded: only that SID loses it, and the record goes.
    let users = sid("S-1-5-32-545");
    let mut rights = FakeRights {
        holders: vec![users.clone(), group()],
        recorded: Some(group()),
        ..FakeRights::default()
    };
    assert_eq!(remove_grant(&mut rights), Ok(true));
    assert_eq!(rights.holders, [users]);
    assert_eq!(rights.recorded, None);

    // A failed removal keeps the record, for the next run.
    let mut rights = FakeRights {
        recorded: Some(group()),
        fail: Some(format!("remove {}", group())),
        ..FakeRights::default()
    };
    assert!(remove_grant(&mut rights).is_err());
    assert_eq!(rights.recorded, Some(group()));
}

#[test]
fn the_rollback_undoes_its_own_grant() {
    let mut rights = admins_only();
    grant_interactive(&mut rights, &group(), &[]).unwrap();
    assert_eq!(undo_grant(&mut rights, &group()), Ok(()));
    assert_eq!(rights.holders, [sid("S-1-5-32-544")]);
    assert_eq!(rights.recorded, None);
}

/// "Allow log on through Remote Desktop Services", spelled in parts so this file doesn't hold it.
fn rdp_right() -> String {
    ["SeRemote", "InteractiveLogonRight"].concat()
}

#[test]
fn no_code_path_grants_the_remote_desktop_right() {
    assert_eq!(GRANTED_RIGHTS, [INTERACTIVE_RIGHT]);
    assert!(!GRANTED_RIGHTS.contains(&rdp_right().as_str()));
    // No source in the crate names it, nor its Intune or group equivalents.
    let needles = [
        rdp_right(),
        ["AllowLogOn", "ThroughRemoteDesktop"].concat(),
        ["S-1-5-32-", "555"].concat(),
    ];
    let mut stack = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    let mut read = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                read += 1;
                for needle in &needles {
                    assert!(
                        !text.contains(needle.as_str()),
                        "{}: {needle}",
                        path.display()
                    );
                }
            }
        }
    }
    assert!(read > 100, "the scan read {read} files");
}
