//! The stale rules as plain functions of their facts, on every OS: each row of each rule, and
//! each failed query counted as held.

use super::{Decision, MacFacts, WindowsFacts, mac_rule, windows_rule};

/// The facts of a stale owner file on macOS: no Word running, no lock, local APFS.
fn stale_mac() -> MacFacts {
    MacFacts {
        word_running: Some(false),
        lock_seen: Some(false),
        local_apfs_or_hfs: Some(true),
    }
}

#[test]
fn mac_owner_file_with_no_word_no_lock_on_a_local_volume_is_stale() {
    assert_eq!(mac_rule(stale_mac()), Decision::Stale);
}

/// Any Word running holds every owner file, whenever either was made: Word reuses a crashed
/// session's owner file when it reopens the document (research 38, point 5), so the rule no
/// longer compares birthtimes with Word's starts and has no birthtime to compare.
#[test]
fn mac_any_running_word_is_held() {
    let facts = MacFacts {
        word_running: Some(true),
        ..stale_mac()
    };
    assert_eq!(mac_rule(facts), Decision::Held);
}

#[test]
fn mac_a_lock_on_the_document_is_held() {
    let facts = MacFacts {
        lock_seen: Some(true),
        ..stale_mac()
    };
    assert_eq!(mac_rule(facts), Decision::Held);
}

#[test]
fn mac_a_network_or_other_volume_is_held() {
    let facts = MacFacts {
        local_apfs_or_hfs: Some(false),
        ..stale_mac()
    };
    assert_eq!(mac_rule(facts), Decision::Held);
}

#[test]
fn mac_each_failed_query_is_held_as_query_failed() {
    let cases: [(&str, MacFacts); 3] = [
        (
            "process listing",
            MacFacts {
                word_running: None,
                ..stale_mac()
            },
        ),
        (
            "F_GETLK",
            MacFacts {
                lock_seen: None,
                ..stale_mac()
            },
        ),
        (
            "statfs",
            MacFacts {
                local_apfs_or_hfs: None,
                ..stale_mac()
            },
        ),
    ];
    for (query, facts) in cases {
        assert_eq!(mac_rule(facts), Decision::QueryFailed, "{query}");
    }
    // A failed query wins over the facts that would say held anyway: it is still not stale.
    let facts = MacFacts {
        lock_seen: Some(true),
        word_running: None,
        ..stale_mac()
    };
    assert_eq!(mac_rule(facts), Decision::QueryFailed);
    assert_eq!(mac_rule(MacFacts::default()), Decision::QueryFailed);
}

/// The facts of a stale owner file on Windows: it exists, the no-share open worked, no Word.
fn stale_windows() -> WindowsFacts {
    WindowsFacts {
        owner_exists: true,
        exclusive_open: true,
        word_running: Some(false),
    }
}

#[test]
fn windows_owner_file_with_the_document_free_and_no_word_is_stale() {
    assert_eq!(windows_rule(stale_windows()), Decision::Stale);
}

#[test]
fn windows_a_running_word_a_failed_open_or_no_owner_file_is_held() {
    let cases = [
        (
            "Word running",
            WindowsFacts {
                word_running: Some(true),
                ..stale_windows()
            },
        ),
        (
            "no-share open failed",
            WindowsFacts {
                exclusive_open: false,
                ..stale_windows()
            },
        ),
        (
            "no owner file",
            WindowsFacts {
                owner_exists: false,
                ..stale_windows()
            },
        ),
    ];
    for (row, facts) in cases {
        assert_eq!(windows_rule(facts), Decision::Held, "{row}");
    }
}

#[test]
fn windows_a_failed_process_listing_is_held_as_query_failed() {
    let facts = WindowsFacts {
        word_running: None,
        ..stale_windows()
    };
    assert_eq!(windows_rule(facts), Decision::QueryFailed);
    assert_eq!(windows_rule(WindowsFacts::default()), Decision::QueryFailed);
}

#[test]
fn each_decision_logs_its_category() {
    assert_eq!(Decision::Stale.category(), "owner_stale");
    assert_eq!(Decision::Held.category(), "owner_held");
    assert_eq!(Decision::QueryFailed.category(), "query_failed");
}
