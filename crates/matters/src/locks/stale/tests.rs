//! The stale rules as plain functions of their facts, on every OS: each row of each rule, and
//! each failed query counted as held.

use std::time::{Duration, SystemTime};

use super::{Decision, MacFacts, WindowsFacts, mac_rule, windows_rule};

/// A moment `seconds` after the epoch.
fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// The facts of a stale owner file: no lock, born at 100, no Word running, local APFS.
fn stale_mac() -> MacFacts {
    MacFacts {
        lock_seen: Some(false),
        owner_born: Some(at(100)),
        word_starts: Some(Vec::new()),
        local_apfs_or_hfs: Some(true),
    }
}

#[test]
fn mac_owner_file_with_no_lock_no_word_on_a_local_volume_is_stale() {
    assert_eq!(mac_rule(&stale_mac()), Decision::Stale);
}

#[test]
fn mac_owner_file_older_than_every_running_word_is_stale() {
    let facts = MacFacts {
        word_starts: Some(vec![at(101), at(500)]),
        ..stale_mac()
    };
    assert_eq!(mac_rule(&facts), Decision::Stale);
}

#[test]
fn mac_a_lock_on_the_document_is_held() {
    let facts = MacFacts {
        lock_seen: Some(true),
        ..stale_mac()
    };
    assert_eq!(mac_rule(&facts), Decision::Held);
}

#[test]
fn mac_a_word_started_before_or_with_the_owner_file_is_held() {
    for starts in [vec![at(99)], vec![at(100)], vec![at(500), at(50)]] {
        let facts = MacFacts {
            word_starts: Some(starts.clone()),
            ..stale_mac()
        };
        assert_eq!(mac_rule(&facts), Decision::Held, "{starts:?}");
    }
}

#[test]
fn mac_a_network_or_other_volume_is_held() {
    let facts = MacFacts {
        local_apfs_or_hfs: Some(false),
        ..stale_mac()
    };
    assert_eq!(mac_rule(&facts), Decision::Held);
}

#[test]
fn mac_each_failed_query_is_held_as_query_failed() {
    let cases: [(&str, MacFacts); 4] = [
        (
            "F_GETLK",
            MacFacts {
                lock_seen: None,
                ..stale_mac()
            },
        ),
        (
            "birthtime",
            MacFacts {
                owner_born: None,
                ..stale_mac()
            },
        ),
        (
            "process listing",
            MacFacts {
                word_starts: None,
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
        assert_eq!(mac_rule(&facts), Decision::QueryFailed, "{query}");
    }
    // A failed query wins over the facts that would say held anyway: it is still not stale.
    let facts = MacFacts {
        lock_seen: Some(true),
        word_starts: None,
        ..stale_mac()
    };
    assert_eq!(mac_rule(&facts), Decision::QueryFailed);
    assert_eq!(mac_rule(&MacFacts::default()), Decision::QueryFailed);
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
