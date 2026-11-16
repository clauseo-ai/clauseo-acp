//! The rollback rule and the time it compares.

use clauseo_blocklist::{Generated, accept};

use crate::fixtures::generated;

#[test]
fn an_older_or_equal_list_never_replaces_the_one_in_use() {
    let (old, now, new) = (
        generated("2026-11-06T00:00:00Z"),
        generated("2026-11-07T06:00:00Z"),
        generated("2026-11-07T12:00:00Z"),
    );
    assert!(accept(new, Some(now), None));
    assert!(!accept(old, Some(now), None));
    assert!(!accept(now, Some(now), None));
    assert!(accept(old, None, None));
}

#[test]
fn a_list_older_than_the_bundled_snapshot_is_refused_even_with_none_in_use() {
    let (old, bundled) = (
        generated("2026-11-06T00:00:00Z"),
        generated("2026-11-07T06:00:00Z"),
    );
    assert!(!accept(old, None, Some(bundled)));
    assert!(accept(bundled, None, Some(bundled)));
}

#[test]
fn generated_reads_only_the_exact_utc_form() {
    let time = generated("2028-02-29T23:59:59Z");
    assert_eq!(time.date(), "2028-02-29");
    assert_eq!(generated("1970-01-01T00:00:00Z").unix_seconds(), 0);
    assert_eq!(
        generated("2026-11-07T05:31:39Z").unix_seconds(),
        1_794_029_499
    );
    for bad in [
        "2026-11-07T06:00:00",
        "2026-11-07T06:00:00.5Z",
        "2026-11-07T06:00:00+00:00",
        "2026-11-07t06:00:00z",
        "2027-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-11-31T00:00:00Z",
        "2026-11-07T24:00:00Z",
        "2026-11-07T06:60:00Z",
        " 2026-11-07T06:00:00Z",
    ] {
        assert_eq!(Generated::parse(bad), None, "{bad}");
    }
}
