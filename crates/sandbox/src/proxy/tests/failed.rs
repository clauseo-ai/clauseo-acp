//! A failed upstream lookup (decision record 0039): the host's route from a real lookup under 10
//! minutes ago, else a refusal when the settings show the firm meant a proxy, else direct.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::harness::{Rig, Setup, echo, rig, round_trip, upstream};
use crate::proxy::upstream::{Answer, Configured, FailKind, Failure, PacSetting, Reach, Scheme};
use crate::proxy::{Limits, Verdict};
use crate::{Denial, Route, Upstream};

/// The OS answer per host; a host not set answers direct.
pub(super) type Answers = Arc<Mutex<HashMap<String, Answer>>>;

/// A rig allowing `a.test` and `b.test` with the scripted OS lookup. `a.test` resolves to
/// 192.0.2.10, where an echo server answers on 443.
fn failing_rig() -> (Rig, Answers) {
    failing_rig_with(None)
}

/// [`failing_rig`], an earlier proxy probed with `reach` (the real probe when `None`).
pub(super) fn failing_rig_with(reach: Option<Box<Reach>>) -> (Rig, Answers) {
    let answers: Answers = Arc::default();
    let script = Arc::clone(&answers);
    let rig = rig(Setup {
        reach,
        allow: vec!["a.test", "b.test"],
        upstream: Upstream::System,
        answers: Some(Box::new(move |_: Scheme, host: &str, _: u16| {
            let script = script.lock().unwrap();
            script
                .get(host)
                .cloned()
                .unwrap_or(Answer::Route(Route::Direct))
        })),
        ..Setup::default()
    });
    rig.net.name("a.test", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    (rig, answers)
}

pub(super) fn set(answers: &Answers, host: &str, answer: Answer) {
    answers.lock().unwrap().insert(host.to_owned(), answer);
}

/// The firm's proxy, which opens every tunnel: its address, its route, and the heads it got.
pub(super) fn firm_proxy() -> (SocketAddr, Route, Receiver<String>) {
    let (address, heads) = upstream("HTTP/1.1 200 Connection established\r\n\r\n");
    let route = Route::Http {
        host: address.ip().to_string(),
        port: address.port(),
    };
    (address, route, heads)
}

fn meant() -> Answer {
    Answer::Failed(Failure::MeantProxy, FailKind::Other)
}

fn unavailable(host: &str, port: u16) -> Denial {
    Denial::UpstreamUnavailable {
        host: host.to_owned(),
        port,
    }
}

/// Past the OS cache's minute, a failed lookup reuses the route a real lookup gave 9 minutes ago.
#[test]
fn a_failed_lookup_reuses_the_hosts_route_from_under_10_minutes_ago() {
    let (rig, answers) = failing_rig();
    let (address, route, _heads) = firm_proxy();
    set(&answers, "a.test", Answer::Route(route));
    assert_eq!(rig.connect("a.test:443").1, 200);
    set(&answers, "a.test", meant());
    rig.clock.advance(Duration::from_mins(9));
    let (mut tunnel, code) = rig.connect("a.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"kept"), b"kept");
    assert_eq!(
        rig.net.dialled(),
        vec![address, address],
        "both through the proxy"
    );
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

/// At 10 minutes the route is not reused, and a firm that meant a proxy is refused.
#[test]
fn at_10_minutes_the_route_is_not_reused_and_a_meant_proxy_refuses() {
    let (rig, answers) = failing_rig();
    let (address, route, _heads) = firm_proxy();
    set(&answers, "a.test", Answer::Route(route));
    let (first, code) = rig.connect("a.test:443");
    assert_eq!(code, 200);
    // A tunnel is recorded only once its relay ends; close it and wait for that record so the
    // refusal's record cannot land ahead of it.
    drop(first);
    assert_eq!(rig.records(1)[0].verdict, Verdict::Tunnel);
    set(&answers, "a.test", meant());
    rig.clock.advance(Duration::from_mins(10));
    assert_eq!(rig.connect("a.test:443").1, 502);
    assert_eq!(rig.net.dialled(), vec![address], "never direct");
    assert_eq!(rig.denials(), vec![unavailable("a.test", 443)]);
    assert_eq!(rig.records(2)[1].verdict, Verdict::Failed);
}

/// A PAC URL or inline script, a static proxy, a PAC script that would not run (12166) and
/// settings that could not be read each refuse with nothing to reuse: CONNECT and plain HTTP
/// alike get 502, and nothing is dialled.
#[test]
fn a_pac_url_a_static_proxy_or_a_broken_script_refuses_with_nothing_to_reuse() {
    let cases = [
        Configured {
            pac: PacSetting::Named,
            ..Configured::default()
        },
        Configured {
            static_proxy: true,
            ..Configured::default()
        },
        Configured {
            broken_script: true,
            ..Configured::default()
        },
        Configured {
            unreadable: true,
            ..Configured::default()
        },
    ];
    for configured in cases {
        let (rig, answers) = failing_rig();
        set(
            &answers,
            "a.test",
            Answer::Failed(configured.failure(), FailKind::Other),
        );
        assert_eq!(rig.connect("a.test:443").1, 502, "{configured:?}");
        let answer = rig.exchange("GET http://a.test/ HTTP/1.1\r\n\r\n");
        assert!(
            answer.starts_with("HTTP/1.1 502 "),
            "{configured:?}: {answer}"
        );
        assert_eq!(
            rig.net.dialled(),
            Vec::<SocketAddr>::new(),
            "{configured:?}"
        );
        assert_eq!(
            rig.denials(),
            vec![unavailable("a.test", 443), unavailable("a.test", 80)],
            "{configured:?}"
        );
    }
}

/// A plain network (settings read, nothing configured, no WPAD) goes direct, as before.
#[test]
fn a_failed_lookup_on_a_plain_network_goes_direct() {
    let (rig, answers) = failing_rig();
    set(
        &answers,
        "a.test",
        Answer::Failed(Configured::default().failure(), FailKind::Other),
    );
    let (mut tunnel, code) = rig.connect("a.test:443");
    assert_eq!(code, 200);
    assert_eq!(round_trip(&mut tunnel, b"direct"), b"direct");
    let direct: SocketAddr = "192.0.2.10:443".parse().unwrap();
    assert_eq!(rig.net.dialled(), vec![direct]);
    assert_eq!(rig.denials(), Vec::<Denial>::new());
}

/// One host's failed lookup never takes another host's route.
#[test]
fn a_hosts_failure_does_not_use_another_hosts_route() {
    let (rig, answers) = failing_rig();
    let (address, route, _heads) = firm_proxy();
    set(&answers, "a.test", Answer::Route(route));
    set(&answers, "b.test", meant());
    assert_eq!(rig.connect("a.test:443").1, 200);
    assert_eq!(rig.connect("b.test:443").1, 502);
    assert_eq!(rig.net.dialled(), vec![address]);
    assert_eq!(rig.denials(), vec![unavailable("b.test", 443)]);
}

/// The refusals' denials pass the rate cap like any other.
#[test]
fn the_unavailable_denial_is_rate_capped() {
    let (rig, answers) = failing_rig();
    set(&answers, "a.test", meant());
    for _ in 0..25 {
        assert_eq!(rig.connect("a.test:443").1, 502);
    }
    assert_eq!(rig.denials().len(), 20);
    rig.clock.advance(crate::DENIAL_WINDOW);
    assert_eq!(rig.connect("a.test:443").1, 502);
    let denials = rig.denials();
    assert_eq!(denials.len(), 22);
    assert_eq!(denials[20], Denial::Dropped { count: 5 });
    assert_eq!(denials[21], unavailable("a.test", 443));
}

/// A lookup slower than the connect timeout counts as failed: the settings' intent decides.
#[test]
fn a_lookup_too_slow_refuses_when_the_settings_meant_a_proxy() {
    let (release, wait) = mpsc::channel::<()>();
    let wait = Mutex::new(wait);
    let hung = move |_: Scheme, _: &str, _: u16| -> Answer {
        if wait.lock().unwrap().recv().is_err() {
            // Released by the test dropping its end.
        }
        Answer::Route(Route::Direct)
    };
    let rig = rig(Setup {
        allow: vec!["a.test"],
        upstream: Upstream::System,
        answers: Some(Box::new(hung)),
        intent: Some(Box::new(|| Failure::MeantProxy)),
        limits: Limits {
            connect: Duration::from_millis(100),
            ..Limits::default()
        },
        ..Setup::default()
    });
    assert_eq!(rig.connect("a.test:443").1, 502);
    assert_eq!(rig.net.dialled(), Vec::<SocketAddr>::new());
    assert_eq!(rig.denials(), vec![unavailable("a.test", 443)]);
    drop(release);
}

/// A lookup too slow whose settings read is too slow as well refuses rather than going direct;
/// one whose settings read nothing goes direct.
#[test]
fn a_lookup_too_slow_refuses_when_the_settings_cannot_be_read_in_time() {
    for (slow_settings, code) in [(true, 502), (false, 200)] {
        let (release, wait) = mpsc::channel::<()>();
        let wait = Arc::new(Mutex::new(wait));
        let held = Arc::clone(&wait);
        let hung = move |_: Scheme, _: &str, _: u16| -> Answer {
            if held.lock().unwrap().recv().is_err() {
                // Released by the test dropping its end.
            }
            Answer::Route(Route::Direct)
        };
        let settings = move || {
            if slow_settings && wait.lock().unwrap().recv().is_err() {
                // Released by the test dropping its end.
            }
            Failure::Unmeant
        };
        let rig = rig(Setup {
            allow: vec!["a.test"],
            upstream: Upstream::System,
            answers: Some(Box::new(hung)),
            intent: Some(Box::new(settings)),
            limits: Limits {
                connect: Duration::from_millis(100),
                ..Limits::default()
            },
            ..Setup::default()
        });
        rig.net.name("a.test", &["192.0.2.10"]);
        rig.net.route("192.0.2.10:443", echo());
        assert_eq!(
            rig.connect("a.test:443").1,
            code,
            "slow settings: {slow_settings}"
        );
        let denials = if slow_settings {
            vec![unavailable("a.test", 443)]
        } else {
            Vec::new()
        };
        assert_eq!(rig.denials(), denials);
        drop(release);
    }
}
