//! The block list's update path against a fake server on 127.0.0.1, with the fixtures of
//! `crates/blocklist/tests/data` (signed by its key A). Logs are not asserted: each test checks
//! the round's outcome, the list in use and the stored files.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use clauseo_blocklist::{Current, Generated, LIST_FILE, MAX_SIGNATURE, SIGNATURE_FILE};
use clauseo_protocol::Timestamp;
use clauseo_sandbox::Upstream;

use super::fetch::{Failure, check};
use super::timing::{MISMATCH_RETRY, next_wait};
use super::{
    BlockListSeams, BlockListUpdate, Fetching, Live, Round, Updater, block_list_is_stale, spawn,
};
use crate::clock::{Clock as _, ManualClock};
use crate::pipeline::tests::support::TempDir;
use crate::write;

mod needed;
mod server;
use server::{Reply, Server};

/// The list's time, and the newer and older lists'.
const LIST: &str = "2026-11-07T06:00:00Z";
const NEWER: &str = "2026-11-07T12:00:00Z";
const OLDER: &str = "2026-11-06T00:00:00Z";

/// A fixture's bytes.
fn fixture(name: &str) -> Vec<u8> {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../blocklist/tests/data");
    std::fs::read(data.join(name)).unwrap()
}

/// Key A's key line (its `.pub` file's second line).
fn key_a() -> String {
    let text = String::from_utf8(fixture("key-a.pub")).unwrap();
    text.lines().nth(1).unwrap().to_owned()
}

/// The fixture pair `name` (`list`, `list-newer`, ...).
fn pair(name: &str) -> (Vec<u8>, Vec<u8>) {
    (
        fixture(&format!("{name}.txt.gz")),
        fixture(&format!("{name}.txt.gz.minisig")),
    )
}

/// Writes the pair `name` into `folder` under the published names.
fn put(folder: &Path, name: &str) {
    write::create_dir_all(folder).unwrap();
    let (gz, signature) = pair(name);
    write::write_file(&folder.join(LIST_FILE), &gz).unwrap();
    write::write_file(&folder.join(SIGNATURE_FILE), &signature).unwrap();
}

/// The host clock's wall time at `text` (RFC 3339 UTC).
fn at(text: &str) -> Timestamp {
    Timestamp::new(Generated::parse(text).unwrap().unix_seconds() * 1000).unwrap()
}

/// Updates on, no proxy setting.
fn on() -> Fetching {
    Fetching {
        updates: true,
        upstream_proxy: None,
    }
}

/// A data folder, a bundled folder, the shared list, the server, the settings each round reads
/// and the host clock (an hour after the newer list was made).
struct Rig {
    dir: TempDir,
    current: Arc<Current>,
    server: Server,
    live: Live,
    clock: ManualClock,
}

impl Rig {
    fn new(test: &str) -> Rig {
        Rig {
            dir: TempDir::new(&format!("blocklist-{test}")),
            current: Arc::new(Current::default()),
            server: Server::start(),
            live: Live::new(on()),
            clock: ManualClock::new(at("2026-11-07T13:00:00Z")),
        }
    }

    fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    fn bundled(&self) -> PathBuf {
        self.dir.path().join("bundled")
    }

    fn stored(&self) -> PathBuf {
        self.data_dir().join("blocklist")
    }

    /// The settings pointing at the server, plain http, `127.0.0.1` allowed, key A.
    fn update(&self) -> BlockListUpdate {
        BlockListUpdate {
            list_url: self.server.url("/list.gz"),
            signature_url: self.server.url("/list.gz.minisig"),
            allowed_hosts: vec!["127.0.0.1".to_owned()],
            https_only: false,
            upstream: Upstream::Direct,
            interval: Duration::from_hours(1),
            max_list: 64 * 1024,
            max_signature: u64::try_from(MAX_SIGNATURE).unwrap(),
            keys: vec![key_a()],
        }
    }

    /// The server publishes `gz` and `signature`.
    fn publish(&self, (gz, signature): (Vec<u8>, Vec<u8>)) {
        self.server.set("/list.gz", Reply::Body(gz));
        self.server.set("/list.gz.minisig", Reply::Body(signature));
    }

    /// An updater with `update`, its start-up load done.
    fn started(&self, update: BlockListUpdate) -> Updater {
        let updater = self.updater(update);
        updater.load_at_start();
        updater
    }

    /// An updater with `update`, nothing loaded yet.
    fn updater(&self, update: BlockListUpdate) -> Updater {
        let seams = BlockListSeams::new(Arc::clone(&self.current), Some(self.bundled()), update);
        let clock = Arc::new(self.clock.clone());
        Updater::new(seams, &self.data_dir(), self.live.clone(), clock)
    }

    /// The time of the list in use.
    fn in_use(&self) -> Option<String> {
        self.current
            .generated()
            .map(|time| time.as_str().to_owned())
    }

    /// The stored `.gz`, if any.
    fn stored_gz(&self) -> Option<Vec<u8>> {
        std::fs::read(self.stored().join(LIST_FILE)).ok()
    }
}

#[test]
fn a_newer_list_is_fetched_stored_and_swapped_in() {
    let rig = Rig::new("newer");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let updater = rig.started(rig.update());
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
    assert_eq!(updater.run_once(&|| false), Round::Applied);
    assert_eq!(rig.in_use().as_deref(), Some(NEWER));
    assert!(rig.current.get().lookup("newer.test").is_some());
    let (gz, signature) = pair("list-newer");
    assert_eq!(rig.stored_gz(), Some(gz));
    let stored_signature = std::fs::read(rig.stored().join(SIGNATURE_FILE)).unwrap();
    assert_eq!(stored_signature, signature);
}

#[test]
fn a_redirect_to_an_allowed_host_is_followed() {
    let rig = Rig::new("redirect-ok");
    put(&rig.bundled(), "list");
    let (gz, signature) = pair("list-newer");
    // An absolute Location for the list, a relative one (two hops) for the signature.
    rig.server
        .set("/list.gz", Reply::Redirect(rig.server.url("/assets/1")));
    rig.server.set("/assets/1", Reply::Body(gz));
    rig.server
        .set("/list.gz.minisig", Reply::Redirect("/hop".to_owned()));
    rig.server
        .set("/hop", Reply::Redirect("assets/2".to_owned()));
    rig.server.set("/assets/2", Reply::Body(signature));
    let updater = rig.started(rig.update());
    assert_eq!(updater.run_once(&|| false), Round::Applied);
    assert_eq!(rig.in_use().as_deref(), Some(NEWER));
}

#[test]
fn a_redirect_to_a_host_not_allowed_is_refused_and_the_list_kept() {
    let rig = Rig::new("redirect-refused");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let elsewhere = format!("http://localhost:{}/list.gz", rig.server.port());
    rig.server
        .set("/list.gz.minisig", Reply::Redirect(elsewhere));
    let updater = rig.started(rig.update());
    assert_eq!(updater.run_once(&|| false), Round::Failed);
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
    assert_eq!(rig.stored_gz(), None);
}

#[test]
fn too_many_redirects_are_refused() {
    let rig = Rig::new("redirect-loop");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    rig.server
        .set("/list.gz", Reply::Redirect("/list.gz".to_owned()));
    let updater = rig.started(rig.update());
    assert_eq!(updater.run_once(&|| false), Round::Failed);
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
}

#[test]
fn a_body_over_the_cap_is_refused_and_the_list_kept() {
    let rig = Rig::new("over-cap");
    put(&rig.bundled(), "list");
    let (gz, signature) = pair("list-newer");
    let mut update = rig.update();
    update.max_list = u64::try_from(gz.len()).unwrap() - 1;
    rig.publish((gz, signature));
    let updater = rig.started(update);
    assert_eq!(updater.run_once(&|| false), Round::Failed);
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
    assert_eq!(rig.stored_gz(), None);
}

#[test]
fn a_404_on_either_file_keeps_the_list() {
    let rig = Rig::new("not-published");
    put(&rig.bundled(), "list");
    let updater = rig.started(rig.update());
    let (gz, signature) = pair("list-newer");
    // The list missing, then the signature missing.
    rig.server
        .set("/list.gz.minisig", Reply::Body(signature.clone()));
    assert_eq!(updater.run_once(&|| false), Round::NotPublished);
    rig.server.set("/list.gz", Reply::Body(gz));
    rig.server.set("/list.gz.minisig", Reply::Status(404));
    assert_eq!(updater.run_once(&|| false), Round::NotPublished);
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
    assert_eq!(rig.stored_gz(), None);
    // Published at last: the next round takes it.
    rig.server.set("/list.gz.minisig", Reply::Body(signature));
    assert_eq!(updater.run_once(&|| false), Round::Applied);
}

#[test]
fn a_new_gz_beside_the_old_signature_is_refused() {
    let rig = Rig::new("half-replaced");
    put(&rig.bundled(), "list");
    let (newer_gz, _) = pair("list-newer");
    let (_, old_signature) = pair("list");
    rig.publish((newer_gz, old_signature));
    let updater = rig.started(rig.update());
    assert_eq!(updater.run_once(&|| false), Round::Mismatch);
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
    assert_eq!(rig.stored_gz(), None);
}

#[test]
fn a_failed_fetch_keeps_the_stored_copy() {
    let rig = Rig::new("keeps-stored");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let updater = rig.started(rig.update());
    assert_eq!(updater.run_once(&|| false), Round::Applied);
    rig.server.set("/list.gz", Reply::Status(500));
    assert_eq!(updater.run_once(&|| false), Round::Failed);
    assert_eq!(rig.stored_gz(), Some(pair("list-newer").0));
    assert_eq!(rig.in_use().as_deref(), Some(NEWER));
}

#[test]
fn an_older_or_equal_verified_list_is_not_applied() {
    let rig = Rig::new("older");
    put(&rig.bundled(), "list");
    let updater = rig.started(rig.update());
    rig.publish(pair("list-older"));
    assert_eq!(updater.run_once(&|| false), Round::NotNewer);
    rig.publish(pair("list"));
    assert_eq!(updater.run_once(&|| false), Round::NotNewer);
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
    assert_eq!(rig.stored_gz(), None);
}

#[test]
fn a_round_that_sees_the_stop_applies_and_writes_nothing() {
    let rig = Rig::new("stopped");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let updater = rig.started(rig.update());
    assert_eq!(updater.run_once(&|| true), Round::Stopped);
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
    assert_eq!(rig.stored_gz(), None);
}

#[test]
fn https_only_refuses_an_explicit_port() {
    let policy = BlockListUpdate::release();
    let url = |text: &str| url::Url::parse(text).unwrap();
    assert_eq!(check(&url("https://github.com/a"), &policy), Ok(()));
    // 443 written out parses as no port, so it passes too.
    assert_eq!(check(&url("https://github.com:443/a"), &policy), Ok(()));
    let refused = Err(Failure::Refused("a URL with a port".to_owned()));
    assert_eq!(check(&url("https://github.com:8443/a"), &policy), refused);
    assert!(check(&url("http://github.com/a"), &policy).is_err());
    assert!(check(&url("https://evil.example/a"), &policy).is_err());
}

#[test]
fn start_up_takes_a_stored_list_newer_than_the_bundled_one() {
    let rig = Rig::new("start-stored");
    put(&rig.bundled(), "list");
    put(&rig.stored(), "list-newer");
    rig.started(rig.update());
    assert_eq!(rig.in_use().as_deref(), Some(NEWER));
}

#[test]
fn start_up_takes_a_bundled_list_newer_than_the_stored_one() {
    let rig = Rig::new("start-bundled");
    put(&rig.bundled(), "list");
    put(&rig.stored(), "list-older");
    rig.started(rig.update());
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
}

#[test]
fn start_up_ignores_a_tampered_stored_copy() {
    let rig = Rig::new("start-tampered");
    put(&rig.bundled(), "list");
    put(&rig.stored(), "list-newer");
    let mut gz = pair("list-newer").0;
    let last = gz.last_mut().unwrap();
    *last ^= 1;
    write::write_file(&rig.stored().join(LIST_FILE), &gz).unwrap();
    rig.started(rig.update());
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
}

#[test]
fn start_up_with_neither_stays_unloaded() {
    let rig = Rig::new("start-none");
    rig.started(rig.update());
    assert!(rig.current.get().is_unloaded());
    assert_eq!(rig.in_use(), None);
}

#[test]
fn the_thread_loads_fetches_and_stops_when_dropped() {
    let rig = Rig::new("thread");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let (rounds, seen) = mpsc::channel();
    let running = spawn(rig.updater(rig.update()), move |round| {
        let _sent = rounds.send(round);
    })
    .unwrap();
    // The first fetch follows the start-up load at once; the next waits about the hour.
    assert_eq!(
        seen.recv_timeout(Duration::from_secs(30)),
        Ok(Round::Applied)
    );
    assert_eq!(rig.in_use().as_deref(), Some(NEWER));
    drop(running);
    // The stop cuts the hour's wait short: the thread ends and drops its sender.
    assert_eq!(
        seen.recv_timeout(Duration::from_secs(30)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}

#[test]
fn updates_off_skip_the_next_round_and_on_again_resumes() {
    let rig = Rig::new("off");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let updater = rig.started(rig.update());
    rig.live.set(Fetching {
        updates: false,
        ..on()
    });
    assert_eq!(updater.run_once(&|| false), Round::Off);
    assert_eq!(updater.run_once(&|| false), Round::Off);
    assert_eq!(rig.server.hits(), Vec::<String>::new());
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
    rig.live.set(on());
    assert_eq!(updater.run_once(&|| false), Round::Applied);
    assert_eq!(rig.in_use().as_deref(), Some(NEWER));
}

#[test]
fn an_upstream_proxy_setting_routes_the_fetch_through_it() {
    let rig = Rig::new("proxied");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    let proxy = Server::proxy(&rig.server);
    rig.live.set(Fetching {
        upstream_proxy: Some(proxy.url("")),
        ..on()
    });
    let updater = rig.started(rig.update());
    assert_eq!(updater.run_once(&|| false), Round::Applied);
    let target = format!("CONNECT 127.0.0.1:{}", rig.server.port());
    let hits = proxy.hits();
    assert_eq!(
        hits.iter().filter(|hit| **hit == target).count(),
        2,
        "{hits:?}"
    );
    assert_eq!(rig.server.hits(), Vec::<String>::new());
}

#[test]
fn a_refused_route_fails_the_round_without_a_request() {
    let rig = Rig::new("route-refused");
    put(&rig.bundled(), "list");
    rig.publish(pair("list-newer"));
    // A proxy URL with a password is refused (Clauseo keeps no proxy password).
    let refused = format!("http://user:secret@127.0.0.1:{}", rig.server.port());
    rig.live.set(Fetching {
        upstream_proxy: Some(refused),
        ..on()
    });
    let updater = rig.started(rig.update());
    assert_eq!(updater.run_once(&|| false), Round::Failed);
    assert_eq!(rig.server.hits(), Vec::<String>::new());
    assert_eq!(rig.in_use().as_deref(), Some(LIST));
}

/// The rig with its clock at the older fixture's time, so `list` and `list-newer` are dated
/// over a day ahead (30 h and 36 h).
fn rig_at_older(test: &str) -> Rig {
    Rig {
        clock: ManualClock::new(at(OLDER)),
        ..Rig::new(test)
    }
}

#[test]
fn a_list_dated_over_a_day_ahead_of_the_clock_is_refused() {
    let rig = rig_at_older("future");
    put(&rig.bundled(), "list-older");
    rig.publish(pair("list-newer"));
    let updater = rig.started(rig.update());
    assert_eq!(updater.run_once(&|| false), Round::Failed);
    assert_eq!(rig.in_use().as_deref(), Some(OLDER));
    assert_eq!(rig.stored_gz(), None);
}

#[test]
fn start_up_sets_aside_a_stored_copy_dated_over_a_day_ahead() {
    let rig = rig_at_older("future-stored");
    put(&rig.bundled(), "list-older");
    put(&rig.stored(), "list-newer");
    rig.started(rig.update());
    assert_eq!(rig.in_use().as_deref(), Some(OLDER));
}

#[test]
fn start_up_sets_aside_a_bundled_copy_dated_over_a_day_ahead_and_updates_still_apply() {
    let rig = rig_at_older("future-bundled");
    put(&rig.bundled(), "list-newer");
    rig.publish(pair("list-older"));
    let updater = rig.started(rig.update());
    assert!(rig.current.get().is_unloaded());
    // The set-aside snapshot's time is no floor: an older, sane list still goes into use.
    assert_eq!(updater.run_once(&|| false), Round::Applied);
    assert_eq!(rig.in_use().as_deref(), Some(OLDER));
}

#[test]
fn a_list_over_three_days_old_is_stale_and_logged_once_a_day() {
    let rig = Rig::new("stale");
    put(&rig.bundled(), "list");
    let rig = Rig {
        clock: ManualClock::new(at("2026-11-10T05:00:00Z")),
        ..rig
    };
    let updater = rig.started(rig.update());
    // 71 hours after 06:00 on the 7th: not stale.
    assert!(!block_list_is_stale(&rig.current, rig.clock.now().wall));
    assert_eq!(updater.run_once(&|| false), Round::NotPublished);
    assert_eq!(updater.stale_noted(), None);
    // 73 hours: stale, and logged.
    rig.clock.advance(Duration::from_hours(2));
    assert!(block_list_is_stale(&rig.current, rig.clock.now().wall));
    updater.run_once(&|| false);
    let first = updater.stale_noted().unwrap();
    // Six hours on: not logged again.
    rig.clock.advance(Duration::from_hours(6));
    updater.run_once(&|| false);
    assert_eq!(updater.stale_noted(), Some(first));
    // A day after the first: logged again.
    rig.clock.advance(Duration::from_hours(18));
    updater.run_once(&|| false);
    assert_eq!(updater.stale_noted(), Some(first + super::DAY));
}

#[test]
fn the_wait_is_jittered_and_a_mismatch_is_retried_once_soon() {
    let six = Duration::from_hours(6);
    let half_hour = Duration::from_mins(30);
    let shortest = Duration::from_mins(330);
    assert_eq!(next_wait(six, Round::Applied, false, 0), (shortest, false));
    let span = u64::try_from((half_hour * 2).as_millis()).unwrap();
    assert_eq!(
        next_wait(six, Round::Failed, false, span),
        (six + half_hour, false)
    );
    assert_eq!(
        next_wait(six, Round::NotPublished, false, span / 2),
        (six, false)
    );
    // A mismatch waits about the publish window once; a second one waits the interval.
    assert_eq!(
        next_wait(six, Round::Mismatch, false, 7),
        (MISMATCH_RETRY, true)
    );
    assert_eq!(next_wait(six, Round::Mismatch, true, 0), (shortest, false));
    assert_eq!(next_wait(six, Round::Applied, true, 0), (shortest, false));
}
