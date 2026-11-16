//! The choice over a failed lookup, with the probe faked: each kind, with an earlier proxy that
//! answers, one that does not, or none, and with the firm meaning a proxy or not.

use std::cell::Cell;
use std::time::Duration;

use super::{Chosen, Key, Plan, REMEMBER_MOST, Remembered};
use crate::proxy::upstream::{Answer, Decided, FailKind, Failure, Route, Scheme};

fn key(host: &str) -> Key {
    (Scheme::Https, host.to_owned(), 443)
}

fn proxy() -> Route {
    Route::Http {
        host: "proxy.firm.test".to_owned(),
        port: 8080,
    }
}

fn got(at: Duration) -> Decided {
    Decided {
        answer: Answer::Route(proxy()),
        got: Some(at),
        now: at,
    }
}

fn failed_as(failure: Failure, kind: FailKind, now: Duration) -> Decided {
    Decided {
        answer: Answer::Failed(failure, kind),
        got: None,
        now,
    }
}

fn failed(now: Duration) -> Decided {
    failed_as(Failure::MeantProxy, FailKind::Other, now)
}

/// The plan, a probe answered with `reached`; with how many probes ran.
fn choose_probed(
    remembered: &Remembered,
    key: &Key,
    decided: Decided,
    reached: bool,
) -> (Chosen, u32) {
    let probes = Cell::new(0);
    let chosen = match remembered.plan(key, decided) {
        Plan::Chosen(chosen) => chosen,
        Plan::Probe { host, port } => {
            probes.set(probes.get() + 1);
            remembered.probed(key, Route::Http { host, port }, reached)
        }
    };
    (chosen, probes.get())
}

/// The plan, with no probe expected.
fn choose(remembered: &Remembered, key: &Key, decided: Decided) -> Chosen {
    let (chosen, probes) = choose_probed(remembered, key, decided, false);
    assert_eq!(probes, 0, "no probe");
    chosen
}

/// Past the cap the oldest host goes first, and the newest is still reused.
#[test]
fn the_cap_holds_and_drops_the_oldest() {
    let remembered = Remembered::default();
    let extra = 10;
    for n in 0..REMEMBER_MOST + extra {
        let at = Duration::from_secs(u64::try_from(n).unwrap());
        choose(&remembered, &key(&format!("h{n}.test")), got(at));
    }
    assert_eq!(remembered.len(), REMEMBER_MOST);
    let now = Duration::from_secs(u64::try_from(REMEMBER_MOST + extra).unwrap());
    assert_eq!(
        choose(&remembered, &key("h0.test"), failed(now)),
        Chosen::Refuse
    );
    let newest = format!("h{}.test", REMEMBER_MOST + extra - 1);
    assert_eq!(
        choose(&remembered, &key(&newest), failed(now)),
        Chosen::Go(proxy())
    );
}

/// A route from the settings or the environment (no OS time) is not remembered.
#[test]
fn a_route_not_from_the_os_is_not_remembered() {
    let remembered = Remembered::default();
    let plain = Decided {
        answer: Answer::Route(proxy()),
        got: None,
        now: Duration::ZERO,
    };
    assert_eq!(
        choose(&remembered, &key("a.test"), plain),
        Chosen::Go(proxy())
    );
    assert_eq!(remembered.len(), 0);
}

/// The earlier proxy for a case: one that answers the probe, one that does not, or none.
#[derive(Debug, Clone, Copy)]
enum Earlier {
    Reachable,
    Unreachable,
    None,
}

/// Each kind × earlier proxy × whether the firm meant a proxy. `PacUnreachable` probes an
/// earlier proxy, keeps it when it answers and goes direct when not, and goes direct with none
/// even when a proxy was meant. `BadScript` and `Other` never probe: they reuse the earlier
/// proxy whether it answers or not, and with none refuse when a proxy was meant, else go direct.
#[test]
fn each_kind_with_each_earlier_proxy_and_intent_chooses_as_decided() {
    use Earlier::{None as Gone, Reachable, Unreachable};
    use FailKind::{BadScript, Other, PacUnreachable};
    let go = |route: Route| Chosen::Go(route);
    let direct = || Chosen::Go(Route::Direct);
    for meant in [true, false] {
        let failure = if meant {
            Failure::MeantProxy
        } else {
            Failure::Unmeant
        };
        let refuse_or_direct = if meant { Chosen::Refuse } else { direct() };
        let cases = [
            (PacUnreachable, Reachable, go(proxy()), 1),
            (PacUnreachable, Unreachable, direct(), 1),
            (PacUnreachable, Gone, direct(), 0),
            (BadScript, Reachable, go(proxy()), 0),
            (BadScript, Unreachable, go(proxy()), 0),
            (BadScript, Gone, refuse_or_direct.clone(), 0),
            (Other, Reachable, go(proxy()), 0),
            (Other, Unreachable, go(proxy()), 0),
            (Other, Gone, refuse_or_direct.clone(), 0),
        ];
        for (kind, earlier, want, want_probes) in cases {
            let remembered = Remembered::default();
            let host = key("a.test");
            if !matches!(earlier, Earlier::None) {
                choose(&remembered, &host, got(Duration::ZERO));
            }
            let reached = matches!(earlier, Earlier::Reachable);
            let decided = failed_as(failure, kind, Duration::from_mins(2));
            let (chosen, probes) = choose_probed(&remembered, &host, decided, reached);
            let case = format!("{kind:?}, earlier {earlier:?}, meant {meant}");
            assert_eq!(chosen, want, "{case}");
            assert_eq!(probes, want_probes, "{case}: probes");
        }
    }
}

/// An earlier proxy that did not answer is forgotten: the next unfetchable PAC goes direct with
/// no probe, and a later failure of another kind no longer reuses it (a meant proxy refuses).
/// One that answered stays, and is probed again on the next.
#[test]
fn an_unreachable_earlier_proxy_is_not_reused_again() {
    let remembered = Remembered::default();
    let host = key("a.test");
    choose(&remembered, &host, got(Duration::ZERO));
    let unfetched = |now| failed_as(Failure::MeantProxy, FailKind::PacUnreachable, now);
    let (chosen, probes) =
        choose_probed(&remembered, &host, unfetched(Duration::from_mins(1)), true);
    assert_eq!((chosen, probes), (Chosen::Go(proxy()), 1), "answered: kept");
    let (chosen, probes) =
        choose_probed(&remembered, &host, unfetched(Duration::from_mins(2)), false);
    assert_eq!(
        (chosen, probes),
        (Chosen::Go(Route::Direct), 1),
        "silent: direct"
    );
    let (chosen, probes) =
        choose_probed(&remembered, &host, unfetched(Duration::from_mins(3)), true);
    assert_eq!(
        (chosen, probes),
        (Chosen::Go(Route::Direct), 0),
        "forgotten"
    );
    let bad = failed_as(
        Failure::MeantProxy,
        FailKind::BadScript,
        Duration::from_mins(4),
    );
    assert_eq!(choose(&remembered, &host, bad), Chosen::Refuse);
}

/// An earlier `DIRECT` is not probed: an unfetchable PAC goes direct.
#[test]
fn an_earlier_direct_is_not_probed() {
    let remembered = Remembered::default();
    let host = key("a.test");
    let direct = Decided {
        answer: Answer::Route(Route::Direct),
        got: Some(Duration::ZERO),
        now: Duration::ZERO,
    };
    choose(&remembered, &host, direct);
    let unfetched = failed_as(
        Failure::MeantProxy,
        FailKind::PacUnreachable,
        Duration::from_mins(1),
    );
    assert_eq!(
        choose(&remembered, &host, unfetched),
        Chosen::Go(Route::Direct)
    );
}

/// At 10 minutes the earlier proxy is past the window: an unfetchable PAC goes direct unprobed.
#[test]
fn past_the_window_an_unfetchable_pac_goes_direct_unprobed() {
    let remembered = Remembered::default();
    let host = key("a.test");
    choose(&remembered, &host, got(Duration::ZERO));
    let unfetched = failed_as(
        Failure::MeantProxy,
        FailKind::PacUnreachable,
        Duration::from_mins(10),
    );
    assert_eq!(
        choose(&remembered, &host, unfetched),
        Chosen::Go(Route::Direct)
    );
}
