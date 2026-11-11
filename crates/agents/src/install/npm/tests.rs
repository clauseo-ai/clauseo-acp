//! The in-place retry of a step that met a sharing or lock error: which errors count, how many
//! retries and waits, the pause, and its log line.

use std::cell::RefCell;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_protocol::AgentId;

use super::remove::{REMOVE_WAITS, remove_retried};
use super::{
    Ended, RENAME_WAITS, in_place, missing_agent, missing_archive, npm_code, windows_busy,
};
use crate::config::AgentsConfig;
use crate::platform::{Arch, Os, Platform};
use crate::state::FailureKind;

fn agent() -> AgentId {
    AgentId::new("agent-a").unwrap()
}

fn sharing() -> io::Error {
    io::Error::from_raw_os_error(32)
}

/// The log lines written while `work` runs on this thread.
fn logged(work: impl FnOnce()) -> String {
    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);
    impl io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, work);
    let bytes = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    String::from_utf8(bytes).unwrap()
}

/// Runs `in_place` over a step answering `results` in turn, Windows' rule deciding: the
/// outcome, the step's runs and the waits asked for.
fn run_step(
    results: Vec<io::Result<u8>>,
    wait_answers: bool,
) -> (Result<u8, Ended<io::Error>>, usize, Vec<Duration>) {
    let results = RefCell::new(results.into_iter());
    let runs = RefCell::new(0);
    let waits = RefCell::new(Vec::new());
    let out = futures::executor::block_on(in_place(
        "the rename",
        &agent(),
        &RENAME_WAITS,
        async |pause| {
            waits.borrow_mut().push(pause);
            wait_answers
        },
        async || {
            *runs.borrow_mut() += 1;
            results.borrow_mut().next().unwrap_or(Ok(0))
        },
        |err: &io::Error| windows_busy(err, Os::Win32),
    ));
    (out, runs.into_inner(), waits.into_inner())
}

#[test]
fn a_sharing_violation_is_retried_in_place_and_logged() {
    let mut outcome = None;
    let text =
        logged(|| outcome = Some(run_step(vec![Err(sharing()), Err(sharing()), Ok(7)], true)));
    let (out, runs, waits) = outcome.unwrap();
    assert_eq!(out.unwrap(), 7);
    assert_eq!(runs, 3);
    assert_eq!(waits, RENAME_WAITS[..2]);
    assert_eq!(
        text.matches("agent install: a sharing or lock error; retrying the step in place")
            .count(),
        2,
        "{text}"
    );
    assert!(text.contains("Windows error 32"), "{text}");
    assert!(
        text.contains("wait_ms=250") && text.contains("wait_ms=500"),
        "{text}"
    );
}

#[test]
fn the_retries_stop_after_the_last_wait_and_keep_the_error() {
    let errors = (0..20).map(|_| Err(sharing())).collect();
    let (out, runs, waits) = run_step(errors, true);
    let Err(Ended::Failed(err)) = out else {
        panic!("{out:?}");
    };
    assert_eq!(err.raw_os_error(), Some(32));
    assert_eq!(runs, RENAME_WAITS.len() + 1);
    assert_eq!(waits, RENAME_WAITS);
}

#[test]
fn another_error_ends_at_once() {
    let missing = io::Error::from_raw_os_error(2);
    let (out, runs, waits) = run_step(vec![Err(missing)], true);
    let Err(Ended::Failed(err)) = out else {
        panic!("{out:?}");
    };
    assert_eq!(err.raw_os_error(), Some(2));
    assert_eq!((runs, waits), (1, Vec::new()));
}

/// A pause during a retry's wait ends the step as paused, not with the sharing error (which
/// for npm is a try to count, and would read as npm's failure).
#[test]
fn a_pause_during_a_retry_wait_ends_as_paused_not_with_the_error() {
    let (out, runs, waits) = run_step(vec![Err(sharing()), Ok(1)], false);
    assert!(matches!(out, Err(Ended::Paused)), "{out:?}");
    assert_eq!((runs, waits), (1, RENAME_WAITS[..1].to_vec()));
}

/// The snapshot not knowing the agent and Node's archive missing are no set-up failures, even
/// though they are met in the set-up step: they fail as `Failed`.
#[test]
fn a_missing_agent_or_archive_is_no_set_up_failure() {
    let config = AgentsConfig::new("/data".into(), Platform::new(Os::Linux, Arch::X64));
    for err in [missing_agent(&config), missing_archive(&config)] {
        assert_eq!(err.in_setup().failed_kind(), FailureKind::Failed);
    }
}

#[test]
fn only_windows_sharing_lock_and_access_errors_count() {
    for code in [5, 32, 33] {
        let err = io::Error::from_raw_os_error(code);
        assert_eq!(
            windows_busy(&err, Os::Win32),
            Some(format!("Windows error {code}"))
        );
        assert_eq!(
            windows_busy(&err, Os::Linux),
            None,
            "{code} means another thing"
        );
        assert_eq!(windows_busy(&err, Os::Darwin), None);
    }
    for code in [2, 3, 112] {
        let err = io::Error::from_raw_os_error(code);
        assert_eq!(windows_busy(&err, Os::Win32), None, "{code}");
    }
    assert_eq!(windows_busy(&io::Error::other("x"), Os::Win32), None);
}

#[test]
fn npm_s_code_is_the_word_after_its_prefix() {
    assert_eq!(npm_code("npm error code EBUSY"), Some("EBUSY"));
    assert_eq!(npm_code("npm error code EPERM "), Some("EPERM"));
    assert_eq!(npm_code("npm error code "), None);
    assert_eq!(npm_code("npm error errno -4082"), None);
}

/// Runs `remove_retried` on Windows' rule over removals answering `results` in turn: the
/// removals tried, the waits asked for, and the log.
fn removal(results: Vec<io::Result<()>>) -> (usize, Vec<Duration>, String) {
    let results = RefCell::new(results.into_iter());
    let tries = RefCell::new(0);
    let waits = RefCell::new(Vec::new());
    let tmp = std::path::Path::new("install").join(".tmp-abc-1");
    let text = logged(|| {
        futures::executor::block_on(remove_retried(
            &agent(),
            Os::Win32,
            &tmp,
            async |pause| waits.borrow_mut().push(pause),
            || {
                *tries.borrow_mut() += 1;
                results.borrow_mut().next().unwrap_or(Ok(()))
            },
        ));
    });
    (tries.into_inner(), waits.into_inner(), text)
}

/// A temp folder an antivirus holds a moment is removed on a retry, with no warning that it
/// stays.
#[test]
fn a_held_temp_folder_is_removed_on_a_retry() {
    let (tries, waits, text) = removal(vec![Err(sharing()), Err(sharing()), Ok(())]);
    assert_eq!(tries, 3);
    assert_eq!(waits, REMOVE_WAITS[..2]);
    assert!(!text.contains("could not be removed"), "{text}");
}

/// A temp folder still held after the last retry (2 s) is logged with its error's code and its
/// name, and left for the next start's clean-up; another error is logged at once.
#[test]
fn a_temp_folder_that_will_not_go_is_logged_after_the_retries() {
    let (tries, waits, text) = removal((0..10).map(|_| Err(sharing())).collect());
    assert_eq!(tries, REMOVE_WAITS.len() + 1);
    assert_eq!(waits, REMOVE_WAITS);
    assert_eq!(
        REMOVE_WAITS.iter().sum::<Duration>(),
        Duration::from_secs(2)
    );
    assert_eq!(
        text.matches("agent install: the temp folder could not be removed")
            .count(),
        1,
        "{text}"
    );
    assert!(
        text.contains("folder=.tmp-abc-1") && text.contains("os error 32"),
        "{text}"
    );
    let (tries, waits, text) = removal(vec![Err(io::Error::from_raw_os_error(2))]);
    assert_eq!((tries, waits), (1, Vec::new()));
    assert!(text.contains("could not be removed"), "{text}");
}
