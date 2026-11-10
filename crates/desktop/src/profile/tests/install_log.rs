//! The support lines of the agent-browser copy at app start (`install_browser`): one line on
//! the profile's lifecycle target per start, saying what happened and naming no path.

use std::sync::{Arc, Mutex, PoisonError};

use clauseo_settings::Locations;

use super::super::{DesktopProfile, LIFECYCLE};
use crate::test_support::Scratch;

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

/// Runs `run` under a subscriber that writes every line, and gives the agent-browser lines.
fn captured(run: impl FnOnce()) -> Vec<String> {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let bytes = lines
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .filter(|line| line.contains("agent-browser"))
        .map(str::to_owned)
        .collect()
}

/// host.log keeps info lines only from a `clauseo_*` crate's `::lifecycle` target.
#[test]
fn the_target_is_one_host_log_keeps() {
    assert!(LIFECYCLE.starts_with("clauseo_desktop::"));
    assert!(LIFECYCLE.ends_with("::lifecycle"));
}

/// A first start copies, a second finds the copy up to date, and a start with no program says
/// it is left out: one info line each on the lifecycle target, with no folder in it.
#[test]
fn each_start_logs_what_the_copy_did_and_no_path() {
    let scratch = Scratch::new("profile-install-log", &["data", "bin", "empty"]);
    scratch.file(
        "bin/agent-browser",
        b"#!/bin/sh\necho 'agent-browser 0.41.1'\n",
    );
    scratch.executable("bin/agent-browser");
    let bin = scratch.dir("bin");
    let empty = scratch.dir("empty");
    let locations = Locations::under_root(&scratch.dir("data"));
    let build = |path: &std::path::Path| {
        captured(|| {
            DesktopProfile::build_with(&locations, None, Some(path.as_os_str())).unwrap();
        })
    };
    let first = build(&bin);
    let second = build(&bin);
    let none = build(&empty);
    for (lines, outcome) in [
        (&first, "copied"),
        (&second, "up_to_date"),
        (&none, "no_sidecar"),
    ] {
        assert_eq!(lines.len(), 1, "{lines:?}");
        let line = &lines[0];
        assert!(line.contains(" INFO "), "{line}");
        assert!(line.contains(LIFECYCLE), "{line}");
        assert!(line.contains(&format!("outcome=\"{outcome}\"")), "{line}");
        let root = scratch.path().to_str().unwrap();
        assert!(!line.contains(root), "{line}");
    }
}
