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
        rights
            .calls
            .iter()
            .all(|call| call.starts_with("policy") || call.starts_with("holders")),
        "{:?}",
        rights.calls
    );
}

/// A policy definition without the pool, as a stale or losing Intune provider key gives.
fn excluded(holders: Vec<Sid>) -> FakeRights {
    FakeRights {
        policy: RightPolicy::Defined {
            includes_pool: false,
        },
        holders,
        ..FakeRights::default()
    }
}

#[test]
fn a_definition_the_right_in_effect_overrides_grants_nothing_and_goes_on() {
    for holder in ["S-1-5-32-545", "S-1-5-21-5-5-5-1000"] {
        let mut rights = excluded(vec![sid("S-1-5-32-544"), sid(holder)]);
        assert_eq!(
            grant_interactive(&mut rights, &group(), &[]),
            Ok(Granted::PolicyNotInEffect),
            "{holder}"
        );
        assert_eq!(rights.recorded, None);
        assert!(!rights.calls.iter().any(|call| call.starts_with("add")));
    }
}

#[test]
fn a_definition_the_right_in_effect_agrees_with_blocks() {
    let mut rights = excluded(vec![sid("S-1-5-32-544")]);
    assert_eq!(
        grant_interactive(&mut rights, &group(), &[]),
        Err(GrantError::Excluded)
    );
    // Holders that can't be read leave the definition standing.
    let mut rights = excluded(vec![sid("S-1-5-32-545")]);
    rights.fail = Some(format!("holders {INTERACTIVE_RIGHT}"));
    assert_eq!(
        grant_interactive(&mut rights, &group(), &[]),
        Err(GrantError::Excluded)
    );
    assert!(!rights.calls.iter().any(|call| call.starts_with("add")));
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
    assert_eq!(remove_grant(&mut rights, &group()), Ok(false));
    assert_eq!(rights.holders, [group()]);
    assert_eq!(rights.calls, ["recorded"]);

    // Recorded: only that SID loses it, and the record goes.
    let users = sid("S-1-5-32-545");
    let mut rights = FakeRights {
        holders: vec![users.clone(), group()],
        recorded: Some(group()),
        ..FakeRights::default()
    };
    assert_eq!(remove_grant(&mut rights, &group()), Ok(true));
    assert_eq!(rights.holders, [users]);
    assert_eq!(rights.recorded, None);

    // A failed removal keeps the record, for the next run.
    let mut rights = FakeRights {
        recorded: Some(group()),
        fail: Some(format!("remove {}", group())),
        ..FakeRights::default()
    };
    assert!(remove_grant(&mut rights, &group()).is_err());
    assert_eq!(rights.recorded, Some(group()));
}

#[test]
fn remove_leaves_a_recorded_sid_that_isnt_the_group() {
    let other = sid("S-1-5-21-5-5-5-2000");
    let mut rights = FakeRights {
        holders: vec![other.clone()],
        recorded: Some(other.clone()),
        ..FakeRights::default()
    };
    assert_eq!(remove_grant(&mut rights, &group()), Ok(false));
    assert_eq!(rights.holders, [other]);
    assert!(!rights.calls.iter().any(|call| call.starts_with("remove")));
}

#[test]
fn the_record_is_written_before_the_grant() {
    let mut rights = admins_only();
    // A crash right after the record leaves a value to remove, never an unrecorded grant.
    rights.fail = Some(format!("add {}", group()));
    assert!(add_recorded(&mut rights, &group()).is_err());
    let at = |prefix: &str| {
        rights
            .calls
            .iter()
            .position(|c| c.starts_with(prefix))
            .unwrap()
    };
    assert!(at("record S-") < at("add"));
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

/// The code files the workspace scan reads: every language that could call the LSA, add a group
/// member or write a policy (Rust, TypeScript and JavaScript, the NSIS scripts, PowerShell, shell,
/// and the config Clauseo ships).
const SCANNED: [&str; 12] = [
    "rs", "ts", "tsx", "js", "mjs", "nsh", "nsi", "ps1", "psm1", "sh", "toml", "json",
];

#[test]
fn no_code_path_grants_the_remote_desktop_right() {
    assert_eq!(GRANTED_RIGHTS, [INTERACTIVE_RIGHT]);
    assert!(!GRANTED_RIGHTS.contains(&rdp_right().as_str()));
    // No code in the workspace names the right, its Intune name, or the built-in Remote Desktop
    // group (by name or SID), which would let the pool in by membership instead.
    let needles = [
        rdp_right(),
        ["AllowLogOn", "ThroughRemoteDesktop"].concat(),
        ["S-1-5-32-", "555"].concat(),
        ["Remote Desktop", " Users"].concat(),
        ["RemoteDesktop", "Users"].concat(),
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut stack = vec![root.clone()];
    let mut read = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                let skipped = ["target", "node_modules", "dist"].contains(&name.as_str())
                    || (name.starts_with('.') && name != ".github");
                if !skipped {
                    stack.push(entry.path());
                }
                continue;
            }
            let path = entry.path();
            let code = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| SCANNED.contains(&ext));
            if !kind.is_file() || !code {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            read += 1;
            let lower = text.to_lowercase();
            for needle in &needles {
                assert!(
                    !lower.contains(&needle.to_lowercase()),
                    "{}: {needle}",
                    path.display()
                );
            }
        }
    }
    assert!(
        read > 3000,
        "the scan read {read} files under {}",
        root.display()
    );
}
