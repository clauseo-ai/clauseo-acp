//! The header checks and the log's method names.

use super::*;

#[test]
fn protocol_versions_fit_for_a_header() {
    for good in ["2025-06-18", "2026-07-28", "v1.2_rc"] {
        assert_eq!(checked_protocol_version(good), Some(good));
    }
    let long = "1".repeat(33);
    for bad in [
        "",
        "2025-06-18\r\nX: 1",
        "2025 06",
        "2025/06/18",
        long.as_str(),
        "é",
    ] {
        assert_eq!(checked_protocol_version(bad), None, "{bad:?}");
    }
}

#[test]
fn session_ids_fit_for_a_header() {
    let longest = "~".repeat(256);
    for good in ["fake-session-0001", "!", longest.as_str()] {
        assert_eq!(checked_session_id(good), Some(good));
    }
    let long = "a".repeat(257);
    for bad in ["", "a b", "a\tb", "a\r\nb", "é", long.as_str()] {
        assert_eq!(checked_session_id(bad), None, "{bad:?}");
    }
}

#[test]
fn the_log_shows_plain_method_names_only() {
    assert_eq!(log_name(Some("tools/call")), "tools/call");
    assert_eq!(
        log_name(Some("notifications/initialized")),
        "notifications/initialized"
    );
    assert_eq!(log_name(None), "response");
    let long = "m".repeat(65);
    for bad in ["", "a b", "a\nb", "secret=1", long.as_str()] {
        assert_eq!(log_name(Some(bad)), "other", "{bad:?}");
    }
}
