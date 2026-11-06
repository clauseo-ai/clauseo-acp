//! The quit against the setup thread's start (`start.rs`): a quit that comes first closes the
//! cell, so no `Agents` starts after it and a command answers `internal`; a start that came
//! first is shut down by the quit.

use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::ErrorCode;
use serde_json::json;

use super::MAIN;
use super::claude;
use super::fakes::{command, rig};
use crate::setup::{BackendCell, Setup, SetupBackend};
use crate::test_support::block_on;

#[test]
fn a_quit_before_the_start_means_no_agents_start() {
    let rig = rig(true);
    let (cell, start) = BackendCell::new();
    // The quit comes while the keys load: the cell closes before the setup thread starts.
    assert!(cell.shutdown(Duration::ZERO));
    let mut started = false;
    let filled = start.start_with(|| {
        started = true;
        (Arc::clone(&rig.backend), ())
    });
    assert!(filled.is_none(), "no backend comes after the quit");
    assert!(!started, "`Agents` started after the quit");
    assert_eq!(rig.journal(), Vec::<String>::new());
    // A command answers as with no start at all.
    let setup = Setup::new(cell);
    let install = command("install_agent", &claude(), &json!({}));
    let error = block_on(setup.run(MAIN, install)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::Internal);
}

#[test]
fn a_quit_after_the_start_shuts_the_started_agents_down() {
    let rig = rig(true);
    let (cell, start) = BackendCell::new();
    let filled = start.start_with(|| (Arc::clone(&rig.backend), ()));
    assert!(filled.is_some());
    assert!(cell.shutdown(Duration::ZERO));
    assert!(rig.journal().contains(&"agents.shutdown".to_owned()));
}
