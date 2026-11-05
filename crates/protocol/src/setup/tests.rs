use std::marker::PhantomData;

use serde::Serialize;

use super::install::cut_line;
use super::{InstallLog, MAX_INSTALL_LOG_LINE_BYTES, MAX_INSTALL_LOG_LINES, Secret, SetupCommand};

/// Asks, at compile time, whether `T` has `Serialize`: autoref picks [`Serializes`] when it
/// does and falls back to [`DoesNot`] when it does not.
struct Probe<T>(PhantomData<T>);

/// Chosen for a `T` with `Serialize`.
trait Serializes {
    fn serializes(&self) -> bool {
        true
    }
}

impl<T: Serialize> Serializes for &Probe<T> {}

/// Chosen for any other `T`.
trait DoesNot {
    fn serializes(&self) -> bool {
        false
    }
}

impl<T> DoesNot for Probe<T> {}

/// Whether `$t` has `Serialize`, through [`Probe`]: the call must start from `&&Probe`.
macro_rules! serializes {
    ($t:ty) => {{
        let probe: &&Probe<$t> = &&Probe(PhantomData);
        probe.serializes()
    }};
}

#[test]
fn the_probe_sees_serialize() {
    assert!(serializes!(String));
}

#[test]
fn a_secret_and_a_setup_command_never_serialize() {
    assert!(!serializes!(Secret));
    assert!(!serializes!(SetupCommand));
}

#[test]
fn a_long_line_is_cut_at_a_character_boundary() {
    let line = "é".repeat(MAX_INSTALL_LOG_LINE_BYTES);
    let cut = cut_line(line);
    assert!(cut.len() <= MAX_INSTALL_LOG_LINE_BYTES);
    assert!(cut.len() >= MAX_INSTALL_LOG_LINE_BYTES - 1);
    assert!(cut.chars().all(|c| c == 'é'));
    assert_eq!(cut_line("short".to_owned()), "short");
}

#[test]
fn a_long_log_keeps_its_heading_and_its_end() {
    let lines = (0..200).map(|n| format!("line {n}"));
    let log = InstallLog::new(lines);
    let kept = log.lines();
    assert_eq!(kept.len(), MAX_INSTALL_LOG_LINES);
    assert_eq!(kept.first().map(String::as_str), Some("line 0"));
    assert_eq!(kept.get(1).map(String::as_str), Some("line 1"));
    assert_eq!(
        kept.get(2).map(String::as_str),
        Some("… 137 lines left out")
    );
    assert_eq!(kept.get(3).map(String::as_str), Some("line 139"));
    assert_eq!(kept.last().map(String::as_str), Some("line 199"));
}

#[test]
fn a_short_log_is_kept_whole() {
    let lines = vec![
        "Clauseo download log".to_owned(),
        "agent: claude-acp".to_owned(),
    ];
    assert_eq!(InstallLog::new(lines.clone()).lines(), lines.as_slice());
}
