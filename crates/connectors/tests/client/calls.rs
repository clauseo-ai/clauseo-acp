//! Liveness and price, the account, recharge and its poll, and out of credit.

use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clauseo_connectors::fake::{FakeOptions, Fixture, ScratchDir};
use clauseo_connectors::{
    CancelFlag, Clock, Connectors, Error, MemoryStore, Origin, PricingStatus, RechargeStatus,
    SystemClock,
};

use crate::support::*;

#[test]
fn pricing_live_is_live_with_its_packs() {
    let rig = rig();
    let liveness = rig.connectors.liveness();
    assert!(liveness.live);
    let pricing = liveness.pricing.unwrap();
    assert_eq!(pricing.status, PricingStatus::Live);
    assert_ne!(pricing.packs.len(), 0);
    assert!(pricing.packs.iter().any(|pack| pack.pack == "200"));
    let request = &rig.fake.requests_to("/pricing.json")[0];
    assert!(request.header("authorization").is_none());
    assert_eq!(request.header("user-agent"), Some("Clauseo/0.0.0-test"));
}

fn not_live(rig: &Rig) {
    let liveness = rig.connectors.liveness();
    assert!(!liveness.live, "{liveness:?}");
    assert!(liveness.fetched_at.is_some());
}

#[test]
fn preview_404_500_and_not_json_are_not_live() {
    let rig = rig();
    rig.fake.set_pricing(Fixture::File("pricing-preview.json"));
    let liveness = rig.connectors.liveness();
    assert!(!liveness.live);
    assert_eq!(liveness.pricing.unwrap().status, PricingStatus::Preview);
    for (status, body) in [(404, "{}"), (500, "{}"), (200, "<html>not json</html>")] {
        let rig = crate::support::rig();
        rig.fake.set_pricing(Fixture::Raw {
            status,
            content_type: "text/html".to_owned(),
            body: body.as_bytes().to_vec(),
        });
        not_live(&rig);
    }
}

#[test]
fn a_refused_connection_and_a_timeout_are_not_live() {
    // A port nothing listens on.
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let scratch = ScratchDir::new("refused").unwrap();
    let rig = rig();
    let mut config = rig.fake.config(scratch.state_file());
    config.origin = Origin::Fake(url::Url::parse(&format!("http://127.0.0.1:{closed}")).unwrap());
    let connectors = Connectors::new(config, Arc::new(MemoryStore::new())).unwrap();
    assert!(!connectors.liveness().live);

    let rig = crate::support::rig();
    rig.connectors
        .set_request_timeout_for_test(Duration::from_millis(200));
    rig.fake.delay_next("/pricing.json", Duration::from_secs(3));
    let started = SystemClock.now();
    not_live(&rig);
    assert!(SystemClock.now().duration_since(started).unwrap() < Duration::from_secs(2));
}

#[test]
fn liveness_is_cached_24_hours_and_a_failure_10_minutes() {
    let rig = manual_rig();
    assert!(rig.connectors.liveness().live);
    rig.clock().advance(Duration::from_hours(23));
    assert!(rig.connectors.liveness().live);
    assert_eq!(rig.fake.requests_to("/pricing.json").len(), 1);
    rig.clock().advance(Duration::from_hours(1));
    rig.fake.fail_next("/pricing.json", 500);
    assert!(!rig.connectors.liveness().live);
    assert_eq!(rig.fake.requests_to("/pricing.json").len(), 2);
    rig.clock().advance(Duration::from_mins(9));
    assert!(!rig.connectors.liveness().live);
    assert_eq!(rig.fake.requests_to("/pricing.json").len(), 2);
    rig.clock().advance(Duration::from_mins(1));
    assert!(rig.connectors.liveness().live);
    assert_eq!(rig.fake.requests_to("/pricing.json").len(), 3);
}

#[test]
fn the_account_is_read_at_most_once_a_minute() {
    let rig = manual_rig();
    sign_in(&rig);
    let first = rig.handle.account().unwrap();
    assert_eq!(first.balance_credits, 1000);
    let request = &rig.fake.requests_to("/v1/case-law/account")[0];
    assert_eq!(request.query, "recent_days=30");
    rig.clock().advance(Duration::from_secs(30));
    rig.fake.set_balance(900);
    assert_eq!(rig.handle.account().unwrap().balance_credits, 1000);
    assert_eq!(rig.fake.requests_to("/v1/case-law/account").len(), 1);
    rig.clock().advance(Duration::from_secs(31));
    assert_eq!(rig.handle.account().unwrap().balance_credits, 900);
    assert_eq!(rig.fake.requests_to("/v1/case-law/account").len(), 2);
}

#[test]
fn the_account_after_sign_out_is_not_served_from_the_cache() {
    let rig = manual_rig();
    sign_in(&rig);
    rig.handle.account().unwrap();
    rig.handle.sign_out().unwrap();
    assert!(matches!(rig.handle.account(), Err(Error::Auth(_))));
}

#[test]
fn create_recharge_sends_the_pack_and_returns_our_pay_link() {
    let rig = rig();
    sign_in(&rig);
    let recharge = rig.handle.create_recharge("200").unwrap();
    let request = &rig.fake.requests_to("/v1/case-law/recharges")[0];
    assert_eq!(request.json().unwrap(), serde_json::json!({"pack": "200"}));
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(recharge.status, RechargeStatus::AwaitingPayment);
    let url = recharge.url.unwrap();
    assert_eq!(
        url.as_str(),
        format!("https://data.clauseo.chat/pay/{}", recharge.recharge_id)
    );
}

#[test]
fn a_pay_link_that_is_not_ours_is_refused() {
    let rig = rig();
    sign_in(&rig);
    let base = rig.fake.base().to_owned();
    let port = rig.fake.base_url().port().unwrap();
    for bad in [
        base.replace("http://", "https://") + "/pay/r1",
        "http://evil.example/pay/r1".to_owned(),
        format!("http://127.0.0.1:{}/pay/r1", port.wrapping_add(1)),
        base.replace("http://", "http://user@") + "/pay/r1",
        format!("{base}/other/r1"),
        format!("{base}/pay/"),
    ] {
        rig.fake.set_pay_url(Some(&bad));
        assert_eq!(
            rig.handle.create_recharge("200"),
            Err(Error::NotOurPayLink),
            "{bad}"
        );
    }
}

#[test]
fn wait_recharge_polls_until_paid() {
    let rig = rig();
    sign_in(&rig);
    rig.fake.set_wait_cap(Duration::from_millis(100));
    let recharge = rig.handle.create_recharge("200").unwrap();
    let id = recharge.recharge_id;
    let payer = id.clone();
    let handle = rig.handle.clone();
    let waiter = thread::spawn(move || handle.wait_recharge(&payer, &CancelFlag::new()));
    // Let a few polls come back unpaid, then pay.
    while rig
        .fake
        .requests_to(&format!("/v1/case-law/recharges/{id}"))
        .len()
        < 3
    {
        thread::park_timeout(Duration::from_millis(20));
    }
    assert!(rig.fake.mark_paid(&id));
    let paid = waiter.join().unwrap().unwrap();
    assert_eq!(paid.status, RechargeStatus::Paid);
    for poll in rig
        .fake
        .requests_to(&format!("/v1/case-law/recharges/{id}"))
    {
        assert_eq!(poll.query, "wait=40");
    }
}

#[test]
fn wait_recharge_stops_at_the_deadline() {
    let rig = rig_with(FakeOptions::default(), false, |config| {
        config.recharge_deadline = Duration::from_millis(300);
    });
    sign_in(&rig);
    // A poll that ignored the deadline would be held for the fake's 10 s.
    rig.fake.set_wait_cap(Duration::from_secs(10));
    let recharge = rig.handle.create_recharge("200").unwrap();
    let started = SystemClock.now();
    let result = rig
        .handle
        .wait_recharge(&recharge.recharge_id, &CancelFlag::new());
    assert_eq!(result, Err(Error::RechargeTimedOut));
    let took = SystemClock.now().duration_since(started).unwrap();
    assert!(took < Duration::from_secs(3), "{took:?}");
}

#[test]
fn the_cancel_flag_stops_wait_recharge() {
    let rig = rig();
    sign_in(&rig);
    // A poll that ignored the flag would be held for the fake's 10 s.
    rig.fake.set_wait_cap(Duration::from_secs(10));
    let recharge = rig.handle.create_recharge("200").unwrap();
    let cancel = CancelFlag::new();
    let handle = rig.handle;
    let flag = cancel.clone();
    let waiter = thread::spawn(move || handle.wait_recharge(&recharge.recharge_id, &flag));
    thread::park_timeout(Duration::from_millis(100));
    let cancelled_at = SystemClock.now();
    cancel.cancel();
    assert_eq!(waiter.join().unwrap(), Err(Error::Cancelled));
    let took = SystemClock
        .now()
        .duration_since(cancelled_at)
        .unwrap_or_default();
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn wait_recharge_retries_after_a_server_error() {
    let rig = rig();
    sign_in(&rig);
    rig.fake.set_wait_cap(Duration::from_millis(50));
    let recharge = rig.handle.create_recharge("200").unwrap();
    let id = recharge.recharge_id;
    rig.fake.mark_paid(&id);
    rig.fake
        .fail_next(&format!("/v1/case-law/recharges/{id}"), 503);
    let started = SystemClock.now();
    let paid = rig.handle.wait_recharge(&id, &CancelFlag::new()).unwrap();
    assert_eq!(paid.status, RechargeStatus::Paid);
    let took = SystemClock.now().duration_since(started).unwrap();
    assert!(took >= Duration::from_secs(5), "the 5 s backoff: {took:?}");
}

#[test]
fn the_api_402_is_insufficient_credits_with_the_payment() {
    let rig = rig();
    sign_in(&rig);
    rig.fake.set_balance(0);
    let Err(Error::InsufficientCredits(payment)) = rig.handle.account() else {
        panic!("expected InsufficientCredits");
    };
    let id = payment.recharge_id.clone().unwrap();
    assert_eq!(
        payment.url.unwrap().as_str(),
        format!("https://data.clauseo.chat/pay/{id}")
    );
    let Err(Error::InsufficientCredits(again)) = rig.handle.create_recharge("200") else {
        panic!("expected InsufficientCredits");
    };
    assert_eq!(again.recharge_id, Some(id));
}

#[test]
fn a_recharge_id_that_is_not_one_is_refused_before_any_request() {
    let rig = rig();
    sign_in(&rig);
    rig.fake.clear_requests();
    for bad in ["", "../account", "a/b", "a?b=c"] {
        assert!(matches!(
            rig.handle.wait_recharge(bad, &CancelFlag::new()),
            Err(Error::Parse(_))
        ));
    }
    assert_eq!(rig.fake.requests().len(), 0);
}

/// A connector (or a proxy) answering `awaiting_payment` at once is polled at most every 2 s:
/// within a 4 s deadline, no more than 4 / 2 + 1 = 3 polls.
#[test]
fn an_instant_awaiting_payment_answer_is_polled_at_most_every_2_seconds() {
    let rig = rig_with(FakeOptions::default(), false, |config| {
        config.recharge_deadline = Duration::from_secs(4);
    });
    sign_in(&rig);
    rig.fake.set_wait_cap(Duration::ZERO);
    let recharge = rig.handle.create_recharge("200").unwrap();
    let id = recharge.recharge_id;
    let result = rig.handle.wait_recharge(&id, &CancelFlag::new());
    assert_eq!(result, Err(Error::RechargeTimedOut));
    let polls = rig
        .fake
        .requests_to(&format!("/v1/case-law/recharges/{id}"))
        .len();
    assert!((2..=3).contains(&polls), "{polls} polls");
}

#[test]
fn the_cancel_flag_acts_at_once_during_the_gap_between_polls() {
    let rig = rig();
    sign_in(&rig);
    rig.fake.set_wait_cap(Duration::ZERO);
    let recharge = rig.handle.create_recharge("200").unwrap();
    let id = recharge.recharge_id;
    let polls = format!("/v1/case-law/recharges/{id}");
    let cancel = CancelFlag::new();
    let flag = cancel.clone();
    let handle = rig.handle.clone();
    let waiter = thread::spawn(move || handle.wait_recharge(&id, &flag));
    // The first poll is answered at once; once the fake has seen it, the wait is in its 2 s gap.
    while rig.fake.requests_to(&polls).is_empty() {
        thread::park_timeout(Duration::from_millis(10));
    }
    CancelFlag::new().wait_timeout(Duration::from_millis(100));
    let cancelled_at = SystemClock.now();
    cancel.cancel();
    assert_eq!(waiter.join().unwrap(), Err(Error::Cancelled));
    // About 1.9 s of the gap is left, so a wait that ignored the flag would take longer than this.
    let took = SystemClock
        .now()
        .duration_since(cancelled_at)
        .unwrap_or_default();
    assert!(took < Duration::from_secs(1), "{took:?}");
}

#[test]
fn an_answer_for_another_recharge_is_a_parse_error() {
    let rig = rig();
    sign_in(&rig);
    let recharge = rig.handle.create_recharge("200").unwrap();
    let id = recharge.recharge_id;
    let body = serde_json::json!({
        "recharge_id": "rch_someone_else",
        "status": "paid",
        "credits": 2000,
    })
    .to_string();
    rig.fake.answer_next(
        &format!("/v1/case-law/recharges/{id}"),
        200,
        &[("Content-Type", "application/json")],
        body.as_bytes(),
    );
    assert!(matches!(
        rig.handle.wait_recharge(&id, &CancelFlag::new()),
        Err(Error::Parse(_))
    ));
}

#[test]
fn a_redirect_is_not_followed() {
    let rig = rig();
    sign_in(&rig);
    let elsewhere = rig.fake.url("/elsewhere");
    rig.fake.answer_next(
        "/v1/case-law/account",
        302,
        &[("Location", elsewhere.as_str())],
        b"",
    );
    assert_eq!(rig.handle.account(), Err(Error::Http { status: 302 }));
    assert_eq!(rig.fake.requests_to("/elsewhere").len(), 0);
}

#[test]
fn a_recharge_deadline_that_overflows_the_clock_is_a_parse_error() {
    let rig = rig_with(FakeOptions::default(), false, |config| {
        config.recharge_deadline = Duration::MAX;
    });
    sign_in(&rig);
    let recharge = rig.handle.create_recharge("200").unwrap();
    assert!(matches!(
        rig.handle
            .wait_recharge(&recharge.recharge_id, &CancelFlag::new()),
        Err(Error::Parse(_))
    ));
}

/// `ProxyChoice::Resolve`: `Connectors::new` never asks the resolver; every request does, so
/// the route follows it (a dead proxy, then direct); a refused route fails the request without
/// going around it.
#[test]
fn a_resolved_proxy_is_asked_per_request_and_never_at_start() {
    use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

    use clauseo_connectors::{ProxyChoice, ProxyRefused, ProxyResolver};

    let rig = rig();
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let asked = Arc::new(AtomicUsize::new(0));
    let mode = Arc::new(AtomicU8::new(0));
    let resolver = {
        let (asked, mode) = (Arc::clone(&asked), Arc::clone(&mode));
        ProxyResolver::new(move || {
            asked.fetch_add(1, Ordering::SeqCst);
            match mode.load(Ordering::SeqCst) {
                0 => Ok(ProxyChoice::Fixed(
                    url::Url::parse(&format!("http://127.0.0.1:{closed}")).unwrap(),
                )),
                1 => Ok(ProxyChoice::Direct),
                _ => Err(ProxyRefused),
            }
        })
    };
    let scratch = ScratchDir::new("resolved-proxy").unwrap();
    let mut config = rig.fake.config(scratch.state_file());
    config.proxy = ProxyChoice::Resolve(resolver);
    let connectors = Connectors::new(config, Arc::new(MemoryStore::new())).unwrap();
    let handle = connectors.get("case-law").unwrap();
    assert_eq!(asked.load(Ordering::SeqCst), 0, "asked at start");
    let key = rig.fake.new_api_key();
    let reads = || rig.fake.requests_to("/v1/case-law/account").len();
    let before = reads();
    assert!(matches!(handle.add_api_key(&key), Err(Error::Network(_))));
    assert_eq!(reads(), before, "went around the dead proxy");
    mode.store(1, Ordering::SeqCst);
    handle.add_api_key(&key).unwrap();
    assert_eq!(reads(), before + 1);
    mode.store(2, Ordering::SeqCst);
    assert!(matches!(handle.add_api_key(&key), Err(Error::Network(_))));
    assert_eq!(reads(), before + 1, "a refused route went direct");
    assert_eq!(asked.load(Ordering::SeqCst), 3);
}

/// The price list's route lookup runs before its cache's lock: while one caller's lookup is
/// held, another caller's liveness check goes through.
#[test]
fn a_slow_route_lookup_does_not_hold_the_price_list() {
    let slow = SlowRoute::default();
    let resolver = slow.resolver();
    let rig = rig_with(FakeOptions::default(), false, move |config| {
        config.proxy = clauseo_connectors::ProxyChoice::Resolve(resolver);
    });
    let connectors = Arc::new(rig.connectors);
    let (entered, release) = slow.arm();
    let first = {
        let connectors = Arc::clone(&connectors);
        thread::spawn(move || connectors.liveness())
    };
    entered.recv().unwrap();
    let other = Arc::clone(&connectors);
    let liveness = ends_while_held("a liveness check", move || other.liveness());
    assert!(liveness.live);
    release.send(()).unwrap();
    assert!(first.join().unwrap().live);
}
