//! The install's `Ready` reaches the windows only once its pending press has run or its status
//! check has been asked, and the asked check marks the agent (`ready_checks`) until S18
//! answers. "Add `ChatGPT`" sends `StartSignIn` on that `Ready`; with Clauseo's own login on
//! disk (a copied data folder), it meets the mark and is dropped unasked, though S18 shows no
//! `Checking` yet. An `auth_required` recheck sets no such mark: the expired card's press asks.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use clauseo_agents::InstallState;
use clauseo_protocol::CommandOutcome;

use super::fakes::{Rig, rig};
use super::pending::{not_installed, pump, ready};
use super::switch_account::{asked, own_expired, sign_ins, start, switch};
use super::{MAIN, codex};
use crate::bridge::lock;
use crate::test_support::block_on;

/// Codex goes `Ready` over Clauseo's own login on disk, S18 having said nothing yet.
fn ready_over_own_login(rig: &Rig) {
    not_installed(rig, &codex());
    rig.agents.own_file.store(true, Ordering::SeqCst);
    pump(rig, &codex(), vec![InstallState::Installing, ready()]);
}

/// C2's release-candidate run: the window's `StartSignIn` on `Ready` comes before S18's
/// `Checking`. Nothing is asked and nothing starts.
#[test]
fn a_start_sent_on_ready_over_the_own_login_asks_nothing() {
    let rig = rig(true);
    ready_over_own_login(&rig);
    assert_eq!(asked(&rig, "check_status"), 1, "{:?}", rig.calls());
    let answer = block_on(rig.setup().run(MAIN, start(&codex())));
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(*lock(&rig.confirm.asked), [], "no confirm");
    assert_eq!(sign_ins(&rig), 0, "no codex login: {:?}", rig.calls());
}

/// A writer the capture layer writes every line into.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut *lock(&self.0), buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The `Ready` publish comes after the status check is asked, by host.log's own lines.
#[test]
fn the_ready_publish_comes_after_the_check_is_asked() {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let rig = rig(true);
    tracing::subscriber::with_default(subscriber, || ready_over_own_login(&rig));
    let text = String::from_utf8(lock(&lines.0).clone()).unwrap();
    let at = |needle: &str| {
        text.lines()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?} in {text}"))
    };
    let asked_at = at("sign-in status check asked");
    let ready_at = at(r#"install state published to the windows agent=codex-acp state="ready""#);
    assert!(asked_at < ready_at, "{text}");
}

/// The room's expired card: S18 says signed out over Clauseo's own login on disk, and an
/// `auth_required` recheck is going. "Sign in again" asks, as before.
#[test]
fn the_expired_card_s_press_during_a_recheck_still_asks() {
    let rig = rig(true);
    own_expired(&rig);
    rig.backend.recheck_sign_in(&codex());
    assert_eq!(asked(&rig, "check_status"), 1, "{:?}", rig.calls());
    let answer = block_on(rig.setup().run(MAIN, start(&codex())));
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(*lock(&rig.confirm.asked), [switch()]);
    assert_eq!(asked(&rig, "sign_in_switching"), 1, "{:?}", rig.calls());
}
