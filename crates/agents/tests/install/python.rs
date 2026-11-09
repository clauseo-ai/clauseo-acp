//! The Python runtime as the second step of an install job: a fake runtime archive (its
//! `bin/python3` a shell script) served by the test registry under `python.test`, or read from
//! the offline bundle's `python/`. Whatever Python meets, the agent stays `Ready`.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clauseo_agents::testing::{Behaviour, TempDir, runtime_tar_gz, sha512_integrity};
use clauseo_agents::{
    Agents, InstallState, PythonArchive, PythonEntry, PythonFailureKind, PythonState, TestResolver,
    python_command_dir,
};

use crate::support::{
    A_PACKAGES, BASE, Events, Fixture, agent_a, agent_b, node_entry, platform, snapshot, states_of,
    to_hex,
};
use crate::write;

// The run check's cases: a refused start, a failed check, no interpreter.
#[path = "python_run.rs"]
mod run;

const fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// A fake runtime whose `bin/python3` runs `script`, with a read-only-to-be library folder.
fn fake_python(script: &str) -> Vec<u8> {
    let program = format!("#!/bin/sh\n{script}\n");
    runtime_tar_gz(
        "python",
        &[
            ("bin/", 0o755, b""),
            ("bin/python3", 0o755, program.as_bytes()),
            ("lib/python3.13/site-packages/numpy/", 0o755, b""),
            ("lib/python3.13/site-packages/numpy/__init__.py", 0o644, b""),
        ],
    )
    .unwrap()
}

/// The archive's name for this platform and build `build`.
fn archive_name(build: u32) -> String {
    format!(
        "clauseo-python-3.13.18-c{build}-{}.tar.gz",
        platform().node_name()
    )
}

/// Python's entry for `bytes` (its real size and SHA-256) as build `build`.
fn python_entry(bytes: &[u8], build: u32, unpacked: u64) -> PythonEntry {
    use sha2::Digest as _;
    let name = archive_name(build);
    let sha = to_hex(&sha2::Sha256::digest(bytes));
    let platform = platform();
    let archive = PythonArchive::new(
        platform.os,
        platform.arch,
        &name,
        &format!("http://python.test/py/{name}"),
        u64::try_from(bytes.len()).unwrap(),
        unpacked,
        &sha,
    )
    .unwrap();
    PythonEntry::new("3.13.18", vec![archive])
}

/// The fixture with Python's archive `bytes` (build 1) in its snapshot and on the registry.
fn with_python(label: &str, bytes: &[u8]) -> Fixture {
    let mut fx = Fixture::new(label);
    fx.config.snapshot = fx
        .config
        .snapshot
        .clone()
        .with_python(python_entry(bytes, 1, 4000));
    fx.registry
        .route(&format!("/py/{}", archive_name(1)), bytes.to_vec());
    fx
}

fn runtime_dir(fx: &Fixture, build: u32) -> PathBuf {
    let name = archive_name(build);
    fx.data
        .path()
        .join("agents/runtime")
        .join(name.strip_suffix(".tar.gz").unwrap())
}

fn download(fx: &Fixture) -> PathBuf {
    fx.data
        .path()
        .join("agents/downloads")
        .join(archive_name(1))
}

fn python_requests(fx: &Fixture) -> Vec<clauseo_agents::testing::Request> {
    let path = format!("/py/{}", archive_name(1));
    fx.registry
        .requests()
        .into_iter()
        .filter(|request| request.path == path)
        .collect()
}

fn failure_kind(state: &PythonState) -> PythonFailureKind {
    match state {
        PythonState::Failed(failure) => failure.kind,
        other => panic!("not failed: {other:?}"),
    }
}

/// A's install to its `Ready`, then Python's to its end.
fn install_a(agents: &Agents, events: &Events) -> PythonState {
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    events.until_python_end().1
}

fn mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o777
}

#[test]
fn python_installs_after_the_agent_with_its_receipt_and_manifest_and_its_download_goes() {
    let bytes = fake_python("echo 3.13.18");
    let fx = with_python("py-install", &bytes);
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    // A's Missing leaves Python's archive out (and the user's Node: none of Node's); Python's
    // own Missing carries it, so a window adds it once.
    let a_packages: u64 = A_PACKAGES
        .iter()
        .map(|name| u64::try_from(fx.tarballs[name].len()).unwrap())
        .sum();
    let size = u64::try_from(bytes.len()).unwrap();
    assert_eq!(
        a,
        InstallState::Missing {
            download_bytes: a_packages
        }
    );
    assert_eq!(
        events.next_python(),
        PythonState::Missing {
            download_bytes: size
        }
    );
    let last = install_a(&agents, &events);
    assert_eq!(
        last,
        PythonState::Ready {
            version: "3.13.18".to_owned()
        }
    );
    assert_eq!(agents.python_state(), last);
    assert!(matches!(
        agents.state(&agent_a()),
        Some(InstallState::Ready { .. })
    ));
    let dir = runtime_dir(&fx, 1);
    assert!(dir.join("clauseo-python.json").is_file());
    let manifest = std::fs::read_to_string(dir.join("clauseo-python-manifest.txt")).unwrap();
    assert_eq!(manifest.lines().count(), 2, "{manifest}");
    assert_eq!(mode(&dir), 0o555);
    assert_eq!(mode(&dir.join("lib/python3.13/site-packages/numpy")), 0o555);
    assert_eq!(python_command_dir(&fx.config), Some(dir.join("bin")));
    assert!(!download(&fx).exists(), "the download is removed at Ready");
    assert_eq!(python_requests(&fx).len(), 1);
    // B's install finds Python there and fetches nothing more.
    agents.install(&agent_b());
    let (_, last) = events.until_end(&agent_b());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(python_requests(&fx).len(), 1);
    // A new start reads it as Ready.
    drop(agents);
    let (_again, events) = fx.start();
    events.first_states();
    assert!(matches!(events.next_python(), PythonState::Ready { .. }));
}

#[test]
fn a_tampered_archive_fails_python_after_three_tries_and_the_agent_stays_ready() {
    let bytes = fake_python("echo 3.13.18");
    let fx = with_python("py-tamper", &bytes);
    let path = format!("/py/{}", archive_name(1));
    for _ in 0..3 {
        fx.registry.then(&path, Behaviour::Tamper);
    }
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    fx.timer.fire_next(secs(2));
    fx.timer.fire_next(secs(8));
    let (_, last) = events.until_python_end();
    assert_eq!(failure_kind(&last), PythonFailureKind::Failed);
    assert!(matches!(
        agents.state(&agent_a()),
        Some(InstallState::Ready { .. })
    ));
    assert!(agents.installed(&agent_a()).is_some());
    assert_eq!(python_command_dir(&fx.config), None);
    assert_eq!(states_of(&events.drain(), &agent_a()), []);
    // A retry of the Ready agent tries Python again.
    agents.retry(&agent_a(), false);
    let (_, last) = events.until_python_end();
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
}

#[test]
fn a_retry_of_any_ready_agent_retries_python_and_leaves_that_agent_s_install_alone() {
    let bytes = fake_python("echo 3.13.18");
    let fx = with_python("py-retry-any", &bytes);
    let path = format!("/py/{}", archive_name(1));
    for _ in 0..6 {
        fx.registry.then(&path, Behaviour::Tamper);
    }
    let (agents, events) = fx.start();
    events.first_states();
    // A's job fails Python, then B's job installs B and fails Python again.
    agents.install(&agent_a());
    events.until_end(&agent_a());
    fx.timer.fire_next(secs(2));
    fx.timer.fire_next(secs(8));
    assert_eq!(
        failure_kind(&events.until_python_end().1),
        PythonFailureKind::Failed
    );
    agents.install(&agent_b());
    let (_, last) = events.until_end(&agent_b());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    fx.timer.fire_next(secs(2));
    fx.timer.fire_next(secs(8));
    assert_eq!(
        failure_kind(&events.until_python_end().1),
        PythonFailureKind::Failed
    );
    let b_installed = agents.installed(&agent_b());
    assert!(b_installed.is_some());
    let other_requests = |fx: &Fixture| {
        fx.registry
            .requests()
            .into_iter()
            .filter(|request| request.path != path)
            .count()
    };
    let before = other_requests(&fx);
    // A retry sent with B's id (the window may send any ready plan's) retries Python.
    agents.retry(&agent_b(), false);
    let (_, last) = events.until_python_end();
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    // B's own install is not redone: nothing but Python fetched, B never left Ready, A
    // untouched, and B's install is the same.
    assert_eq!(other_requests(&fx), before);
    let seen = events.drain();
    assert!(
        states_of(&seen, &agent_b())
            .iter()
            .all(|state| matches!(state, InstallState::Ready { .. })),
        "{seen:?}"
    );
    assert_eq!(states_of(&seen, &agent_a()), []);
    assert_eq!(agents.installed(&agent_b()), b_installed);
}

#[test]
fn a_switch_pauses_python_not_the_agent_and_the_next_job_resumes_it_by_range() {
    let bytes = fake_python("echo 3.13.18");
    let fx = with_python("py-switch", &bytes);
    let path = format!("/py/{}", archive_name(1));
    fx.registry.then(&path, Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), |s| matches!(s, InstallState::Ready { .. }));
    fx.registry.wait_for_stalls(1);
    events
        .until_python(|s| matches!(s, PythonState::Downloading { received, .. } if *received > 0));
    agents.install(&agent_b());
    events.until_python(|s| matches!(s, PythonState::Missing { .. }));
    assert!(matches!(
        agents.state(&agent_a()),
        Some(InstallState::Ready { .. })
    ));
    fx.registry.release();
    let (seen, last) = events.until_end(&agent_b());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert!(
        !states_of(&seen, &agent_a()).contains(&InstallState::Paused),
        "the agent stays Ready"
    );
    let (_, last) = events.until_python_end();
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    let half = bytes.len() / 2;
    let ranges: Vec<_> = python_requests(&fx)
        .into_iter()
        .map(|request| request.range)
        .collect();
    assert_eq!(ranges, [None, Some(format!("bytes={half}-"))]);
}

#[test]
fn offline_python_waits_30_seconds_and_resumes_while_the_agent_is_ready() {
    let bytes = fake_python("echo 3.13.18");
    let mut fx = with_python("py-offline", &bytes);
    let online = Arc::new(AtomicBool::new(false));
    let addr = fx.registry.addr();
    let flag = Arc::clone(&online);
    fx.config.install.resolver = Some(TestResolver(Arc::new(move |name: &str| {
        if name != "python.test" || flag.load(Ordering::SeqCst) {
            Ok(vec![addr])
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no {name}"),
            ))
        }
    })));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), |s| matches!(s, InstallState::Ready { .. }));
    let seen = events.until_python(|s| matches!(s, PythonState::Waiting { .. }));
    let Some(PythonState::Waiting { log, .. }) = seen.last() else {
        unreachable!()
    };
    let log = log.to_string();
    assert!(log.contains("host: python.test"), "{log}");
    assert!(matches!(
        agents.state(&agent_a()),
        Some(InstallState::Ready { .. })
    ));
    online.store(true, Ordering::SeqCst);
    fx.timer.fire_next(secs(30));
    let (_, last) = events.until_python_end();
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
}

/// The offline bundle for agent A: its tarballs, and Python's archive when `python` is given.
fn bundle_for_a(fx: &Fixture, python: Option<&[u8]>) -> TempDir {
    let bundle = TempDir::new("py-bundle").unwrap();
    for name in A_PACKAGES {
        let bytes = &fx.tarballs[name];
        let integrity = sha512_integrity(bytes);
        let b64 = integrity.strip_prefix("sha512-").unwrap();
        let digest = {
            use base64::Engine as _;
            to_hex(
                &base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap(),
            )
        };
        let path = bundle.path().join("npm").join(format!("{digest}.tgz"));
        write::write(&path, bytes).unwrap();
    }
    if let Some(bytes) = python {
        let path = bundle.path().join("python").join(archive_name(1));
        write::write(&path, bytes).unwrap();
    }
    bundle
}

#[test]
fn a_bundle_with_python_installs_it_with_no_network() {
    let bytes = fake_python("echo 3.13.18");
    let mut fx = with_python("py-bundle", &bytes);
    let bundle = bundle_for_a(&fx, Some(&bytes));
    fx.config.install.bundle = Some(bundle.path().to_owned());
    let (agents, events) = fx.start();
    events.first_states();
    let last = install_a(&agents, &events);
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    assert_eq!(fx.registry.requests(), []);
    assert_eq!(fx.lookups.names(), Vec::<String>::new());
    assert!(bundle.path().join("python").join(archive_name(1)).is_file());
    assert!(!download(&fx).exists(), "the copy in downloads/ is removed");
}

#[test]
fn a_bundle_without_python_fails_python_alone() {
    let bytes = fake_python("echo 3.13.18");
    let mut fx = with_python("py-bundle-none", &bytes);
    let bundle = bundle_for_a(&fx, None);
    fx.config.install.bundle = Some(bundle.path().to_owned());
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    fx.timer.fire_next(secs(2));
    fx.timer.fire_next(secs(8));
    let (_, last) = events.until_python_end();
    assert_eq!(failure_kind(&last), PythonFailureKind::Failed);
    let PythonState::Failed(failure) = &last else {
        unreachable!()
    };
    let log = failure.log.to_string();
    assert!(log.contains("the offline bundle has no"), "{log}");
    assert_eq!(fx.registry.requests(), []);
    assert!(matches!(
        agents.state(&agent_a()),
        Some(InstallState::Ready { .. })
    ));
}

#[test]
fn no_python_for_the_platform_skips_it_and_the_agent_installs_as_before() {
    let fx = Fixture::new("py-none");
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    let a_packages: u64 = A_PACKAGES
        .iter()
        .map(|name| u64::try_from(fx.tarballs[name].len()).unwrap())
        .sum();
    assert_eq!(
        a,
        InstallState::Missing {
            download_bytes: a_packages
        }
    );
    assert_eq!(events.next_python(), PythonState::Unavailable);
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert_eq!(agents.python_state(), PythonState::Unavailable);
    assert_eq!(python_command_dir(&fx.config), None);
    assert!(!fx.data.path().join("agents/runtime").exists());
}

#[test]
fn the_disk_check_counts_python() {
    let bytes = fake_python("echo 3.13.18");
    let mut fx = with_python("py-disk", &bytes);
    fx.config.snapshot =
        snapshot(BASE, node_entry(None), 10_000).with_python(python_entry(&bytes, 1, 1 << 60));
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("{last:?}");
    };
    assert_eq!(failure.kind, clauseo_agents::FailureKind::DiskFull);
    assert!(failure.need_mb.unwrap() > (1_u64 << 60) / 1_000_000);
    assert_eq!(fx.registry.requests(), []);
}

#[test]
fn an_update_s_new_archive_removes_the_old_read_only_runtime_at_the_next_start() {
    let bytes = fake_python("echo 3.13.18");
    let mut fx = with_python("py-stale", &bytes);
    let (agents, events) = fx.start();
    events.first_states();
    let last = install_a(&agents, &events);
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    let old = runtime_dir(&fx, 1);
    assert_eq!(mode(&old), 0o555);
    drop(agents);
    // The app update: build 2's archive.
    fx.config.snapshot = fx
        .config
        .snapshot
        .clone()
        .with_python(python_entry(&bytes, 2, 4000));
    let (_agents, events) = fx.start();
    events.first_states();
    assert!(matches!(events.next_python(), PythonState::Missing { .. }));
    assert!(!old.exists(), "the old runtime is removed");
}
