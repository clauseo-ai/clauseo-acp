//! Python's install cut short by a quit, found at the next start: the disk as the quit left it
//! (a whole archive, a part of one, a bad one, an unpack's leftovers), or a real quit mid
//! download. The start resumes it with no press, reusing what checks against the pin; a Python
//! the OS refused stays refused at the next start until Try again.

use clauseo_agents::PythonBlocker;

use super::run::refused_with;
use super::*;

/// A ready, Python missing, and the disk as `seed` leaves it; then a start, and Python's end.
fn restart_with(label: &str, bytes: &[u8], seed: impl Fn(&Fixture)) -> (Fixture, PythonState) {
    let fx = b_found_before_python(label, bytes);
    seed(&fx);
    let (agents, events) = fx.start();
    let (_, b) = events.first_states();
    assert!(matches!(b, InstallState::Ready { .. }), "{b:?}");
    assert!(matches!(events.next_python(), PythonState::Missing { .. }));
    // No press: the start's own job.
    let (seen, last) = events.until_python_end();
    assert!(seen.contains(&PythonState::Installing), "{seen:?}");
    drop(agents);
    (fx, last)
}

fn write_download(fx: &Fixture, name: &str, bytes: &[u8]) {
    let dir = fx.data.path().join("agents/downloads");
    write::write(&dir.join(name), bytes).unwrap();
}

fn ranges(fx: &Fixture) -> Vec<Option<String>> {
    python_requests(fx).into_iter().map(|r| r.range).collect()
}

#[test]
fn closed_after_the_download_python_installs_at_the_next_start_from_the_archive_kept() {
    let bytes = fake_python("echo 3.13.18");
    let (fx, last) = restart_with("py-restart-whole", &bytes, |fx| {
        write_download(fx, &archive_name(1), &bytes);
    });
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    assert_eq!(ranges(&fx), Vec::<Option<String>>::new(), "nothing fetched");
    assert!(!download(&fx).exists(), "the archive goes at Ready");
}

#[test]
fn closed_mid_download_python_resumes_its_part_at_the_next_start() {
    let bytes = fake_python("echo 3.13.18");
    let half = bytes.len() / 2;
    let (fx, last) = restart_with("py-restart-part", &bytes, |fx| {
        write_download(fx, &format!("{}.part", archive_name(1)), &bytes[..half]);
    });
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    assert_eq!(ranges(&fx), [Some(format!("bytes={half}-"))]);
}

#[test]
fn a_kept_archive_that_does_not_match_the_pin_is_deleted_and_fetched_again() {
    let bytes = fake_python("echo 3.13.18");
    let (fx, last) = restart_with("py-restart-bad", &bytes, |fx| {
        write_download(fx, &archive_name(1), &vec![b'x'; bytes.len()]);
    });
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    assert_eq!(ranges(&fx), [None], "fetched whole, once");
}

#[test]
fn closed_mid_unpack_the_leftovers_go_and_the_unpack_is_redone() {
    let bytes = fake_python("echo 3.13.18");
    let (fx, last) = restart_with("py-restart-unpack", &bytes, |fx| {
        write_download(fx, &archive_name(1), &bytes);
        let dir = runtime_dir(fx, 1);
        let tmp = dir.with_file_name(format!(".tmp-{}", dir.file_name().unwrap().display()));
        for folder in [&dir, &tmp] {
            write::write(&folder.join("bin/half"), b"cut short").unwrap();
        }
        // An older pin's archive, left by the build before an update.
        write_download(fx, &archive_name(0), b"old");
    });
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    assert_eq!(ranges(&fx), Vec::<Option<String>>::new(), "nothing fetched");
    let dir = runtime_dir(&fx, 1);
    assert!(!dir.join("bin/half").exists(), "the cut-short tree is gone");
    assert!(dir.join("clauseo-python.json").is_file());
    let runtime = dir.parent().unwrap();
    let names: Vec<_> = std::fs::read_dir(runtime)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(
        names
            .iter()
            .all(|name| !name.to_string_lossy().starts_with(".tmp-")),
        "{names:?}"
    );
    let old = fx
        .data
        .path()
        .join("agents/downloads")
        .join(archive_name(0));
    assert!(!old.exists(), "the older pin's archive goes at the start");
}

#[test]
fn a_real_quit_mid_download_is_resumed_at_the_next_start_with_no_press() {
    let bytes = fake_python("echo 3.13.18");
    let fx = with_python("py-restart-quit", &bytes);
    fx.registry
        .then(&format!("/py/{}", archive_name(1)), Behaviour::Stall);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    events.until(&agent_a(), |s| matches!(s, InstallState::Ready { .. }));
    fx.registry.wait_for_stalls(1);
    events
        .until_python(|s| matches!(s, PythonState::Downloading { received, .. } if *received > 0));
    // The quit: every job cancelled; the stalled read ends once the rest is sent.
    let _: bool = agents.shutdown(Duration::ZERO);
    fx.registry.release();
    assert!(agents.shutdown(secs(20)), "the job ended");
    drop(agents);
    let (agents, events) = fx.start();
    let (a, _) = events.first_states();
    assert!(matches!(a, InstallState::Ready { .. }), "{a:?}");
    assert!(matches!(events.next_python(), PythonState::Missing { .. }));
    let (_, last) = events.until_python_end();
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    // The first request was cut short: the start either kept a whole file (no request) or
    // resumed its part; never a second download from the start.
    let ranges = ranges(&fx);
    assert!(ranges.len() <= 2, "{ranges:?}");
    assert_eq!(ranges.first(), Some(&None), "{ranges:?}");
    if let Some(second) = ranges.get(1) {
        assert!(
            second.as_deref().is_some_and(|r| r != "bytes=0-"),
            "{ranges:?}"
        );
    }
    drop(agents);
}

#[test]
fn a_python_the_os_refused_stays_refused_at_the_next_start_until_try_again() {
    let bytes = fake_python("echo 3.13.18");
    let fx = with_python("py-restart-blocked", &bytes);
    let (agents, _events, last) =
        refused_with(&fx, || io::Error::from(io::ErrorKind::PermissionDenied));
    assert_eq!(failure_kind(&last), PythonFailureKind::RunBlocked);
    drop(agents);
    // The next start says so again, and starts nothing.
    let (agents, events) = fx.start();
    events.first_states();
    let first = events.next_python();
    assert_eq!(failure_kind(&first), PythonFailureKind::RunBlocked);
    let PythonState::Failed(failure) = &first else {
        unreachable!()
    };
    assert_eq!(failure.blocked_by, Some(PythonBlocker::Unknown));
    // A's `Ready` again tells the test the worker has handled the start before it.
    agents.install(&agent_a());
    events.until(&agent_a(), |s| matches!(s, InstallState::Ready { .. }));
    assert_eq!(agents.python_state(), first, "no job started");
    // Try again runs it: this launcher lets it start.
    agents.retry(&agent_a(), false);
    let (_, last) = events.until_python_end();
    assert!(matches!(last, PythonState::Ready { .. }), "{last:?}");
    assert!(!fx.data.path().join("agents/.python-blocked").exists());
}
