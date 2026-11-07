//! The runner pipes' names and DACL.

use super::*;

#[test]
fn a_random_set_has_four_distinct_names_under_one_base() {
    let set = PipeSet::random().unwrap();
    assert_eq!(PipeSet::parse(set.base()).unwrap(), set);
    let names: Vec<String> = Role::ALL.iter().map(|role| set.name(*role)).collect();
    for (name, suffix) in names.iter().zip(["ctl", "in", "out", "err"]) {
        assert_eq!(*name, format!(r"\\.\pipe\{}-{suffix}", set.base()));
    }
    assert_ne!(PipeSet::random().unwrap(), set, "128 random bits each");
}

#[test]
fn only_the_exact_base_form_parses() {
    let good = format!("clauseo-run-{}", "0123456789abcdef".repeat(2));
    assert!(PipeSet::parse(&good).is_ok());
    for bad in [
        "clauseo-run-".to_owned(),
        format!("clauseo-run-{}", "0123456789ABCDEF".repeat(2)),
        format!("clauseo-run-{}0", "0123456789abcdef".repeat(2)),
        format!("other-{}", "0123456789abcdef".repeat(2)),
        format!(r"clauseo-run-{}\x", "0123456789abcdef".repeat(2)),
    ] {
        assert!(PipeSet::parse(&bad).is_err(), "{bad}");
    }
}

#[test]
fn the_dacl_names_the_desktop_user_and_the_pool_user_only() {
    let desktop = Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    let pool = Sid::parse("S-1-5-21-1-2-3-1005").unwrap();
    let sddl = pipe_sddl(&desktop, &pool);
    assert_eq!(
        sddl,
        "D:P(A;;0x12019f;;;S-1-5-21-1-2-3-1001)(A;;0x12019f;;;S-1-5-21-1-2-3-1005)\
         S:(ML;;NWNR;;;ME)"
    );
    // Protected, read and write only, and no broad group, logon SID or owner rights.
    for absent in [
        ";GA;", ";WD)", ";BU)", ";AU)", "S-1-5-5-", ";OW)", "WDAC", "0x1f01ff",
    ] {
        assert!(!sddl.contains(absent), "{absent} in {sddl}");
    }
    assert_eq!(
        PIPE_RIGHTS,
        0x0012_0089 | 0x0012_0116,
        "FILE_GENERIC_READ | FILE_GENERIC_WRITE"
    );
}
