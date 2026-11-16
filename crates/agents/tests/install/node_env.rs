//! No install run gets `NODE_OPTIONS` or `NODE_PATH`: the launcher adds both to every spec,
//! naming a preload and a module folder that do not exist, and the install still finishes, which
//! it could not if the real Node saw `--require` of a missing file.

use std::collections::BTreeMap;
use std::ffi::OsString;

use clauseo_agents::InstallState;

use crate::support::{Fixture, agent_a};

#[test]
fn an_install_runs_without_node_options_or_node_path() {
    let fx = Fixture::new("node-env");
    let missing = fx.data.path().join("missing");
    let preload = format!("--require={}", missing.join("preload.js").display());
    let extra = BTreeMap::from([
        (OsString::from("NODE_OPTIONS"), OsString::from(preload)),
        (OsString::from("NODE_PATH"), missing.into_os_string()),
    ]);
    let (agents, events) = fx.start_adding_env(extra);
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
}
