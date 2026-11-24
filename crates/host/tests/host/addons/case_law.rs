//! Case law in `ListAddons`: listed whatever its liveness check says, with the status its state
//! gives (signing in, signed out, out of credit, offered, not offered, no answer), and the
//! checks a read of the list starts: a liveness check at most every 30 s while it is not known
//! to be offered, a key and account check at most every 5 minutes while signed in, each one at a
//! time unless the one running started over 2 minutes ago. `AddonsChanged` follows a change of
//! status, never a check with the same answer.

use std::time::Duration;

use clauseo_connectors::fake::Fixture;
use clauseo_protocol::{
    Addon, AddonKind, AddonStatus, ConnectorCommand, HostCommand, HostEvent, ToolsPatch,
};

use super::{addons, addons_changed, drain, item};
use crate::connector_rig::{Gate, Rig, Start, add, add_key, refresh, sign_out, signed_in};
use crate::support::{Dir, Running, wait};

/// Case law's row, read until listed: it is left out until the start-up has read the
/// credential, and those reads start no check. The read that lists it may start one.
fn case_law_row(host: &Running) -> Addon {
    for _ in 0..4_000 {
        let items = addons(host);
        if let Some(row) = items.iter().find(|row| row.id == "case-law") {
            return row.clone();
        }
        wait("a pause", async_io::Timer::after(Duration::from_millis(5)));
    }
    panic!("case law never listed");
}

/// Case law's status in the list (a read, so it may start a check).
fn status(host: &Running) -> AddonStatus {
    case_law_row(host).status
}

/// The `pricing.json` fetches the fake has had.
fn fetches(rig: &Rig) -> usize {
    rig.fake.requests_to("/pricing.json").len()
}

/// Reads the list until `done`, pausing between reads: each read may start a check whose first
/// request the fake records off the actor.
fn read_until(rig: &Rig, what: &str, done: impl Fn(&Rig) -> bool) {
    for _ in 0..4_000 {
        if done(rig) {
            return;
        }
        let _ = addons(&rig.host);
        wait("a pause", async_io::Timer::after(Duration::from_millis(5)));
    }
    panic!("never {what}");
}

/// Waits without reading the list until the fake has had `count` `pricing.json` fetches.
fn fetched(rig: &Rig, count: usize) {
    wait("the pricing.json fetch", async {
        while fetches(rig) < count {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
}

fn down() -> Fixture {
    Fixture::Raw {
        status: 500,
        content_type: "text/plain".to_owned(),
        body: b"down".to_vec(),
    }
}

fn connectors_off() -> HostCommand {
    HostCommand::SetToolSettings {
        patch: ToolsPatch {
            connectors: Some(false),
            ..ToolsPatch::default()
        },
    }
}

/// Not checked yet, the first read shows `needs_setup` (no `unreachable` flash) and starts the
/// first check; it finds case law offered, the same status, so no `AddonsChanged` goes out.
#[test]
fn not_checked_yet_is_needs_setup_and_the_same_answer_sends_nothing() {
    let dir = Dir::new("addons_law_unchecked", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    drain(&rig.host, &mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::NeedsSetup);
    fetched(&rig, 1);
    rig.state_until("live", |state| !state.connectors.is_empty());
    let after = drain(&rig.host, &mut rig.sub);
    let events = rig
        .seen
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!events.contains("addons_changed"), "{events}");
    assert!(
        !after
            .iter()
            .any(|event| matches!(event, HostEvent::AddonsChanged {})),
        "{after:?}"
    );
    assert_eq!(status(&rig.host), AddonStatus::NeedsSetup);
}

/// No answer (a 500) is `unreachable`, with `AddonsChanged`; reads within 30 s of that check
/// start none; a read after 30 s starts one that finds it offered: `needs_setup`, with the event.
#[test]
fn no_answer_is_unreachable_and_checked_again_after_30_s() {
    let dir = Dir::new("addons_law_unreachable", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    rig.fake.set_pricing(down());
    drain(&rig.host, &mut rig.sub);
    let clock = rig.host.clock.clone();
    assert_eq!(status(&rig.host), AddonStatus::NeedsSetup);
    addons_changed(&mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::Unreachable);
    rig.fake.set_pricing(Fixture::File("pricing-live.json"));
    clock.advance(Duration::from_secs(29));
    for _ in 0..3 {
        assert_eq!(status(&rig.host), AddonStatus::Unreachable);
    }
    rig.host.settle();
    assert_eq!(fetches(&rig), 1);
    clock.advance(Duration::from_secs(2));
    read_until(&rig, "a second check", |rig| fetches(rig) >= 2);
    addons_changed(&mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::NeedsSetup);
    assert_eq!(fetches(&rig), 2);
}

/// A preview price list is `not_offered`, checked again no sooner than 30 s later.
#[test]
fn a_preview_price_list_is_not_offered_and_checked_at_most_every_30_s() {
    let dir = Dir::new("addons_law_not_offered", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    rig.fake.set_pricing(Fixture::File("pricing-preview.json"));
    drain(&rig.host, &mut rig.sub);
    let clock = rig.host.clock.clone();
    let _ = case_law_row(&rig.host);
    addons_changed(&mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::NotOffered);
    clock.advance(Duration::from_secs(29));
    for _ in 0..3 {
        assert_eq!(status(&rig.host), AddonStatus::NotOffered);
    }
    rig.host.settle();
    assert_eq!(fetches(&rig), 1);
    clock.advance(Duration::from_secs(2));
    read_until(&rig, "a second check", |rig| fetches(rig) >= 2);
    assert_eq!(status(&rig.host), AddonStatus::NotOffered);
}

/// A check whose answer never comes keeps any other from starting, until it started over
/// 2 minutes ago: then it counts as ended and a read starts the next. The connectors crate runs
/// one fetch at a time, so the next one's request reaches the fake once the held one is let go:
/// exactly one more, the one the read after 2 minutes started.
#[test]
fn a_check_with_no_result_counts_as_ended_after_two_minutes() {
    let dir = Dir::new("addons_law_stuck", &[]);
    let rig = Rig::start(&dir, Start::default());
    rig.fake.set_pricing(down());
    let release = rig.fake.hold_next("/pricing.json");
    let clock = rig.host.clock.clone();
    let _ = case_law_row(&rig.host);
    fetched(&rig, 1);
    // Past 30 s but within 2 minutes: the first still runs, so these reads start none.
    clock.advance(Duration::from_secs(31));
    for _ in 0..3 {
        let _ = addons(&rig.host);
    }
    clock.advance(Duration::from_secs(90));
    let _ = addons(&rig.host);
    rig.host.settle();
    release.cancel();
    fetched(&rig, 2);
    wait(
        "a pause",
        async_io::Timer::after(Duration::from_millis(300)),
    );
    assert_eq!(fetches(&rig), 2, "a check started while the first ran");
}

/// Signing in is `signing_in`; signed out is `signed_out`; both send `AddonsChanged`.
#[test]
fn signing_in_and_signed_out_have_their_own_statuses() {
    let dir = Dir::new("addons_law_signing_in", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    drain(&rig.host, &mut rig.sub);
    let _ = case_law_row(&rig.host);
    rig.ok(add());
    addons_changed(&mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::SigningIn);

    let dir = Dir::new("addons_law_signed_out", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    drain(&rig.host, &mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::Ready);
    rig.ok(sign_out());
    addons_changed(&mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::SignedOut);
    rig.finish();
}

/// Out of credit is `needs_credit`; paying the recharge brings it back to `ready`, each with
/// `AddonsChanged`.
#[test]
fn out_of_credit_is_needs_credit_until_paid() {
    let dir = Dir::new("addons_law_credit", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    drain(&rig.host, &mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::Ready);
    rig.host.clock.advance(Duration::from_secs(61));
    rig.fake.set_balance(0);
    rig.ok(refresh());
    addons_changed(&mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::NeedsCredit);
    let recharge = rig.fake.recharges().pop().expect("no recharge opened");
    assert!(rig.fake.mark_paid(&recharge));
    addons_changed(&mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::Ready);
    rig.finish();
}

/// Signed in, a read checks the key and the account at most every 5 minutes (and not within a
/// minute of the last account read): a key revoked on the server then reads `signed_out`
/// without a paid call.
#[test]
fn a_signed_in_read_checks_the_key_at_most_every_five_minutes() {
    let dir = Dir::new("addons_law_key_check", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let clock = rig.host.clock.clone();
    drain(&rig.host, &mut rig.sub);
    let base = rig.account_reads();
    // Within a minute of the start's read: the read slot is taken, so no check.
    assert_eq!(status(&rig.host), AddonStatus::Ready);
    rig.host.settle();
    assert_eq!(rig.account_reads(), base);
    clock.advance(Duration::from_secs(61));
    read_until(&rig, "the key check", |rig| rig.account_reads() > base);
    // Within 5 minutes of that check, however often the list is read: none.
    clock.advance(Duration::from_secs(120));
    for _ in 0..3 {
        assert_eq!(status(&rig.host), AddonStatus::Ready);
    }
    rig.host.settle();
    assert_eq!(rig.account_reads(), base + 1);
    rig.fake.revoke_all();
    clock.advance(Duration::from_secs(181));
    read_until(&rig, "the next key check", |rig| {
        rig.account_reads() > base + 1
    });
    addons_changed(&mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::SignedOut);
    assert_eq!(rig.account_reads(), base + 2);
}

/// With connectors switched off, the row is off and a read starts no check; a firm's lock on
/// `tools.connectors` shows as locked.
#[test]
fn switched_off_or_locked_case_law_starts_no_check() {
    let dir = Dir::new("addons_law_off", &[]);
    let rig = Rig::start(&dir, Start::default());
    rig.host.ok(connectors_off());
    let row = case_law_row(&rig.host);
    assert!(!row.on && !row.locked, "{row:?}");
    rig.host.settle();
    assert_eq!(fetches(&rig), 0);

    let dir = Dir::new("addons_law_locked", &[]);
    dir.set_policy(r#"{"tools": {"connectors": false}}"#);
    let rig = Rig::start(&dir, Start::default());
    let row = case_law_row(&rig.host);
    assert!(!row.on && row.locked, "{row:?}");
    rig.host.settle();
    assert_eq!(fetches(&rig), 0);
}

/// Case law is not `ready` until a key is added; connecting sends `AddonsChanged`, after
/// which it is `ready`.
#[test]
fn connecting_case_law_sends_addons_changed_and_reads_ready() {
    let dir = Dir::new("addons_law_connect", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    drain(&rig.host, &mut rig.sub);
    let before = case_law_row(&rig.host);
    assert_ne!(before.status, AddonStatus::Ready, "{before:?}");
    assert!(before.on, "{before:?}");
    rig.ok(ConnectorCommand::Check {});
    let key = rig.fake.new_api_key();
    rig.keys.push(key.clone());
    rig.ok(add_key(&key));
    rig.state_until("signed in", |state| signed_in(state).is_some());
    addons_changed(&mut rig.sub);
    assert_eq!(status(&rig.host), AddonStatus::Ready);
    rig.assert_no_secret_seen();
}

/// Before the start-up has read the credential (a keychain prompt still open) case law is left
/// out of the list; once read, one `AddonsChanged` adds it, signed in: `ready`.
#[test]
fn case_law_is_left_out_until_the_credential_is_read() {
    let dir = Dir::new("addons_law_held", &[]);
    let gate = std::sync::Arc::new(Gate::default());
    let mut rig = Rig::start_held(&dir, &gate);
    drain(&rig.host, &mut rig.sub);
    let items = addons(&rig.host);
    assert!(!items.iter().any(|row| row.id == "case-law"), "{items:?}");
    gate.open();
    addons_changed(&mut rig.sub);
    assert_eq!(
        item(&addons(&rig.host), AddonKind::Tool, "case-law").status,
        AddonStatus::Ready
    );
    let after = drain(&rig.host, &mut rig.sub);
    assert!(
        !after
            .iter()
            .any(|event| matches!(event, HostEvent::AddonsChanged {})),
        "{after:?}"
    );
}
