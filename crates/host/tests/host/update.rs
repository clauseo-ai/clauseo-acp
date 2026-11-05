//! The updater wired into the host (decision record 0037, R23 PR 4), driven through a fake
//! [`Updater`] and the manual clock: a managed run never checks, the first check comes 1 to
//! 10 minutes after start and then every 50 to 70, `CheckForUpdate` downloads and offers,
//! `SetUpdateMode` writes `settings.json` unless policy locks it, `update.json` says at the next
//! start how the install went, and last run's download installs at start in `auto` only.
//! `update_install.rs` has the installs that wait on turns. Every test that writes
//! `update.json` also runs the file thread's assertion that the actor never touches it.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use clauseo_counts::Counts;
use clauseo_counts::schema::{
    AppOpened, Launch, SettingChanged, Update, UpdateMode as CountedMode, UpdateResult,
};
use clauseo_host::{
    Downloaded, HostConfig, HostExit, InstallKind, Offer, Opaque, Updater, UpdaterError,
};
use clauseo_protocol::{
    CommandOutcome, ErrorCode, FailedStage, HostCommand, HostEvent, HostQuery, InstallWhen,
    ManagedBy, QueryResult, UpdateMode, UpdateState, UpdateView,
};
use futures::future::BoxFuture;

use crate::counts::open_counts;
use crate::settings::{ready, subscribe_app};
use crate::support::{Dir, Running, wait};
use crate::write;

/// The running app's version in these tests.
pub(crate) const CURRENT: &str = "0.4.0";
/// The version the fake offers.
pub(crate) const NEXT: &str = "0.4.1";
/// The timers a host keeps armed with nothing to do: the updater's one, which under the default
/// `NoUpdater` (managed by a package) counts `managed` again a day later (decision record 0037).
pub(crate) const STANDING_TIMERS: usize = 1;

/// Every test's jitter seed.
const SEED: u64 = 7;
const MINUTE: Duration = Duration::from_secs(60);

/// How the fake's downloads go.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Download {
    /// Done at once, with one progress value.
    Instant,
    /// Never done; dropping it sets [`FakeUpdater::dropped`].
    Hang,
    /// Progress posted flat out from a thread of its own until [`FakeUpdater::stop_flood`].
    Flood,
}

/// An [`Updater`] that offers [`NEXT`] (or nothing), counts its calls and downloads as told.
pub(crate) struct FakeUpdater {
    kind: InstallKind,
    offer: Option<&'static str>,
    download: Download,
    updates: PathBuf,
    pub(crate) checks: AtomicUsize,
    pub(crate) downloads: AtomicUsize,
    pub(crate) dropped: Arc<AtomicBool>,
    pub(crate) stop_flood: Arc<AtomicBool>,
}

impl FakeUpdater {
    /// A fake for `dir` installed as `kind`, offering `offer`, downloading as `download`.
    pub(crate) fn new(
        dir: &Dir,
        kind: InstallKind,
        offer: Option<&'static str>,
        download: Download,
    ) -> Arc<Self> {
        Arc::new(Self {
            kind,
            offer,
            download,
            updates: dir.data().join("updates"),
            checks: AtomicUsize::new(0),
            downloads: AtomicUsize::new(0),
            dropped: Arc::new(AtomicBool::new(false)),
            stop_flood: Arc::new(AtomicBool::new(false)),
        })
    }

    /// A fake that updates itself and offers [`NEXT`], downloaded at once.
    pub(crate) fn offering(dir: &Dir) -> Arc<Self> {
        Self::new(dir, InstallKind::CurrentUser, Some(NEXT), Download::Instant)
    }

    pub(crate) fn checks(&self) -> usize {
        self.checks.load(Ordering::SeqCst)
    }

    pub(crate) fn downloads(&self) -> usize {
        self.downloads.load(Ordering::SeqCst)
    }
}

/// Sets its flag when dropped.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl Updater for FakeUpdater {
    fn install_kind(&self) -> InstallKind {
        self.kind
    }

    fn check(&self) -> BoxFuture<'static, Result<Option<Offer>, UpdaterError>> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        let offer = self.offer.map(|version| Offer {
            version: version.to_owned(),
            handle: Opaque::new(version),
        });
        Box::pin(std::future::ready(Ok(offer)))
    }

    fn download(
        &self,
        offer: Offer,
        mut progress: clauseo_host::ProgressSink,
    ) -> BoxFuture<'static, Result<Downloaded, UpdaterError>> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        let done = Downloaded {
            path: self.updates.join(&offer.version).join("Clauseo.exe"),
            version: offer.version,
            handle: offer.handle,
        };
        match self.download {
            Download::Instant => {
                progress.post(10, Some(10));
                Box::pin(std::future::ready(Ok(done)))
            }
            Download::Hang => {
                let guard = DropFlag(Arc::clone(&self.dropped));
                Box::pin(async move {
                    let _guard = guard;
                    futures::future::pending::<Result<Downloaded, UpdaterError>>().await
                })
            }
            Download::Flood => {
                let (tx, rx) = futures::channel::oneshot::channel();
                let stop = Arc::clone(&self.stop_flood);
                std::thread::Builder::new()
                    .name("fake-update-flood".to_owned())
                    .spawn(move || {
                        let mut received = 0_u64;
                        // Capped, so a test that fails before it stops the flood ends anyway.
                        while !stop.load(Ordering::SeqCst) && received < 50_000_000 {
                            received += 1;
                            progress.post(received, Some(u64::MAX));
                        }
                        let _gone = tx.send(done);
                    })
                    .unwrap();
                Box::pin(async move {
                    rx.await
                        .map_err(|_gone| UpdaterError::Other("the flood thread is gone".to_owned()))
                })
            }
        }
    }
}

/// Starts run `run` of `dir` as `version` with `fake` as its updater, `extra` changing the
/// config last.
pub(crate) fn start_updating_with(
    dir: &Dir,
    run: u16,
    fake: &Arc<FakeUpdater>,
    version: &str,
    extra: impl FnOnce(HostConfig) -> HostConfig,
) -> Running {
    let updater: Arc<dyn Updater> = Arc::<FakeUpdater>::clone(fake);
    let version = version.to_owned();
    dir.start_with(run, move |config| {
        extra(
            config
                .with_updater(updater)
                .with_update_jitter_seed(SEED)
                .with_app_version(version),
        )
    })
}

/// As [`start_updating_with`], running [`CURRENT`] with nothing else changed.
pub(crate) fn start_updating(dir: &Dir, run: u16, fake: &Arc<FakeUpdater>) -> Running {
    start_updating_with(dir, run, fake, CURRENT, |config| config)
}

/// What `GetUpdate` answers.
pub(crate) fn view(host: &Running) -> UpdateView {
    match host.query(HostQuery::GetUpdate).unwrap() {
        QueryResult::Update(view) => view,
        other => panic!("GetUpdate answered {other:?}"),
    }
}

/// Asks `GetUpdate` until `done` holds, in real time up to the watchdog.
pub(crate) fn until_view(
    host: &Running,
    what: &str,
    done: impl Fn(&UpdateView) -> bool,
) -> UpdateView {
    for _ in 0..2_000 {
        let view = view(host);
        if done(&view) {
            return view;
        }
        wait("a pause", async_io::Timer::after(Duration::from_millis(10)));
    }
    panic!("the update view never became {what}: {:?}", view(host));
}

/// Waits until `ready { NEXT }`.
pub(crate) fn until_ready(host: &Running) {
    let ready = UpdateState::Ready {
        version: NEXT.to_owned(),
    };
    until_view(host, "ready", |view| view.state == ready);
}

/// The next delay armed between `low` and `high`, passing over the host's other timers.
fn armed_between(host: &Running, low: Duration, high: Duration) -> Duration {
    let clock = host.clock.clone();
    wait("an update timer", async move {
        loop {
            let delay = clock.next_armed().await;
            if (low..=high).contains(&delay) {
                return delay;
            }
        }
    })
}

/// How `host` ended, once it has, with its thread joined.
pub(crate) fn exit_of(mut host: Running) -> HostExit {
    let exit = wait("the host's exit", host.handle.exit());
    host.take_join()
        .join()
        .unwrap_or_else(|e| panic!("join: {e}"));
    exit
}

/// Asserts `exit` installs [`NEXT`] from `path`.
pub(crate) fn assert_installs(exit: &HostExit, path: &std::path::Path) {
    match exit {
        HostExit::Install(downloaded) => {
            assert_eq!(downloaded.version, NEXT);
            assert_eq!(downloaded.path, path);
        }
        other => panic!("the host ended {other:?}, not with an install"),
    }
}

/// `update.json`'s text.
pub(crate) fn marker_text(dir: &Dir) -> Option<String> {
    std::fs::read_to_string(dir.data().join("update.json")).ok()
}

/// The marker an install of [`NEXT`] from [`CURRENT`] writes.
pub(crate) const INSTALLING: &str = r#"{"from":"0.4.0","to":"0.4.1","state":"installing"}"#;

/// A count's cells, as `(key, n)`.
fn cells<K: Copy>(cells: &[clauseo_counts::schema::Cell<K>]) -> Vec<(K, u64)> {
    cells
        .iter()
        .map(|cell| (cell.key, u64::from(cell.n)))
        .collect()
}

fn counting(counts: &Counts) -> impl FnOnce(HostConfig) -> HostConfig {
    let counts = counts.clone();
    move |config| config.with_counts(counts)
}

#[test]
fn a_managed_run_never_checks() {
    let dir = Dir::new("update_managed", &[]);
    let kinds = [
        (InstallKind::PerMachine, None, ManagedBy::PerMachine),
        (
            InstallKind::ReadOnlyLocation,
            None,
            ManagedBy::ReadOnlyLocation,
        ),
        (InstallKind::Package, None, ManagedBy::Package),
        (
            InstallKind::CurrentUser,
            Some(r#"{"update": {"mode": "none"}}"#),
            ManagedBy::Policy,
        ),
        // A policy file that cannot be read fails closed: `none`, locked.
        (
            InstallKind::CurrentUser,
            Some("{ not json"),
            ManagedBy::Policy,
        ),
    ];
    for (run, (kind, policy, by)) in (0_u16..).zip(kinds) {
        dir.set_policy(policy.unwrap_or("{}"));
        let fake = FakeUpdater::new(&dir, kind, Some(NEXT), Download::Instant);
        let host = start_updating(&dir, run, &fake);
        assert_eq!(view(&host).state, UpdateState::Managed { by }, "{kind:?}");
        for _ in 0..6 {
            host.clock.advance(60 * MINUTE);
            host.settle();
        }
        assert_eq!(fake.checks(), 0, "{kind:?} {policy:?}");
        host.refused(HostCommand::CheckForUpdate {}, ErrorCode::CommandInvalid);
        host.shutdown();
    }
}

#[test]
fn the_first_check_comes_within_ten_minutes_then_about_hourly() {
    let dir = Dir::new("update_timing", &[]);
    let fake = FakeUpdater::new(&dir, InstallKind::CurrentUser, None, Download::Instant);
    let host = start_updating(&dir, 0, &fake);
    let first = armed_between(&host, MINUTE, 10 * MINUTE);
    host.settle();
    assert_eq!(fake.checks(), 0);
    assert_eq!(view(&host).state, UpdateState::Idle);
    host.clock.advance(first);
    let checked = until_view(&host, "checked", |view| view.checked_at.is_some());
    assert_eq!(checked.state, UpdateState::Idle);
    assert_eq!(fake.checks(), 1);
    let next = armed_between(&host, 50 * MINUTE, 70 * MINUTE);
    host.clock.advance(next);
    until_view(&host, "checked again", |view| {
        view.checked_at != checked.checked_at
    });
    assert_eq!(fake.checks(), 2);
    host.shutdown();
}

#[test]
fn check_for_update_downloads_and_tells_the_windows() {
    let dir = Dir::new("update_check", &[]);
    let fake = FakeUpdater::offering(&dir);
    let host = start_updating(&dir, 0, &fake);
    let (mut sub, _first) = subscribe_app(&host);
    assert_eq!(
        host.ok(HostCommand::CheckForUpdate {}),
        CommandOutcome::Done
    );
    until_ready(&host);
    assert_eq!((fake.checks(), fake.downloads()), (1, 1));
    host.settle();
    let states: Vec<UpdateState> = ready(&mut sub)
        .into_iter()
        .filter_map(|event| match event {
            HostEvent::UpdateChanged(view) => Some(view.state),
            _ => None,
        })
        .collect();
    assert_eq!(states.first(), Some(&UpdateState::Checking), "{states:?}");
    assert_eq!(
        states.last(),
        Some(&UpdateState::Ready {
            version: NEXT.to_owned()
        }),
        "{states:?}"
    );
    // `ready` takes no second check.
    host.refused(HostCommand::CheckForUpdate {}, ErrorCode::CommandInvalid);
    host.refused(
        HostCommand::CancelScheduledUpdate {},
        ErrorCode::CommandInvalid,
    );
    host.shutdown();
}

#[test]
fn set_update_mode_writes_the_setting_unless_policy_locks_it() {
    let dir = Dir::new("update_mode", &[]);
    let counts = open_counts(&dir, true);
    let fake = FakeUpdater::new(&dir, InstallKind::CurrentUser, None, Download::Instant);
    let host = start_updating_with(&dir, 0, &fake, CURRENT, counting(&counts));
    let (mut sub, _first) = subscribe_app(&host);
    let set = |mode| HostCommand::SetUpdateMode { mode };
    assert_eq!(host.ok(set(UpdateMode::Notify)), CommandOutcome::Done);
    assert_eq!(view(&host).mode, UpdateMode::Notify);
    assert!(
        dir.settings_json().contains("\"notify\""),
        "{}",
        dir.settings_json()
    );
    host.settle();
    let changed = ready(&mut sub);
    assert!(
        changed
            .iter()
            .any(|e| matches!(e, HostEvent::SettingsChanged(_))),
        "{changed:?}"
    );
    host.refused(set(UpdateMode::None), ErrorCode::CommandInvalid);
    host.settle();
    assert_eq!(
        cells(&counts.today().setting_changed),
        [(SettingChanged::UpdateMode(CountedMode::Notify), 1)]
    );
    host.shutdown();

    let before = dir.settings_json();
    dir.set_policy(r#"{"update": {"mode": "notify"}}"#);
    let host = start_updating(&dir, 1, &fake);
    let error = host.refused(set(UpdateMode::Auto), ErrorCode::SettingsLocked);
    assert!(error.message().contains("update.mode"), "{error}");
    assert_eq!(dir.settings_json(), before);
    assert_eq!(view(&host).mode, UpdateMode::Notify);
    host.shutdown();
}

#[test]
fn the_marker_says_at_the_next_start_how_the_install_went() {
    let dir = Dir::new("update_marker", &[]);
    let counts = open_counts(&dir, true);
    let fake = FakeUpdater::new(&dir, InstallKind::CurrentUser, None, Download::Instant);
    let marker = dir.data().join("update.json");

    // It took: this run is the version the marker names.
    write::write(&marker, INSTALLING.as_bytes());
    let host = start_updating_with(&dir, 0, &fake, NEXT, counting(&counts));
    assert_eq!(view(&host).updated_from.as_deref(), Some(CURRENT));
    until_cleared(&host, &dir);
    host.settle();
    let today = counts.today();
    let after = AppOpened {
        launch: Launch::AfterUpdate,
    };
    assert_eq!(cells(&today.app_opened), [(after, 1)]);
    let installed = Update {
        result: UpdateResult::Installed,
    };
    assert_eq!(cells(&today.update), [(installed, 1)]);
    host.shutdown();

    // The next start has no marker: nothing counted twice.
    let host = start_updating_with(&dir, 1, &fake, NEXT, counting(&counts));
    assert_eq!(view(&host).updated_from, None);
    host.settle();
    assert_eq!(cells(&counts.today().update), [(installed, 1)]);
    host.shutdown();

    // It failed: this run is still the version it came from.
    write::write(&marker, INSTALLING.as_bytes());
    let host = start_updating_with(&dir, 2, &fake, CURRENT, counting(&counts));
    let failed = view(&host);
    assert!(
        matches!(
            &failed.state,
            UpdateState::Failed { version: Some(v), stage: FailedStage::Install, .. } if v == NEXT
        ),
        "{failed:?}"
    );
    until_cleared(&host, &dir);
    host.settle();
    let failure = Update {
        result: UpdateResult::Failed,
    };
    assert_eq!(
        cells(&counts.today().update),
        [(installed, 1), (failure, 1)]
    );
    host.shutdown();
}

/// Waits until `update.json` is gone (the file thread deletes it after start-up).
fn until_cleared(host: &Running, dir: &Dir) {
    for _ in 0..2_000 {
        if marker_text(dir).is_none() {
            return;
        }
        host.settle();
        wait("a pause", async_io::Timer::after(Duration::from_millis(10)));
    }
    panic!("update.json was never deleted");
}

#[test]
fn last_runs_download_installs_at_start_in_auto_only() {
    let dir = Dir::new("update_cached", &[]);
    let cached = dir.data().join("updates").join(NEXT).join("Clauseo.exe");
    write::write(&cached, b"installer");
    let fake = FakeUpdater::offering(&dir);

    // `notify`: nothing at start; the first check makes it `ready` with no second download.
    dir.set_settings(&[], serde_json::json!({ "update": { "mode": "notify" } }));
    let host = start_updating(&dir, 0, &fake);
    let first = armed_between(&host, MINUTE, 10 * MINUTE);
    host.settle();
    assert_eq!((fake.checks(), view(&host).state), (0, UpdateState::Idle));
    host.clock.advance(first);
    until_ready(&host);
    assert_eq!(fake.downloads(), 0);
    // `ready` stays so in `notify` until the lawyer says to install.
    host.clock.advance(60 * MINUTE);
    host.settle();
    assert_eq!(
        view(&host).state,
        UpdateState::Ready {
            version: NEXT.to_owned()
        }
    );
    assert_eq!(marker_text(&dir), None);
    host.ok(HostCommand::InstallUpdate {
        when: InstallWhen::AfterTurn,
    });
    assert_installs(&exit_of(host), &cached);
    assert_eq!(marker_text(&dir).as_deref(), Some(INSTALLING));

    // A host that only stops is `Stopped`.
    write::remove_file(&dir.data().join("update.json"));
    let host = start_updating(&dir, 1, &fake);
    wait("shutdown", host.handle.shutdown()).unwrap();
    assert_eq!(exit_of(host), HostExit::Stopped);

    // `auto`: checked at once, the same version is offered, and the host stops to install it.
    dir.set_settings(&[], serde_json::json!({}));
    let host = start_updating(&dir, 2, &fake);
    assert_installs(&exit_of(host), &cached);
    assert_eq!(fake.downloads(), 0);
    assert_eq!(marker_text(&dir).as_deref(), Some(INSTALLING));
}

#[test]
fn managed_counts_once_a_day_across_starts() {
    // Review of R23 PR 4: the machine's once-a-day rule lives in memory, so each start would
    // count it again; the open day's counts say whether it already was.
    let dir = Dir::new("update_managed_daily", &[]);
    let counts = open_counts(&dir, true);
    let fake = FakeUpdater::new(&dir, InstallKind::Package, None, Download::Instant);
    for run in 0..2 {
        let host = start_updating_with(&dir, run, &fake, CURRENT, counting(&counts));
        host.settle();
        host.shutdown();
    }
    let managed = Update {
        result: UpdateResult::Managed,
    };
    assert_eq!(cells(&counts.today().update), [(managed, 1)]);
}
