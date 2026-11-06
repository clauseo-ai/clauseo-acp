use super::*;
use crate::test_support::TempDir;

const LAWYER: &str = "S-1-5-21-111-222-333-1001";
const COLLEAGUE: &str = "S-1-5-21-111-222-333-1002";
const GROUP: &str = "S-1-5-21-111-222-333-1003";
const SB01: &str = "S-1-5-21-111-222-333-1004";
const SB02: &str = "S-1-5-21-111-222-333-1005";
const SB03: &str = "S-1-5-21-111-222-333-1006";

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// A marker of `version` for `user`, naming `GROUP` and the pool `SB01`, `SB02`.
fn marker(version: u32, user: &str) -> Marker {
    Marker {
        version,
        user_sid: sid(user),
        group_sid: sid(GROUP),
        pool: vec![sid(SB01), sid(SB02)],
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
fn a_version_1_marker_is_outdated() {
    // Version 1's shape: one `sandbox_sid`, no group or pool.
    let temp = TempDir::new("marker-v1");
    let file = format!("{SANDBOX_DIR}/{SETUP_FILE}");
    temp.file(
        &file,
        &format!(r#"{{"version":1,"user_sid":"{LAWYER}","sandbox_sid":"{SB01}","set_up_at":1}}"#),
    );
    let lawyer = sid(LAWYER);
    let found = read(temp.path(), &lawyer);
    assert_eq!(found, MarkerState::Older { found: 1 });
    let records = Fake::ready();
    assert_eq!(
        state_of(found, &lawyer, &records),
        SetupState::Outdated {
            found: 1,
            current: 2
        }
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
    let fields = format!(r#""user_sid":"{LAWYER}","group_sid":"{GROUP}","set_up_at":1"#);
    for broken in [
        "not json".to_owned(),
        r#"{"version":2,"user_sid":"BA"}"#.to_owned(),
        format!(r#"{{"version":2,"user_sid":"{LAWYER}"}}"#),
        // Version 1's single account under version 2.
        format!(r#"{{"version":2,{fields},"sandbox_sid":"{SB01}"}}"#),
        format!(r#"{{"version":2,{fields},"pool":["BA"]}}"#),
        format!(r#"{{"version":2,{fields},"pool":["{SB01}"],"extra":true}}"#),
    ] {
        temp.file(&file, &broken);
        let found = read(temp.path(), &sid(LAWYER));
        assert!(
            matches!(found, MarkerState::Unreadable(_)),
            "{broken}: {found:?}"
        );
    }
    // The same fields with a pool parse.
    temp.file(
        &file,
        &format!(r#"{{"version":2,{fields},"pool":["{SB01}"]}}"#),
    );
    assert!(matches!(
        read(temp.path(), &sid(LAWYER)),
        MarkerState::Current(_)
    ));
}

/// Records with set answers.
#[derive(Clone)]
struct Fake {
    owner: Result<Option<String>, String>,
    group: Result<Option<String>, String>,
    pool: Result<Vec<Sid>, String>,
    removing: Result<bool, String>,
}

impl Fake {
    /// The records a finished set-up of `LAWYER` leaves.
    fn ready() -> Fake {
        Fake {
            owner: Ok(Some(LAWYER.to_owned())),
            group: Ok(Some(GROUP.to_owned())),
            pool: Ok(vec![sid(SB01), sid(SB02)]),
            removing: Ok(false),
        }
    }
}

impl Records for Fake {
    fn owner(&self) -> Result<Option<String>, String> {
        self.owner.clone()
    }

    fn group(&self) -> Result<Option<String>, String> {
        self.group.clone()
    }

    fn pool(&self) -> Result<Vec<Sid>, String> {
        self.pool.clone()
    }

    fn removing(&self) -> Result<bool, String> {
        self.removing.clone()
    }
}

/// The marker states [`read`] can give, by name.
fn marker_states() -> Vec<(&'static str, MarkerState)> {
    vec![
        ("missing", MarkerState::Missing),
        ("v1", MarkerState::Older { found: 1 }),
        (
            "current",
            MarkerState::Current(marker(SETUP_VERSION, LAWYER)),
        ),
        (
            "other",
            MarkerState::OtherUser {
                user_sid: sid(COLLEAGUE),
            },
        ),
        ("newer", MarkerState::Newer { found: 3 }),
        ("unreadable", MarkerState::Unreadable("bad".to_owned())),
    ]
}

/// The `OwnerSid` answers, by name.
fn owners() -> Vec<(&'static str, Result<Option<String>, String>)> {
    vec![
        ("me", Ok(Some(LAWYER.to_owned()))),
        ("none", Ok(None)),
        ("colleague", Ok(Some(COLLEAGUE.to_owned()))),
        ("alias", Ok(Some("BA".to_owned()))),
        ("denied", Err("denied".to_owned())),
    ]
}

/// The `PoolSids` answers, by name.
fn pools() -> Vec<(&'static str, Result<Vec<Sid>, String>)> {
    vec![
        ("same", Ok(vec![sid(SB01), sid(SB02)])),
        ("reordered", Ok(vec![sid(SB02), sid(SB01)])),
        ("extra", Ok(vec![sid(SB01), sid(SB02), sid(SB03)])),
        ("missing one", Ok(vec![sid(SB01)])),
        ("empty", Ok(vec![])),
        ("malformed", Err("malformed".to_owned())),
    ]
}

/// A state as a short name, `Failed` with the start of its cause.
fn show(state: &SetupState) -> String {
    match state {
        SetupState::Failed(why) => format!("failed: {why}"),
        SetupState::Ready
        | SetupState::Needed
        | SetupState::Refused
        | SetupState::Outdated { .. }
        | SetupState::SetUpByOtherUser => format!("{state:?}"),
    }
}

#[test]
fn the_state_over_every_marker_owner_and_pool() {
    let lawyer = sid(LAWYER);
    for (marker_name, marker_state) in marker_states() {
        for (owner_name, owner) in owners() {
            for (pool_name, pool) in pools() {
                let records = Fake {
                    owner: owner.clone(),
                    pool: pool.clone(),
                    ..Fake::ready()
                };
                let state = state_of(marker_state.clone(), &lawyer, &records);
                let expected = match (owner_name, marker_name, pool_name) {
                    ("alias" | "denied", ..) => "failed: the sandbox owner record",
                    ("colleague", ..) | (_, "other", _) => "SetUpByOtherUser",
                    (_, "v1", _) => "Outdated",
                    (_, "newer", _) => "failed: setup.json is from a newer Clauseo",
                    (_, "unreadable", _) => "failed: setup.json: bad",
                    (_, "current", "malformed") => "failed: the sandbox pool record",
                    ("me", "current", "same" | "reordered" | "extra") => "Ready",
                    (_, "missing", _)
                    | ("none", "current", _)
                    | (_, "current", "missing one" | "empty") => "Needed",
                    unlisted => panic!("no row for {unlisted:?}"),
                };
                assert!(
                    show(&state).starts_with(expected),
                    "{marker_name} × owner {owner_name} × pool {pool_name}: {state:?}"
                );
            }
        }
    }
}

#[test]
fn a_set_removing_fails_every_state_but_another_owners() {
    let lawyer = sid(LAWYER);
    for (marker_name, marker_state) in marker_states() {
        for (owner_name, owner) in owners() {
            let records = Fake {
                owner: owner.clone(),
                removing: Ok(true),
                ..Fake::ready()
            };
            let state = state_of(marker_state.clone(), &lawyer, &records);
            match owner_name {
                "colleague" => assert_eq!(state, SetupState::SetUpByOtherUser),
                "alias" | "denied" => assert!(
                    show(&state).starts_with("failed: the sandbox owner record"),
                    "{state:?}"
                ),
                _ => assert_eq!(
                    state,
                    SetupState::Failed(
                        "removal unfinished: reinstall and uninstall, or run set-up again"
                            .to_owned()
                    ),
                    "{marker_name} × owner {owner_name}"
                ),
            }
        }
    }
    let unreadable = Fake {
        removing: Err("denied".to_owned()),
        ..Fake::ready()
    };
    let current = MarkerState::Current(marker(SETUP_VERSION, LAWYER));
    assert!(matches!(
        state_of(current, &lawyer, &unreadable),
        SetupState::Failed(why) if why.contains("removal") && why.contains("denied")
    ));
}

#[test]
fn a_current_marker_needs_the_recorded_group() {
    let lawyer = sid(LAWYER);
    let current = || MarkerState::Current(marker(SETUP_VERSION, LAWYER));
    let with_group = |group: Result<Option<&str>, &str>| Fake {
        group: group
            .map(|sid| sid.map(str::to_owned))
            .map_err(str::to_owned),
        ..Fake::ready()
    };
    assert_eq!(
        state_of(current(), &lawyer, &with_group(Ok(Some(GROUP)))),
        SetupState::Ready
    );
    for gone in [Ok(None), Ok(Some(SB03))] {
        assert_eq!(
            state_of(current(), &lawyer, &with_group(gone)),
            SetupState::Needed,
            "{gone:?}"
        );
    }
    for broken in [Ok(Some("BA")), Err("denied")] {
        assert!(matches!(
            state_of(current(), &lawyer, &with_group(broken)),
            SetupState::Failed(why) if why.contains("group")
        ));
    }
}

#[test]
fn a_current_marker_with_no_pool_makes_no_sense() {
    let lawyer = sid(LAWYER);
    let empty = Marker {
        pool: vec![],
        ..marker(SETUP_VERSION, LAWYER)
    };
    assert!(matches!(
        state_of(MarkerState::Current(empty), &lawyer, &Fake::ready()),
        SetupState::Failed(why) if why.contains("no pool user")
    ));
}

#[cfg(unix)]
#[test]
fn a_hard_linked_marker_is_unreadable_and_not_written() {
    let temp = TempDir::new("marker-hard-link");
    write(temp.path(), &marker(SETUP_VERSION, LAWYER)).unwrap();
    let outside = temp.hard_link("outside.json", &path(temp.path()));
    assert!(matches!(
        read(temp.path(), &sid(LAWYER)),
        MarkerState::Unreadable(why) if why.contains("hard links")
    ));
    assert!(write(temp.path(), &marker(SETUP_VERSION, LAWYER)).is_err());
    drop(outside);
}
