//! The cap on the 407s' log lines: the bucket alone, then the proxy under a flood.

use std::io::Write;
use std::time::Duration;

use super::harness::{read_all, read_head, rig_allowing, status};
use crate::proxy::Verdict;
use crate::proxy::server::auth_log::{Admit, AuthLog};

const fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn the_first_five_log_and_the_sixth_is_dropped() {
    let mut log = AuthLog::new(secs(0));
    for _ in 0..5 {
        assert_eq!(log.admit(secs(1)), Admit::Log { suppressed: 0 });
    }
    assert_eq!(log.admit(secs(1)), Admit::Drop);
}

#[test]
fn ten_seconds_earn_one_line_that_reports_the_dropped_count() {
    let mut log = AuthLog::new(secs(0));
    for _ in 0..5 {
        assert_eq!(log.admit(secs(0)), Admit::Log { suppressed: 0 });
    }
    for _ in 0..3 {
        assert_eq!(log.admit(secs(9)), Admit::Drop);
    }
    assert_eq!(log.admit(secs(10)), Admit::Log { suppressed: 3 });
    assert_eq!(log.admit(secs(10)), Admit::Drop);
    assert_eq!(log.admit(secs(20)), Admit::Log { suppressed: 1 });
}

#[test]
fn the_refill_stops_at_five() {
    let mut log = AuthLog::new(secs(0));
    for _ in 0..5 {
        assert_eq!(log.admit(secs(0)), Admit::Log { suppressed: 0 });
    }
    assert_eq!(log.admit(secs(0)), Admit::Drop);
    assert_eq!(log.admit(secs(1000)), Admit::Log { suppressed: 1 });
    for _ in 0..4 {
        assert_eq!(log.admit(secs(1000)), Admit::Log { suppressed: 0 });
    }
    assert_eq!(log.admit(secs(1000)), Admit::Drop);
}

#[test]
fn a_full_bucket_earns_nothing_while_full() {
    let mut log = AuthLog::new(secs(0));
    assert_eq!(log.admit(secs(9)), Admit::Log { suppressed: 0 });
    for _ in 0..4 {
        assert_eq!(log.admit(secs(10)), Admit::Log { suppressed: 0 });
    }
    // The one spent at 9 s comes back at 19 s, not at 10 s.
    assert_eq!(log.admit(secs(18)), Admit::Drop);
    assert_eq!(log.admit(secs(19)), Admit::Log { suppressed: 1 });
}

#[test]
fn twenty_requests_without_a_token_log_five_lines_then_one_more_after_ten_seconds() {
    let rig = rig_allowing(&["api.anthropic.com"]);
    let send = || {
        let mut stream = rig.agent();
        stream
            .write_all(b"CONNECT api.anthropic.com:443 HTTP/1.1\r\n\r\n")
            .unwrap();
        assert_eq!(status(&read_head(&mut stream)), 407);
        assert_eq!(read_all(&mut stream), b"");
    };
    for _ in 0..20 {
        send();
    }
    assert_eq!(rig.records(5).len(), 5);
    rig.clock.advance(secs(10));
    send();
    let records = rig.records(6);
    assert_eq!(records.len(), 6, "{records:?}");
    assert!(records.iter().all(|r| r.verdict == Verdict::AuthRequired));
    // The sixth line, in the proxy's own log, says how many were dropped before it.
    // A record is written after its client sees the connection close, so the twentieth may be
    // counted only once the clock has moved: it is then the sixth line, after 14 drops.
    let counts: Vec<u64> = records.iter().map(|r| r.suppressed).collect();
    assert!(counts[..5].iter().all(|n| *n == 0), "{counts:?}");
    assert!(matches!(counts[5], 14 | 15), "{counts:?}");
    let line = serde_json::to_value(&records[5]).unwrap();
    assert_eq!(line["suppressed"], counts[5]);
    assert!(
        serde_json::to_value(&records[0])
            .unwrap()
            .get("suppressed")
            .is_none()
    );
}
