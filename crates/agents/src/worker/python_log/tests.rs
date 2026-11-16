use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_protocol::AgentId;

use super::{LIFECYCLE, changed};
use crate::log::InstallLog;
use crate::state::{FailureKind, PythonBlocker, PythonFailure, PythonFailureKind, PythonState};

/// What a test's subscriber wrote.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Runs each state of `states` through [`changed`] in turn, from `Unavailable`; the lines on
/// the lifecycle target.
fn logged(states: &[PythonState]) -> Vec<String> {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let mut began: Option<Duration> = None;
        let ticks = crate::worker::Ticks::default();
        let mut old = PythonState::Unavailable;
        for state in states {
            changed(&old, state, &mut began, &ticks);
            old = state.clone();
        }
    });
    let text = String::from_utf8(lines.0.lock().unwrap().clone()).unwrap();
    text.lines()
        .filter(|line| line.contains(LIFECYCLE))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_target_is_one_host_log_keeps() {
    assert!(LIFECYCLE.starts_with("clauseo_") && LIFECYCLE.ends_with("::lifecycle"));
}

/// An install from `Missing` to `Ready`: one line per change of kind, the download's sizes as
/// it starts and as it ends but at no chunk between, and the version with how long it took.
#[test]
fn an_install_logs_each_state_once_with_its_sizes_and_time() {
    let downloading = |received| PythonState::Downloading {
        received,
        total: 900,
    };
    let lines = logged(&[
        PythonState::Missing {
            download_bytes: 900,
        },
        downloading(100),
        downloading(400),
        downloading(900),
        PythonState::Installing,
        PythonState::Ready {
            version: "3.13.18".to_owned(),
        },
    ]);
    let want: [&[&str]; 5] = [
        &[
            "state changed",
            "from=\"unavailable\"",
            "state=\"missing\"",
            "download_bytes=900",
        ],
        &["state=\"downloading\"", "received=100", "total=900"],
        &["download ended", "received=900", "total=900"],
        &["from=\"downloading\"", "state=\"installing\""],
        &["state=\"ready\"", "version=3.13.18", "took_ms="],
    ];
    assert_eq!(lines.len(), want.len(), "{lines:#?}");
    for (line, fields) in lines.iter().zip(want) {
        for field in fields {
            assert!(line.contains(field), "{field} in {line}");
        }
    }
    assert!(
        !lines.iter().any(|line| line.contains("received=400")),
        "{lines:#?}"
    );
}

/// A failure says its cause and code, and a policy that refused the run; never the log's
/// lines, which can name the data folder. Waiting offline says why.
#[test]
fn a_failure_logs_its_cause_and_code_and_no_path() {
    let data_dir = std::env::temp_dir().join("clauseo-python-log-data");
    let mut log = InstallLog::new(&AgentId::new("codex-acp").unwrap());
    log.push(format!("python: {}", data_dir.join("downloads").display()));
    let failed = |kind, blocked_by| {
        PythonState::Failed(PythonFailure {
            kind,
            blocked_by,
            log: log.clone(),
        })
    };
    let lines = logged(&[
        PythonState::Waiting {
            reason: FailureKind::Offline,
            log: log.clone(),
        },
        failed(PythonFailureKind::DiskFull, None),
        PythonState::Installing,
        failed(
            PythonFailureKind::RunBlocked,
            Some(PythonBlocker::AppControl),
        ),
    ]);
    assert_eq!(lines.len(), 4, "{lines:#?}");
    assert!(lines[0].contains("state=\"waiting\"") && lines[0].contains("reason=\"offline\""));
    for field in ["cause=\"disk_full\"", "code=\"agent.install_disk_full\""] {
        assert!(lines[1].contains(field), "{field} in {}", lines[1]);
    }
    for field in ["cause=\"run_blocked\"", "blocked_by=\"app_control\""] {
        assert!(lines[3].contains(field), "{field} in {}", lines[3]);
    }
    let text = lines.join("\n");
    assert!(!text.contains(&*data_dir.to_string_lossy()), "{text}");
    assert!(!text.contains("clauseo-python-log-data"), "{text}");
}
